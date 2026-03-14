from __future__ import annotations

import os
import subprocess
import sys


def _is_native_binary(path: str) -> bool:
    """Return True if *path* looks like a native executable, not a script wrapper."""
    try:
        with open(path, "rb") as fh:
            head = fh.read(4)
    except OSError:
        return False

    # ELF magic (\x7fELF), Mach-O thin (0xfeedface / 0xfeedfacf),
    # Mach-O fat/universal (0xcafebabe / 0xbebafeca).
    elf = head[:4] == b"\x7fELF"
    macho = head[:4] in (
        b"\xfe\xed\xfa\xce",
        b"\xfe\xed\xfa\xcf",
        b"\xce\xfa\xed\xfe",
        b"\xcf\xfa\xed\xfe",
        b"\xca\xfe\xba\xbe",
        b"\xbe\xba\xfe\xca",
    )
    pe = head[:2] == b"MZ"
    return elf or macho or pe


def _resolve_binary() -> str:
    """Find the native kronroe-mcp binary, skipping script wrappers."""
    explicit = os.environ.get("KRONROE_MCP_BIN")
    if explicit:
        return explicit

    name = "kronroe-mcp"
    path_dirs = os.environ.get("PATH", "").split(os.pathsep)

    for directory in path_dirs:
        candidate = os.path.join(directory, name)
        if not os.path.isfile(candidate):
            continue
        if not os.access(candidate, os.X_OK):
            continue
        if not _is_native_binary(candidate):
            continue
        return candidate

    raise FileNotFoundError(
        "Could not find native 'kronroe-mcp' binary on PATH. "
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
