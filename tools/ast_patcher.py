#!/usr/bin/env python3
"""
skill_ast_patcher: Surgical code replacement for Rust and C# files.
Avoids full-file rewrites and context bloat by patching specific function blocks.
"""

import sys
import re

def patch_function(file_path, symbol_pattern, new_content):
    with open(file_path, "r", encoding="utf-8") as f:
        content = f.read()

    # Pattern matches standard function or block declaration in Rust or C#
    regex = re.compile(symbol_pattern, re.MULTILINE | re.DOTALL)
    if not regex.search(content):
        return False, f"Patrón '{symbol_pattern}' no encontrado en {file_path}"

    updated, count = regex.subn(new_content, content, count=1)
    if count == 0:
        return False, "No se realizaron sustituciones."

    with open(file_path, "w", encoding="utf-8") as f:
        f.write(updated)
    return True, f"Parche aplicado con éxito en {file_path} ({count} bloque reemplazado)."

if __name__ == "__main__":
    if len(sys.argv) < 4:
        print("Uso: ast_patcher.py <archivo> <patron_bloque> <nuevo_contenido>")
        sys.exit(1)
    ok, msg = patch_function(sys.argv[1], sys.argv[2], sys.argv[3])
    print(msg)
    sys.exit(0 if ok else 1)
