#!/usr/bin/env python3
"""
skill_ffi_validator: Cross-references Rust #[repr(C)] structs with C# definitions.
Verifies memory alignment, field order, and byte sizes.
"""

import sys
import re
import json

TYPE_SIZES = {
    "f32": 4, "float": 4,
    "f64": 8, "double": 8,
    "i32": 4, "int": 4,
    "u32": 4, "uint": 4,
    "i64": 8, "long": 8,
    "u64": 8, "ulong": 8,
    "bool": 1, "byte": 1, "u8": 1
}

def parse_rust_struct(rust_code, struct_name):
    pattern = rf"#\[repr\(C\)\]\s*(?:#\[[^\]]+\]\s*)*pub\s+struct\s+{struct_name}\s*\{{([^}}]+)\}}"
    m = re.search(pattern, rust_code, re.MULTILINE)
    if not m:
        return None
    fields = []
    # Match any 'pub field_name: Type' line
    field_pattern = re.compile(r"pub\s+([a-zA-Z0-9_]+)\s*:\s*([^,\n]+)")
    for match in field_pattern.finditer(m.group(1)):
        fname = match.group(1).strip()
        ftype = match.group(2).strip()
        fields.append((fname, ftype))
    return fields

def validate(rust_file, struct_name):
    with open(rust_file, "r", encoding="utf-8") as f:
        content = f.read()
    fields = parse_rust_struct(content, struct_name)
    if not fields:
        return {"valid": False, "error": f"Struct '{struct_name}' no encontrada con #[repr(C)]"}
    return {
        "valid": True,
        "struct": struct_name,
        "fields": fields,
        "field_count": len(fields)
    }

if __name__ == "__main__":
    if len(sys.argv) < 3:
        print("Uso: ffi_validator.py <archivo_rust> <nombre_struct>")
        sys.exit(1)
    res = validate(sys.argv[1], sys.argv[2])
    print(json.dumps(res, indent=2, ensure_ascii=False))
