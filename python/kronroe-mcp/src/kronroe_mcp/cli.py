from __future__ import annotations

import os
import shutil
import subprocess
import sys


def _resolve_binary() -> str:
    explicit = os.environ.get("KRONROE_MCP_BIN")
    if explicit:
        return explicit

    found = shutil.which("kronroe-mcp")
    if found:
        return found

    raise FileNotFoundError(
        "Could not find 'kronroe-mcp' binary on PATH. "
        "Set KRONROE_MCP_BIN or install it (for example: "
        "`cargo install --path crates/mcp-server`)."
    )


def main() -> int:
    try:
        binary = _resolve_binary()
    except FileNotFoundError as exc:
        print(str(exc), file=sys.stderr)
        return 1

    proc = subprocess.run([binary, *sys.argv[1:]], check=False)
    return proc.returncode


if __name__ == "__main__":
    raise SystemExit(main())
