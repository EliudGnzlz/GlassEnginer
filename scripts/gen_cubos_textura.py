#!/usr/bin/env python3
"""Genera dos cubos GLB con un PNG de damero incrustado."""

import json
import struct
import zlib
from pathlib import Path


def chunk_png(tipo, datos):
    """Construye un chunk PNG con su CRC."""
    return (
        struct.pack(">I", len(datos))
        + tipo
        + datos
        + struct.pack(">I", zlib.crc32(tipo + datos) & 0xFFFFFFFF)
    )


def crear_png(tipo_color):
    """Crea el damero RGB o RGBA usando únicamente la stdlib."""
    canales = 3 if tipo_color == 2 else 4
    filas = bytearray()

    for y in range(256):
        filas.append(0)  # Filtro PNG None.
        for x in range(256):
            color = ((x // 32) + (y // 32)) % 2
            filas.extend((255, 255, 255) if color else (255, 0, 255))
            if canales == 4:
                alfa = ((x // 64) + (y // 64)) % 2
                filas.append(110 if alfa else 255)

    ihdr = struct.pack(">IIBBBBB", 256, 256, 8, tipo_color, 0, 0, 0)
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk_png(b"IHDR", ihdr)
        + chunk_png(b"IDAT", zlib.compress(bytes(filas), 9))
        + chunk_png(b"IEND", b"")
    )


def crear_cubo():
    """Devuelve posiciones, normales, UVs e índices del cubo 2x2x2."""
    caras = [
        # Frente, atrás, derecha, izquierda, arriba y abajo.
        ([(-1, -1, 1), (1, -1, 1), (1, 1, 1), (-1, 1, 1)], (0, 0, 1)),
        ([(1, -1, -1), (-1, -1, -1), (-1, 1, -1), (1, 1, -1)], (0, 0, -1)),
        ([(1, -1, 1), (1, -1, -1), (1, 1, -1), (1, 1, 1)], (1, 0, 0)),
        ([(-1, -1, -1), (-1, -1, 1), (-1, 1, 1), (-1, 1, -1)], (-1, 0, 0)),
        ([(-1, 1, 1), (1, 1, 1), (1, 1, -1), (-1, 1, -1)], (0, 1, 0)),
        ([(-1, -1, -1), (1, -1, -1), (1, -1, 1), (-1, -1, 1)], (0, -1, 0)),
    ]

    posiciones = []
    normales = []
    uvs = []
    indices = []
    uv_cara = [(0, 0), (1, 0), (1, 1), (0, 1)]

    for vertices, normal in caras:
        base = len(posiciones)
        posiciones.extend(vertices)
        normales.extend([normal] * 4)
        uvs.extend(uv_cara)
        indices.extend((base, base + 1, base + 2, base, base + 2, base + 3))

    return posiciones, normales, uvs, indices


def crear_glb(png):
    """Construye el GLB y calcula manualmente bufferViews y accessors."""
    posiciones, normales, uvs, indices = crear_cubo()
    binario = bytearray()
    vistas = []

    def agregar_vista(datos, target=None):
        # Cada bloque comienza en un offset alineado a cuatro bytes.
        while len(binario) % 4:
            binario.append(0)
        vista = {
            "buffer": 0,
            "byteOffset": len(binario),
            "byteLength": len(datos),
        }
        if target is not None:
            vista["target"] = target
        binario.extend(datos)
        vistas.append(vista)
        return len(vistas) - 1

    vista_pos = agregar_vista(
        b"".join(struct.pack("<3f", *v) for v in posiciones), 34962
    )
    vista_norm = agregar_vista(
        b"".join(struct.pack("<3f", *v) for v in normales), 34962
    )
    vista_uv = agregar_vista(
        b"".join(struct.pack("<2f", *v) for v in uvs), 34962
    )
    vista_indices = agregar_vista(
        struct.pack(f"<{len(indices)}I", *indices), 34963
    )
    vista_png = agregar_vista(png)

    documento = {
        "asset": {"version": "2.0", "generator": "GlassEnginer gen_cubos_textura"},
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": [{"name": "CuboTextura", "mesh": 0}],
        "meshes": [{
            "primitives": [{
                "attributes": {
                    "POSITION": 0,
                    "NORMAL": 1,
                    "TEXCOORD_0": 2,
                },
                "indices": 3,
                "material": 0,
            }]
        }],
        "materials": [{
            "pbrMetallicRoughness": {
                "baseColorTexture": {"index": 0},
                "metallicFactor": 0,
                "roughnessFactor": 0.9,
            }
        }],
        "samplers": [{
            "magFilter": 9729,
            "minFilter": 9987,
            "wrapS": 10497,
            "wrapT": 10497,
        }],
        "textures": [{"sampler": 0, "source": 0}],
        "images": [{"bufferView": vista_png, "mimeType": "image/png"}],
        "buffers": [{"byteLength": len(binario)}],
        "bufferViews": vistas,
        "accessors": [
            {
                "bufferView": vista_pos,
                "componentType": 5126,
                "count": 24,
                "type": "VEC3",
                "min": [-1, -1, -1],
                "max": [1, 1, 1],
            },
            {
                "bufferView": vista_norm,
                "componentType": 5126,
                "count": 24,
                "type": "VEC3",
            },
            {
                "bufferView": vista_uv,
                "componentType": 5126,
                "count": 24,
                "type": "VEC2",
            },
            {
                "bufferView": vista_indices,
                "componentType": 5125,
                "count": len(indices),
                "type": "SCALAR",
            },
        ],
    }

    json_bytes = json.dumps(documento, separators=(",", ":")).encode("utf-8")
    json_bytes += b" " * ((-len(json_bytes)) % 4)
    bin_bytes = bytes(binario) + b"\x00" * ((-len(binario)) % 4)

    chunks = (
        struct.pack("<II", len(json_bytes), 0x4E4F534A) + json_bytes
        + struct.pack("<II", len(bin_bytes), 0x004E4942) + bin_bytes
    )
    return struct.pack("<4sII", b"glTF", 2, 12 + len(chunks)) + chunks


def main():
    destino = Path(__file__).resolve().parents[1] / "assets" / "models"
    destino.mkdir(parents=True, exist_ok=True)

    archivos = [
        ("cubo_checker_opaco.glb", crear_png(2)),
        ("cubo_checker_alfa.glb", crear_png(6)),
    ]
    for nombre, png in archivos:
        ruta = destino / nombre
        ruta.write_bytes(crear_glb(png))
        print(f"{ruta} ({ruta.stat().st_size} bytes)")


if __name__ == "__main__":
    main()
