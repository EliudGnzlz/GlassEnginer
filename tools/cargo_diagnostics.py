#!/usr/bin/env python3
"""
skill_cargo_diagnostics: Parses cargo check --message-format=json.
Extracts only exact errors and relevant snippets to avoid context bloat.
"""

import subprocess
import json
import sys

def run_diagnostics(workspace_dir):
    proc = subprocess.Popen(
        ["cargo", "check", "--message-format=json"],
        cwd=workspace_dir,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True
    )

    diagnostics = []
    for line in proc.stdout:
        line = line.strip()
        if not line:
            continue
        try:
            msg = json.loads(line)
            if msg.get("reason") == "compiler-message":
                compiler_msg = msg.get("message", {})
                level = compiler_msg.get("level")
                if level in ("error", "warning"):
                    spans = compiler_msg.get("spans", [])
                    primary_span = next((s for s in spans if s.get("is_primary")), spans[0] if spans else None)
                    diagnostics.append({
                        "level": level,
                        "code": compiler_msg.get("code", {}).get("code") if compiler_msg.get("code") else None,
                        "message": compiler_msg.get("message"),
                        "file": primary_span.get("file_name") if primary_span else None,
                        "line": primary_span.get("line_start") if primary_span else None,
                        "col": primary_span.get("column_start") if primary_span else None,
                        "snippet": primary_span.get("text", [{}])[0].get("text") if primary_span and primary_span.get("text") else None
                    })
        except Exception:
            continue

    proc.wait()
    return diagnostics

if __name__ == "__main__":
    target_dir = sys.argv[1] if len(sys.argv) > 1 else "."
    res = run_diagnostics(target_dir)
    print(json.dumps(res, indent=2, ensure_ascii=False))
