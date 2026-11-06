#!/usr/bin/env python3
"""
skill_headless_blender_bridge: Headless execution of Blender export pipeline.
Converts .blend and .fbx to standard .glb and validates binary glTF headers.
"""

import sys
import subprocess
import struct
import os

BLENDER_EXPORT_SCRIPT = """
import bpy
import sys

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
bpy.ops.export_scene.gltf(filepath=output_glb, export_format='GLB')
print(f"Exportado exitosamente: {output_glb}")
"""

def export_blend_to_glb(blend_path, glb_path):
    # Acepta .blend y .fbx: el script de Blender ramifica por extensión.
    if not os.path.isfile(blend_path):
        return False, f"Archivo de modelo no encontrado: {blend_path}"

    script_path = "/tmp/glass_blender_export.py"
    with open(script_path, "w", encoding="utf-8") as f:
        f.write(BLENDER_EXPORT_SCRIPT)

    cmd = ["blender", "-b", "--python", script_path, "--", blend_path, glb_path]
    res = subprocess.run(cmd, capture_output=True, text=True)

    if res.returncode != 0:
        return False, f"Fallo al ejecutar Blender: {res.stderr}\n{res.stdout}"

    # Verify GLB header (magic 0x46546C67 = 'glTF', version 2)
    if not os.path.isfile(glb_path):
        return False, "El archivo .glb no fue generado."

    with open(glb_path, "rb") as f:
        header = f.read(12)
        if len(header) < 12:
            return False, "Encabezado GLB corrupto o demasiado corto."
        magic, version, length = struct.unpack("<4sII", header)
        if magic != b"glTF" or version != 2:
            return False, f"Encabezado GLB inválido (magic={magic}, version={version})"

    return True, f"Conversión exitosa y verificada: {glb_path} ({length} bytes)"

if __name__ == "__main__":
    if len(sys.argv) < 3:
        print("Uso: headless_blender_bridge.py <input.blend|input.fbx> <output.glb>")
        sys.exit(1)
    ok, msg = export_blend_to_glb(sys.argv[1], sys.argv[2])
    print(msg)
    sys.exit(0 if ok else 1)
