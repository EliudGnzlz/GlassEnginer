#!/usr/bin/env python3
import json
import os
import struct
import sys


def box_geometry(bounds):
    x0, x1, y0, y1, z0, z1 = bounds
    faces = [
        ([(x1, y0, z0), (x1, y1, z0), (x1, y1, z1), (x1, y0, z1)], (1.0, 0.0, 0.0)),
        ([(x0, y0, z0), (x0, y0, z1), (x0, y1, z1), (x0, y1, z0)], (-1.0, 0.0, 0.0)),
        ([(x0, y1, z0), (x0, y1, z1), (x1, y1, z1), (x1, y1, z0)], (0.0, 1.0, 0.0)),
        ([(x0, y0, z0), (x1, y0, z0), (x1, y0, z1), (x0, y0, z1)], (0.0, -1.0, 0.0)),
        ([(x0, y0, z1), (x1, y0, z1), (x1, y1, z1), (x0, y1, z1)], (0.0, 0.0, 1.0)),
        ([(x0, y0, z0), (x0, y1, z0), (x1, y1, z0), (x1, y0, z0)], (0.0, 0.0, -1.0)),
    ]

    positions = []
    normals = []
    indices = []
    for vertices, normal in faces:
        base = len(positions)
        positions.extend(vertices)
        normals.extend([normal] * 4)
        indices.extend((base, base + 1, base + 2, base, base + 2, base + 3))
    return positions, normals, indices


def main():
    project_dir = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    output = sys.argv[1] if len(sys.argv) > 1 else os.path.join(
        project_dir, "assets", "models", "canal_u.glb"
    )

    boxes = [
        (-1.2, 1.2, -0.2, 0.0, -2.0, 2.0),
        (-1.2, -1.0, 0.0, 1.0, -2.0, 2.0),
        (1.0, 1.2, 0.0, 1.0, -2.0, 2.0),
    ]

    positions = []
    normals = []
    indices = []
    for bounds in boxes:
        box_positions, box_normals, box_indices = box_geometry(bounds)
        offset = len(positions)
        positions.extend(box_positions)
        normals.extend(box_normals)
        indices.extend(index + offset for index in box_indices)

    position_data = b"".join(struct.pack("<3f", *position) for position in positions)
    normal_data = b"".join(struct.pack("<3f", *normal) for normal in normals)
    index_data = struct.pack("<108I", *indices)

    normal_offset = len(position_data)
    index_offset = normal_offset + len(normal_data)
    bin_data = position_data + normal_data + index_data

    minimum = [min(position[axis] for position in positions) for axis in range(3)]
    maximum = [max(position[axis] for position in positions) for axis in range(3)]

    gltf = {
        "asset": {"version": "2.0"},
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": [{"mesh": 0}],
        "meshes": [{
            "primitives": [{
                "attributes": {"POSITION": 0, "NORMAL": 1},
                "indices": 2,
                "material": 0,
            }]
        }],
        "materials": [{
            "pbrMetallicRoughness": {
                "baseColorFactor": [0.75, 0.72, 0.68, 1.0],
                "metallicFactor": 0.0,
                "roughnessFactor": 0.9,
            }
        }],
        "buffers": [{"byteLength": len(bin_data)}],
        "bufferViews": [
            {"buffer": 0, "byteOffset": 0, "byteLength": len(position_data), "target": 34962},
            {"buffer": 0, "byteOffset": normal_offset, "byteLength": len(normal_data), "target": 34962},
            {"buffer": 0, "byteOffset": index_offset, "byteLength": len(index_data), "target": 34963},
        ],
        "accessors": [
            {
                "bufferView": 0,
                "componentType": 5126,
                "count": 72,
                "type": "VEC3",
                "min": minimum,
                "max": maximum,
            },
            {
                "bufferView": 1,
                "componentType": 5126,
                "count": 72,
                "type": "VEC3",
            },
            {
                "bufferView": 2,
                "componentType": 5125,
                "count": 108,
                "type": "SCALAR",
            },
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

    output_dir = os.path.dirname(os.path.abspath(output))
    os.makedirs(output_dir, exist_ok=True)
    with open(output, "wb") as file:
        file.write(glb)

    print(f"{output}: {len(glb)} bytes")


if __name__ == "__main__":
    main()
