#!/usr/bin/env python3
import argparse
import contextlib
import glob
import io
import json
import os
import shutil
import sys
import tempfile


class ErrorUso(Exception):
    pass


EXT_IMAGEN = {".png", ".jpg", ".jpeg"}


def _tam_dir(ruta):
    if not os.path.exists(ruta):
        return 0
    if os.path.isfile(ruta):
        return os.path.getsize(ruta)
    total = 0
    for base, dirs, files in os.walk(ruta):
        dirs.sort()
        files.sort()
        for nombre in files:
            try:
                total += os.path.getsize(os.path.join(base, nombre))
            except OSError:
                pass
    return total


def _es_imagen(nombre):
    return os.path.splitext(nombre)[1].lower() in EXT_IMAGEN


def _tiene_gemelo(directorio, nombre):
    base, _ = os.path.splitext(nombre)
    return os.path.isfile(os.path.join(directorio, base + ".ktx2"))


def copiar_arbol(origen, destino):
    if not os.path.exists(origen):
        raise ErrorUso(f"origen no existe: {origen}")
    if not os.path.isdir(origen):
        raise ErrorUso(f"origen no es un directorio: {origen}")

    resultado = {"copiados": 0, "bytes_copiados": 0, "excluidos": []}
    os.makedirs(destino, exist_ok=True)

    def recorrer(src, dst, rel):
        for nombre in sorted(os.listdir(src)):
            sp = os.path.join(src, nombre)
            rp = f"{rel}/{nombre}" if rel else nombre
            if os.path.isdir(sp):
                if nombre == "polygon-prototype":
                    resultado["excluidos"].append({
                        "ruta": rp,
                        "bytes": _tam_dir(sp),
                        "motivo": "polygon-prototype",
                    })
                    continue
                dp = os.path.join(dst, nombre)
                os.makedirs(dp, exist_ok=True)
                recorrer(sp, dp, rp)
                continue
            if not os.path.isfile(sp):
                continue
            if _es_imagen(nombre) and _tiene_gemelo(src, nombre):
                resultado["excluidos"].append({
                    "ruta": rp,
                    "bytes": os.path.getsize(sp),
                    "motivo": "gemelo-ktx2",
                })
                continue
            shutil.copy2(sp, os.path.join(dst, nombre))
            resultado["copiados"] += 1
            resultado["bytes_copiados"] += os.path.getsize(sp)

    recorrer(origen, destino, "")
    return resultado


def _leer_limites(ruta):
    if not os.path.isfile(ruta):
        raise ErrorUso(f"presupuesto no existe: {ruta}")
    try:
        with open(ruta, "r", encoding="utf-8") as f:
            datos = json.load(f)
    except json.JSONDecodeError as e:
        raise ErrorUso(f"presupuesto JSON mal formado: {e}") from None
    except OSError as e:
        raise ErrorUso(f"no se pudo leer presupuesto: {e}") from None
    if not isinstance(datos, dict) or "limites_bytes" not in datos:
        raise ErrorUso("presupuesto sin clave limites_bytes")
    limites = datos["limites_bytes"]
    if not isinstance(limites, dict):
        raise ErrorUso("limites_bytes debe ser un objeto")
    return limites


def comprobar(payload, presupuesto, zips_globs):
    if not os.path.isdir(payload):
        raise ErrorUso(f"payload no existe o no es un directorio: {payload}")
    limites = _leer_limites(presupuesto)

    def limite(nombre):
        valor = limites.get(nombre)
        if not isinstance(valor, int) or isinstance(valor, bool):
            raise ErrorUso(f"limite '{nombre}' ausente o no entero")
        return valor

    checks = [
        ("payload_total", _tam_dir(payload), limite("payload_total")),
        ("assets", _tam_dir(os.path.join(payload, "assets")), limite("assets")),
    ]

    if zips_globs:
        tamanos = []
        for patron in zips_globs:
            for ruta in sorted(glob.glob(patron)):
                if os.path.isfile(ruta):
                    tamanos.append(os.path.getsize(ruta))
        checks.append(("zip_engine", max(tamanos) if tamanos else 0, limite("zip_engine")))

    ok = True
    for nombre, valor, tope in checks:
        if valor <= tope:
            print(f"PASS {nombre}: {valor} <= {tope}")
        else:
            print(f"::error::{nombre} excedido: {valor} > {tope}")
            ok = False
    return 0 if ok else 1


def _capturar(fn, *args):
    buf = io.StringIO()
    with contextlib.redirect_stdout(buf):
        codigo = fn(*args)
    return codigo, buf.getvalue()


def _ejecutar_autopruebas():
    pasadas = 0
    fallidas = 0

    def check(ok):
        nonlocal pasadas, fallidas
        if ok:
            pasadas += 1
        else:
            fallidas += 1

    with tempfile.TemporaryDirectory() as tmp:
        def dir_tmp(nombre):
            ruta = os.path.join(tmp, nombre)
            os.makedirs(ruta, exist_ok=True)
            return ruta

        def escribir(ruta, datos):
            os.makedirs(os.path.dirname(ruta), exist_ok=True)
            with open(ruta, "wb") as f:
                f.write(datos)

        src = dir_tmp("src1")
        dst = os.path.join(tmp, "dst1")
        escribir(os.path.join(src, "Textura_01.png"), b"png")
        escribir(os.path.join(src, "Textura_01.ktx2"), b"ktx2")
        r = copiar_arbol(src, dst)
        check(not os.path.exists(os.path.join(dst, "Textura_01.png")))
        check(os.path.exists(os.path.join(dst, "Textura_01.ktx2")))
        check(any(e["ruta"] == "Textura_01.png" and e["motivo"] == "gemelo-ktx2" for e in r["excluidos"]))

        src = dir_tmp("src2")
        dst = os.path.join(tmp, "dst2")
        escribir(os.path.join(src, "Textura_02.jpg"), b"jpg")
        r = copiar_arbol(src, dst)
        check(os.path.exists(os.path.join(dst, "Textura_02.jpg")))
        check(r["copiados"] == 1)

        src = dir_tmp("src3")
        dst = os.path.join(tmp, "dst3")
        escribir(os.path.join(src, "a", "polygon-prototype", "x.txt"), b"x")
        escribir(os.path.join(src, "polygon-prototype", "y.txt"), b"y")
        escribir(os.path.join(src, "keep.txt"), b"k")
        r = copiar_arbol(src, dst)
        check(not os.path.exists(os.path.join(dst, "a", "polygon-prototype")))
        check(not os.path.exists(os.path.join(dst, "polygon-prototype")))
        check(os.path.exists(os.path.join(dst, "keep.txt")))
        check(sum(1 for e in r["excluidos"] if e["motivo"] == "polygon-prototype") == 2)

        payload = dir_tmp("payload")
        escribir(os.path.join(payload, "assets", "a.bin"), b"12345")
        presupuesto_ok = os.path.join(tmp, "presupuesto_ok.json")
        with open(presupuesto_ok, "w", encoding="utf-8") as f:
            json.dump({"version": 1, "limites_bytes": {"payload_total": 1000, "assets": 1000, "zip_engine": 1000}}, f)
        codigo, salida = _capturar(comprobar, payload, presupuesto_ok, [])
        check(codigo == 0)
        check("PASS payload_total" in salida)

        presupuesto_mal = os.path.join(tmp, "presupuesto_mal.json")
        with open(presupuesto_mal, "w", encoding="utf-8") as f:
            json.dump({"version": 1, "limites_bytes": {"payload_total": 1, "assets": 1000, "zip_engine": 1000}}, f)
        codigo, salida = _capturar(comprobar, payload, presupuesto_mal, [])
        check(codigo == 1)
        check("::error::payload_total" in salida)

        presupuesto_json_mal = os.path.join(tmp, "presupuesto_json_mal.json")
        with open(presupuesto_json_mal, "w", encoding="utf-8") as f:
            f.write("{")
        try:
            _capturar(comprobar, payload, presupuesto_json_mal, [])
            check(False)
        except ErrorUso:
            check(True)

    print(f"AUTOPRUEBA: {pasadas} pasadas, {fallidas} fallidas")
    return 0 if fallidas == 0 else 1


def _parser():
    p = argparse.ArgumentParser(prog="empaquetar_assets.py")
    p.add_argument("--autoprueba", action="store_true")
    sub = p.add_subparsers(dest="cmd")
    c = sub.add_parser("copiar")
    c.add_argument("origen")
    c.add_argument("destino")
    co = sub.add_parser("comprobar")
    co.add_argument("--payload", required=True)
    co.add_argument("--presupuesto", required=True)
    co.add_argument("--zips", action="append", nargs="+", default=[])
    return p


def main(argv=None):
    parser = _parser()
    args = parser.parse_args(argv)
    try:
        if args.autoprueba:
            return _ejecutar_autopruebas()
        if args.cmd == "copiar":
            resultado = copiar_arbol(args.origen, args.destino)
            print(json.dumps(resultado, ensure_ascii=False, separators=(",", ":")))
            return 0
        if args.cmd == "comprobar":
            globs = [g for grupo in args.zips for g in grupo]
            return comprobar(args.payload, args.presupuesto, globs)
        parser.print_help(sys.stderr)
        return 2
    except ErrorUso as e:
        print(f"error: {e}", file=sys.stderr)
        return 2
    except OSError as e:
        print(f"error: {e}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
