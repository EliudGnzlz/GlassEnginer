#!/usr/bin/env python3
"""Tabla subdividida para la demo de sombras de grafo (Fase 2S, S3).

Losa de 5 x 0.15 x 3 m con la cara superior en rejilla 20x12 para que el
desplazamiento de vértices de un grafo (olas) se vea en la malla y en su
sombra. Incluye UV (la plantilla de vértices del grafo lo exige).
"""
import json
import os
import struct
import sys

SX, SZ = 20, 12  # segmentos X, segmentos Z
GROSOR = 0.15
W, D = 5.0, 3.0


def grid(y, normal, flip):
    pos, nor, uv, idx = [], [], [], []
    for j in range(SZ + 1):
        for i in range(SX + 1):
            x = -W / 2 + W * i / SX
            z = -D / 2 + D * j / SZ
            pos.append((x, y, z))
            nor.append(normal)
            uv.append((i / SX, j / SZ))
    for j in range(SZ):
        for i in range(SX):
            a = j * (SX + 1) + i
            b = a + 1
            c = a + SX + 1
            d = c + 1
            if flip:
                idx.extend((a, b, c, b, d, c))
            else:
                idx.extend((a, c, b, b, c, d))
    return pos, nor, uv, idx


def skirt(borde, y_top, y_bot, normal):
    """Tira lateral: borde es la lista de (x, z) del perímetro de una cara."""
    pos, nor, uv, idx = [], [], [], []
    for k, (x, z) in enumerate(borde):
        pos.append((x, y_top, z))
        nor.append(normal)
        uv.append((k / max(1, len(borde) - 1), 1.0))
        pos.append((x, y_bot, z))
        nor.append(normal)
        uv.append((k / max(1, len(borde) - 1), 0.0))
    for k in range(len(borde) - 1):
        a = 2 * k
        idx.extend((a, a + 2, a + 1, a + 1, a + 2, a + 3))
    return pos, nor, uv, idx


def main():
    project_dir = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    output = sys.argv[1] if len(sys.argv) > 1 else os.path.join(
        project_dir, "assets", "models", "tabla_olas.glb"
    )

    parts = []
    parts.append(grid(GROSOR, (0.0, 1.0, 0.0), flip=False))   # tapa
    parts.append(grid(0.0, (0.0, -1.0, 0.0), flip=True))      # base
    xs = [-W / 2 + W * i / SX for i in range(SX + 1)]
    zs = [-D / 2 + D * j / SZ for j in range(SZ + 1)]
    parts.append(skirt([(x, -D / 2) for x in xs], GROSOR, 0.0, (0.0, 0.0, -1.0)))
    parts.append(skirt([(x, D / 2) for x in xs], GROSOR, 0.0, (0.0, 0.0, 1.0)))
    parts.append(skirt([(-W / 2, z) for z in zs], GROSOR, 0.0, (-1.0, 0.0, 0.0)))
    parts.append(skirt([(W / 2, z) for z in zs], GROSOR, 0.0, (1.0, 0.0, 0.0)))

    positions, normals, uvs, indices = [], [], [], []
    for pos, nor, uv, idx in parts:
        base = len(positions)
        positions.extend(pos)
        normals.extend(nor)
        uvs.extend(uv)
        indices.extend(i + base for i in idx)

    position_data = b"".join(struct.pack("<3f", *p) for p in positions)
    normal_data = b"".join(struct.pack("<3f", *n) for n in normals)
    uv_data = b"".join(struct.pack("<2f", *t) for t in uvs)
    index_data = struct.pack("<%dI" % len(indices), *indices)

    off_n = len(position_data)
    off_uv = off_n + len(normal_data)
    off_i = off_uv + len(uv_data)
    bin_data = position_data + normal_data + uv_data + index_data

    minimum = [min(p[a] for p in positions) for a in range(3)]
    maximum = [max(p[a] for p in positions) for a in range(3)]
    n = len(positions)

    gltf = {
        "asset": {"version": "2.0"},
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": [{"mesh": 0}],
        "meshes": [{
            "primitives": [{
                "attributes": {"POSITION": 0, "NORMAL": 1, "TEXCOORD_0": 2},
                "indices": 3,
                "material": 0,
            }]
        }],
        "materials": [{
            "pbrMetallicRoughness": {
                "baseColorFactor": [0.8, 0.78, 0.72, 1.0],
                "metallicFactor": 0.0,
                "roughnessFactor": 0.9,
            }
        }],
        "buffers": [{"byteLength": len(bin_data)}],
        "bufferViews": [
            {"buffer": 0, "byteOffset": 0, "byteLength": len(position_data), "target": 34962},
            {"buffer": 0, "byteOffset": off_n, "byteLength": len(normal_data), "target": 34962},
            {"buffer": 0, "byteOffset": off_uv, "byteLength": len(uv_data), "target": 34962},
            {"buffer": 0, "byteOffset": off_i, "byteLength": len(index_data), "target": 34963},
        ],
        "accessors": [
            {"bufferView": 0, "componentType": 5126, "count": n, "type": "VEC3",
             "min": minimum, "max": maximum},
            {"bufferView": 1, "componentType": 5126, "count": n, "type": "VEC3"},
            {"bufferView": 2, "componentType": 5126, "count": n, "type": "VEC2"},
            {"bufferView": 3, "componentType": 5125, "count": len(indices), "type": "SCALAR"},
        ],
    }

    json_data = json.dumps(gltf, separators=(",", ":")).encode("utf-8")
    json_data += b" " * ((-len(json_data)) % 4)
    bin_data += b"\0" * ((-len(bin_data)) % 4)
    total_length = 12 + 8 + len(json_data) + 8 + len(bin_data)
    glb = (
        struct.pack("<III", 0x46546C67, 2, total_length)
        + struct.pack("<II", len(json_data), 0x4E4F534A)
        + json_data
        + struct.pack("<II", len(bin_data), 0x004E4942)
        + bin_data
    )
    os.makedirs(os.path.dirname(os.path.abspath(output)), exist_ok=True)
    with open(output, "wb") as f:
        f.write(glb)
    print(f"{output}: {len(glb)} bytes, {n} vértices, {len(indices)} índices")


if __name__ == "__main__":
    main()
