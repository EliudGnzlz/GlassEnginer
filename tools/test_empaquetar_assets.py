"""Tests contra la CLI fijada de tools/empaquetar_assets.py.

Ejecutar con: python3 -m unittest discover -s tools -p "test_*.py"
No usa pytest ni dependencias fuera de la stdlib. No toca el árbol real de assets/.
"""

import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
SCRIPT = REPO_ROOT / "tools" / "empaquetar_assets.py"
WORKFLOW = REPO_ROOT / ".github" / "workflows" / "release.yml"


def run(*args, cwd=None):
    cmd = [sys.executable, str(SCRIPT), *args]
    return subprocess.run(
        cmd,
        cwd=str(cwd) if cwd else None,
        capture_output=True,
        text=True,
    )


def write_bytes(path: Path, size: int, filler: bytes = b"\x00"):
    path.parent.mkdir(parents=True, exist_ok=True)
    with open(path, "wb") as f:
        f.write(filler * size)


class TestCopiar(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.origen = Path(self.tmp.name) / "origen"
        self.destino = Path(self.tmp.name) / "destino"
        self.origen.mkdir()

    def test_copiar_excluye_gemelos_y_prototype(self):
        write_bytes(self.origen / "textura.png", 10)
        write_bytes(self.origen / "textura.ktx2", 20)

        write_bytes(self.origen / "sinpar.jpg", 15)

        write_bytes(self.origen / "mayus.PNG", 12)
        write_bytes(self.origen / "mayus.ktx2", 22)

        write_bytes(self.origen / "foto.jpeg", 13)
        write_bytes(self.origen / "foto.ktx2", 23)

        write_bytes(self.origen / "modelo.glb", 30)
        write_bytes(self.origen / "suelto.ktx2", 40)

        proto = self.origen / "polygon-prototype"
        write_bytes(proto / "dentro.png", 5)
        proto_deep = self.origen / "a" / "b" / "polygon-prototype"
        write_bytes(proto_deep / "hondo.jpg", 6)

        result = run("copiar", str(self.origen), str(self.destino))
        self.assertEqual(result.returncode, 0, result.stderr)

        data = json.loads(result.stdout)

        self.assertFalse((self.destino / "textura.png").exists())
        self.assertTrue((self.destino / "textura.ktx2").exists())

        self.assertTrue((self.destino / "sinpar.jpg").exists())

        self.assertFalse((self.destino / "mayus.PNG").exists())
        self.assertTrue((self.destino / "mayus.ktx2").exists())

        self.assertFalse((self.destino / "foto.jpeg").exists())
        self.assertTrue((self.destino / "foto.ktx2").exists())

        self.assertTrue((self.destino / "modelo.glb").exists())
        self.assertTrue((self.destino / "suelto.ktx2").exists())

        self.assertFalse((self.destino / "polygon-prototype").exists())
        self.assertFalse((self.destino / "a" / "b" / "polygon-prototype").exists())

        motivos = {e["motivo"] for e in data["excluidos"]}
        self.assertIn("gemelo-ktx2", motivos)
        self.assertIn("polygon-prototype", motivos)

        gemelos = [e for e in data["excluidos"] if e["motivo"] == "gemelo-ktx2"]
        rutas_gemelos = {e["ruta"] for e in gemelos}
        self.assertEqual(rutas_gemelos, {"textura.png", "mayus.PNG", "foto.jpeg"})
        for e in gemelos:
            self.assertNotIn("\\", e["ruta"])

        prototipos = [e for e in data["excluidos"] if e["motivo"] == "polygon-prototype"]
        self.assertEqual(len(prototipos), 2)

        copiados_esperados = {
            "sinpar.jpg", "textura.ktx2", "mayus.ktx2", "foto.ktx2",
            "modelo.glb", "suelto.ktx2",
        }
        self.assertEqual(data["copiados"], len(copiados_esperados))

        bytes_esperados = 15 + 20 + 22 + 23 + 30 + 40
        self.assertEqual(data["bytes_copiados"], bytes_esperados)

    def test_copiar_origen_inexistente(self):
        result = run("copiar", str(self.origen / "no_existe"), str(self.destino))
        self.assertEqual(result.returncode, 2)


class TestComprobar(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.base = Path(self.tmp.name)

    def _presupuesto(self, nombre, limites):
        path = self.base / nombre
        path.write_text(json.dumps({
            "version": 1,
            "notas": "test",
            "politica": "test",
            "limites_bytes": limites,
        }))
        return path

    def _payload(self, nombre, assets_bytes, otros_bytes=0):
        payload = self.base / nombre
        if otros_bytes:
            write_bytes(payload / "glass_app.bin", otros_bytes)
        write_bytes(payload / "assets" / "dato.bin", assets_bytes)
        return payload

    def test_todos_los_limites_pasan(self):
        payload = self._payload("payload_ok", assets_bytes=100, otros_bytes=50)
        presupuesto = self._presupuesto("p_ok.json", {
            "payload_total": 1_000_000,
            "assets": 1_000_000,
            "zip_engine": 1_000_000,
        })
        result = run("comprobar", "--payload", str(payload), "--presupuesto", str(presupuesto))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("PASS", result.stdout)
        self.assertNotIn("::error::", result.stdout)

    def test_payload_excede_limite(self):
        payload = self._payload("payload_grande", assets_bytes=100, otros_bytes=5000)
        presupuesto = self._presupuesto("p_bajo.json", {
            "payload_total": 10,
            "assets": 1_000_000,
            "zip_engine": 1_000_000,
        })
        result = run("comprobar", "--payload", str(payload), "--presupuesto", str(presupuesto))
        self.assertEqual(result.returncode, 1)
        self.assertIn("::error::", result.stdout)
        self.assertIn("payload_total", result.stdout)

    def test_limite_exacto_pasa(self):
        payload = self._payload("payload_exacto", assets_bytes=100, otros_bytes=0)
        total = sum(f.stat().st_size for f in payload.rglob("*") if f.is_file())
        assets_total = sum(
            f.stat().st_size for f in (payload / "assets").rglob("*") if f.is_file()
        )
        presupuesto = self._presupuesto("p_exacto.json", {
            "payload_total": total,
            "assets": assets_total,
            "zip_engine": 1_000_000,
        })
        result = run("comprobar", "--payload", str(payload), "--presupuesto", str(presupuesto))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn("::error::", result.stdout)

    def test_zip_excede_limite(self):
        payload = self._payload("payload_zip", assets_bytes=10, otros_bytes=10)
        zip_path = self.base / "engine-linux.zip"
        write_bytes(zip_path, 5000)
        presupuesto = self._presupuesto("p_zip.json", {
            "payload_total": 1_000_000,
            "assets": 1_000_000,
            "zip_engine": 100,
        })
        result = run(
            "comprobar", "--payload", str(payload),
            "--presupuesto", str(presupuesto),
            "--zips", str(self.base / "*.zip"),
        )
        self.assertEqual(result.returncode, 1)
        self.assertIn("::error::", result.stdout)
        self.assertIn("zip_engine", result.stdout)

    def test_sin_zips_no_comprueba_zip_aunque_exista_enorme(self):
        payload = self._payload("payload_sin_zip", assets_bytes=10, otros_bytes=10)
        zip_path = self.base / "engine-enorme.zip"
        write_bytes(zip_path, 999_999)
        presupuesto = self._presupuesto("p_sinzip.json", {
            "payload_total": 1_000_000,
            "assets": 1_000_000,
            "zip_engine": 1,
        })
        result = run("comprobar", "--payload", str(payload), "--presupuesto", str(presupuesto))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn("zip_engine", result.stdout)

    def test_presupuesto_inexistente(self):
        payload = self._payload("payload_x", assets_bytes=10)
        result = run(
            "comprobar", "--payload", str(payload),
            "--presupuesto", str(self.base / "no_existe.json"),
        )
        self.assertEqual(result.returncode, 2)

    def test_presupuesto_json_mal_formado(self):
        payload = self._payload("payload_y", assets_bytes=10)
        bad = self.base / "mal.json"
        bad.write_text("{ esto no es json valido")
        result = run("comprobar", "--payload", str(payload), "--presupuesto", str(bad))
        self.assertEqual(result.returncode, 2)

    def test_presupuesto_sin_limites_bytes(self):
        payload = self._payload("payload_z", assets_bytes=10)
        incompleto = self.base / "incompleto.json"
        incompleto.write_text(json.dumps({"version": 1, "notas": "", "politica": ""}))
        result = run("comprobar", "--payload", str(payload), "--presupuesto", str(incompleto))
        self.assertEqual(result.returncode, 2)

    def test_payload_inexistente(self):
        presupuesto = self._presupuesto("p_payload_inexistente.json", {
            "payload_total": 1_000_000,
            "assets": 1_000_000,
            "zip_engine": 1_000_000,
        })
        result = run(
            "comprobar",
            "--payload", str(self.base / "no_existe_payload"),
            "--presupuesto", str(presupuesto),
        )
        self.assertEqual(result.returncode, 2)


class TestAutoprueba(unittest.TestCase):
    def test_autoprueba_pasa(self):
        result = run("--autoprueba")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("AUTOPRUEBA", result.stdout)


class TestCableadoWorkflow(unittest.TestCase):
    def setUp(self):
        self.assertTrue(
            WORKFLOW.exists(),
            f"No se encuentra {WORKFLOW}",
        )
        self.contenido = WORKFLOW.read_text(encoding="utf-8")

    def test_copiar_invocado_en_windows_y_linux(self):
        self.assertIn("empaquetar_assets.py copiar", self.contenido)
        ocurrencias = self.contenido.count("empaquetar_assets.py copiar")
        self.assertGreaterEqual(
            ocurrencias, 2,
            "Se esperaba al menos una invocación de 'copiar' por cada job (Windows y Linux)",
        )

    def test_comprobar_invocado_condicionado_a_family_engine(self):
        self.assertIn("empaquetar_assets.py comprobar", self.contenido)

        lineas = self.contenido.splitlines()
        indices_comprobar = [
            i for i, linea in enumerate(lineas)
            if "empaquetar_assets.py comprobar" in linea
        ]
        self.assertTrue(indices_comprobar, "No se encontró invocación de 'comprobar'")

        encontrado_condicionado = False
        for idx in indices_comprobar:
            ventana = "\n".join(lineas[max(0, idx - 15):idx + 1])
            if "FAMILY" in ventana and "engine" in ventana:
                encontrado_condicionado = True
                break

        self.assertTrue(
            encontrado_condicionado,
            "No se encontró un paso 'comprobar' condicionado a FAMILY == 'engine'",
        )


if __name__ == "__main__":
    unittest.main()
