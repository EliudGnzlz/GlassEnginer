#!/usr/bin/env python3
"""
skill_vulkan_grep: Semantic search in local Vulkan headers (/usr/include/vulkan/).
Prevents AI models from hallucinating Vulkan struct members, function signatures, or enums.
"""

import sys
import subprocess
import os

VULKAN_HEADERS_DIR = "/usr/include/vulkan"

def vulkan_grep(query, max_results=15):
    if not os.path.isdir(VULKAN_HEADERS_DIR):
        print(f"Error: No se encontró el directorio de cabeceras en {VULKAN_HEADERS_DIR}")
        sys.exit(1)

    cmd = ["rg", "-n", "-C", "3", query, VULKAN_HEADERS_DIR]
    try:
        proc = subprocess.run(cmd, capture_output=True, text=True)
        lines = proc.stdout.splitlines()
        print("\n".join(lines[:max_results * 8]))
    except FileNotFoundError:
        # Fallback to grep
        proc = subprocess.run(["grep", "-rn", "-C", "3", query, VULKAN_HEADERS_DIR], capture_output=True, text=True)
        lines = proc.stdout.splitlines()
        print("\n".join(lines[:max_results * 8]))

if __name__ == "__main__":
    if len(sys.argv) < 2:
        print("Uso: vulkan_grep.py <término_o_struct_vk>")
        sys.exit(1)
    vulkan_grep(sys.argv[1])
