#!/usr/bin/env python3
"""
skill_headless_blender_bridge: Headless execution of Blender export pipeline.
Converts .blend and .fbx to standard .glb and validates binary glTF headers.

Fase 6: además de exportar, el bridge inventaría la escena (objetos,
instancias enlazadas por malla compartida, niveles LOD por sufijo
`_LOD<n>`, colecciones y prefabs `Prefab_<X>`), valida el GLB
(cabecera + chunk JSON + conteos) y escribe un sidecar
`<modelo>.glb.meta.json` con el SHA-256 de la fuente, para que el
motor sepa QUÉ se exportó, CUÁNDO y DESDE QUÉ contenido.
La última línea de salida es siempre `GLASS_BRIDGE_RESULT {json}`.
"""

import hashlib
import json
import os
import shutil
import struct
import subprocess
import sys
import tempfile
from datetime import datetime, timezone

BLENDER_EXPORT_SCRIPT = r"""
import bpy
import sys
import json

argv = sys.argv
try:
    idx = argv.index("--") + 1
    input_file = argv[idx]
    output_glb = argv[idx + 1]
except (ValueError, IndexError):
    print("Error: Argumentos de exportación incorrectos")
    sys.exit(1)

import os
ext = os.path.splitext(input_file)[1].lower()
if ext == ".fbx":
    # Escena vacía e importar el FBX con el importador de Blender.
    bpy.ops.wm.read_factory_settings(use_empty=True)
    try:
        bpy.ops.import_scene.fbx(filepath=input_file)
    except (AttributeError, RuntimeError):
        # Blender >= 5: el operador pasó a wm.fbx_import.
        bpy.ops.wm.fbx_import(filepath=input_file)
else:
    bpy.ops.wm.open_mainfile(filepath=input_file)

# La exportación va ANTES del inventario: si lanza, no hay inventario y
# (con --python-exit-code) Blender sale no-cero.
bpy.ops.export_scene.gltf(filepath=output_glb, export_format='GLB')
print(f"Exportado exitosamente: {output_glb}")

# --- Inventario de la escena (Fase 6) ---
def lod_de(nombre):
    import re
    m = re.search(r"_LOD(\d+)$", nombre)
    return int(m.group(1)) if m else None

usuarios_por_malla = {}
for ob in bpy.data.objects:
    if ob.type == "MESH" and ob.data is not None:
        usuarios_por_malla.setdefault(ob.data.name, []).append(ob.name)

objetos = []
for ob in bpy.data.objects:
    es_malla = ob.type == "MESH" and ob.data is not None
    objetos.append({
        "name": ob.name,
        "type": ob.type,
        "mesh": ob.data.name if es_malla else None,
        "collections": sorted(c.name for c in ob.users_collection),
        "parent": ob.parent.name if ob.parent else None,
        "lod": lod_de(ob.name),
        "instance_of": (ob.data.name if es_malla and len(usuarios_por_malla.get(ob.data.name, [])) > 1 else None),
        "location": [round(float(v), 4) for v in ob.location],
    })

colecciones = []
for col in bpy.data.collections:
    colecciones.append({
        "name": col.name,
        "prefab": col.name[len("Prefab_"):] if col.name.startswith("Prefab_") else None,
        "objects": sorted(o.name for o in col.objects),
    })

inventario = {
    "blender_version": bpy.app.version_string,
    "objects": objetos,
    "collections": colecciones,
}
print("GLASS_INVENTORY_BEGIN")
print(json.dumps(inventario))
print("GLASS_INVENTORY_END")
"""


def resolver_blender():
    env = os.environ.get("GLASS_BLENDER")
    if env and os.path.isfile(env):
        return env
    return shutil.which("blender")


def sha256_de(ruta):
    h = hashlib.sha256()
    with open(ruta, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def validar_glb(glb_path):
    """(ok, mensaje, conteos): cabecera, chunk JSON y conteos básicos."""
    if not os.path.isfile(glb_path):
        return False, "El archivo .glb no fue generado.", {}
    with open(glb_path, "rb") as f:
        data = f.read()
    if len(data) < 20:
        return False, "Encabezado GLB corrupto o demasiado corto.", {}
    magic, version, length = struct.unpack("<4sII", data[:12])
    if magic != b"glTF" or version != 2:
        return False, f"Encabezado GLB inválido (magic={magic}, version={version})", {}
    if length != len(data):
        return False, (
            f"Longitud GLB inconsistente: cabecera={length} bytes, archivo={len(data)} bytes"
        ), {}
    chunk_len, chunk_type = struct.unpack("<I4s", data[12:20])
    if chunk_type != b"JSON":
        return False, "El primer chunk del GLB no es JSON.", {}
    try:
        doc = json.loads(data[20:20 + chunk_len])
    except (ValueError, UnicodeDecodeError) as e:
        return False, f"Chunk JSON del GLB ilegible: {e}", {}
    conteos = {
        "meshes": len(doc.get("meshes", [])),
        "nodes": len(doc.get("nodes", [])),
        "materials": len(doc.get("materials", [])),
        "animations": len(doc.get("animations", [])),
        "skins": len(doc.get("skins", [])),
        "bytes": length,
    }
    if conteos["meshes"] == 0:
        return False, "El GLB no contiene ninguna malla.", conteos
    return True, f"GLB válido: {length} bytes", conteos


def export_blend_to_glb(blend_path, glb_path):
    if not os.path.isfile(blend_path):
        return False, f"Archivo de modelo no encontrado: {blend_path}", {}

    blender = resolver_blender()
    if not blender:
        return False, (
            "No se encontró el ejecutable de Blender: define GLASS_BLENDER "
            "con su ruta o añade 'blender' al PATH."
        ), {}

    # Hash del contenido que se va a exportar, ANTES de lanzar Blender.
    try:
        source_sha256 = sha256_de(blend_path)
    except OSError as e:
        return False, f"No se pudo leer la fuente {blend_path}: {e}", {}

    # Exportación a una ruta temporal hermana; el destino solo se toca
    # tras validar el GLB temporal.
    tmp_glb = glb_path + ".glass-tmp"

    def limpiar_tmp():
        for p in (tmp_glb, tmp_glb + ".glb"):
            try:
                os.unlink(p)
            except OSError:
                pass

    limpiar_tmp()

    with tempfile.NamedTemporaryFile(
        "w", suffix=".py", prefix="glass_blender_export_", delete=False, encoding="utf-8"
    ) as f:
        f.write(BLENDER_EXPORT_SCRIPT)
        script_path = f.name

    try:
        cmd = [
            blender, "-b", "--python-exit-code", "1", "--python", script_path,
            "--", blend_path, tmp_glb,
        ]
        res = subprocess.run(cmd, capture_output=True, text=True)
    except OSError as e:
        limpiar_tmp()
        return False, f"Fallo al lanzar Blender: {e}", {}
    finally:
        try:
            os.unlink(script_path)
        except OSError:
            pass

    if res.returncode != 0:
        limpiar_tmp()
        return False, f"Fallo al ejecutar Blender: {res.stderr}\n{res.stdout}", {}

    # Por si alguna versión del exportador añade la extensión ".glb".
    if not os.path.isfile(tmp_glb) and os.path.isfile(tmp_glb + ".glb"):
        os.replace(tmp_glb + ".glb", tmp_glb)

    inventario = {}
    if "GLASS_INVENTORY_BEGIN" in res.stdout:
        trozo = res.stdout.split("GLASS_INVENTORY_BEGIN", 1)[1].split("GLASS_INVENTORY_END", 1)[0]
        try:
            inventario = json.loads(trozo.strip())
        except ValueError:
            inventario = {}
    if not inventario:
        # Sin inventario no hay sidecar fiable: mejor un error claro
        # que pisar un sidecar bueno con uno vacío (pasó una vez con
        # la salida de Blender truncada).
        limpiar_tmp()
        return False, (
            "Blender exportó pero no devolvió el inventario de la escena; "
            "no se escribe el sidecar. Salida: " + res.stdout[-400:]
        ), {}

    ok, msg, conteos = validar_glb(tmp_glb)
    if not ok:
        limpiar_tmp()
        return False, msg, {}

    # Re-hash tras exportar: si la fuente cambió, el sidecar conserva el
    # hash previo (el del contenido exportado) y la validación dirá fresh=false.
    try:
        hash_despues = sha256_de(blend_path)
    except OSError:
        hash_despues = None
    fuente_cambio = hash_despues != source_sha256

    sidecar = {
        "source": os.path.basename(blend_path),
        "source_sha256": source_sha256,
        "exported_at": datetime.now(timezone.utc).isoformat(),
        "blender_version": inventario.get("blender_version", ""),
        "counts": conteos,
        "objects": inventario.get("objects", []),
        "collections": inventario.get("collections", []),
    }
    sidecar_path = glb_path + ".meta.json"
    sidecar_tmp = sidecar_path + ".glass-tmp"
    try:
        with open(sidecar_tmp, "w", encoding="utf-8") as f:
            json.dump(sidecar, f, indent=2)
        os.replace(tmp_glb, glb_path)
        os.replace(sidecar_tmp, sidecar_path)
    except OSError as e:
        limpiar_tmp()
        try:
            os.unlink(sidecar_tmp)
        except OSError:
            pass
        return False, f"No se pudo instalar el GLB/sidecar en el destino: {e}", {}

    resultado = {
        "ok": True,
        "glb": glb_path,
        "sidecar": sidecar_path,
        "source_sha256": sidecar["source_sha256"],
        "counts": conteos,
        "objects": len(sidecar["objects"]),
        "instances": sum(1 for o in sidecar["objects"] if o.get("instance_of")),
        "lods": sorted({o["lod"] for o in sidecar["objects"] if o.get("lod") is not None}),
        "prefabs": [c["prefab"] for c in sidecar["collections"] if c.get("prefab")],
    }
    if fuente_cambio:
        resultado["source_changed_during_export"] = True
    return True, f"Conversión exitosa y verificada: {glb_path} ({conteos['bytes']} bytes)", resultado


if __name__ == "__main__":
    if len(sys.argv) < 3:
        print("Uso: headless_blender_bridge.py <input.blend|input.fbx> <output.glb>")
        print('GLASS_BRIDGE_RESULT {"ok": false, "error": "faltan argumentos"}')
        sys.exit(1)
    ok, msg, resultado = export_blend_to_glb(sys.argv[1], sys.argv[2])
    print(msg)
    if not ok and not resultado:
        resultado = {"ok": False, "error": msg}
    print("GLASS_BRIDGE_RESULT " + json.dumps(resultado))
    sys.exit(0 if ok else 1)
