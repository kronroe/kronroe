#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from typing import BinaryIO


def _write_message(stdin: BinaryIO, payload: dict) -> None:
    body = json.dumps(payload, separators=(",", ":")).encode("utf-8")
    stdin.write(f"Content-Length: {len(body)}\r\n\r\n".encode("ascii"))
    stdin.write(body)
    stdin.flush()


def _read_message(stdout: BinaryIO) -> dict:
    content_length: int | None = None
    while True:
        line = stdout.readline()
        if not line:
            raise RuntimeError("unexpected EOF while reading MCP headers")
        line = line.rstrip(b"\r\n")
        if not line:
            break
        name, sep, value = line.partition(b":")
        if sep and name.lower() == b"content-length":
            content_length = int(value.strip())

    if content_length is None:
        raise RuntimeError("missing Content-Length in MCP response")
    body = stdout.read(content_length)
    if len(body) != content_length:
        raise RuntimeError("short MCP response body")
    return json.loads(body.decode("utf-8"))


def _wrapper_command(wrapper: str) -> list[str]:
    if wrapper == "npm":
        return ["node", "packages/kronroe-mcp/bin/kronroe-mcp.js"]
    if wrapper == "python":
        return [sys.executable, "-m", "kronroe_mcp.cli"]
    raise ValueError(f"unsupported wrapper: {wrapper}")


def _wrapper_env(wrapper: str, binary: Path, tmp: Path) -> dict[str, str]:
    env = os.environ.copy()
    env["KRONROE_MCP_BIN"] = str(binary)
    env["KRONROE_MCP_DB_PATH"] = str(tmp / "wrapper-smoke.kronroe")

    if wrapper == "python":
        existing_path = env.get("PYTHONPATH", "")
        env["PYTHONPATH"] = (
            "python/kronroe-mcp/src"
            if not existing_path
            else f"python/kronroe-mcp/src{os.pathsep}{existing_path}"
        )

    return env


def _run_smoke(wrapper: str, binary: Path) -> None:
    with tempfile.TemporaryDirectory(prefix=f"kronroe-{wrapper}-wrapper-smoke-") as tmpdir:
        tmp = Path(tmpdir)
        proc = subprocess.Popen(
            _wrapper_command(wrapper),
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            env=_wrapper_env(wrapper, binary, tmp),
        )
        try:
            if proc.stdin is None or proc.stdout is None:
                raise RuntimeError("failed to open wrapper stdio pipes")

            _write_message(
                proc.stdin,
                {
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "initialize",
                    "params": {},
                },
            )
            init = _read_message(proc.stdout)
            if init.get("result", {}).get("serverInfo", {}).get("name") != "kronroe-mcp":
                raise RuntimeError(f"initialize failed via {wrapper}: {init}")

            _write_message(
                proc.stdin,
                {
                    "jsonrpc": "2.0",
                    "id": 2,
                    "method": "tools/call",
                    "params": {
                        "name": "remember",
                        "arguments": {
                            "text": "alice works at Acme",
                            "query_embedding": [1.0, 0.0, 0.0],
                        },
                    },
                },
            )
            remember = _read_message(proc.stdout)
            fact_ids = (
                remember.get("result", {})
                .get("structuredContent", {})
                .get("fact_ids", [])
            )
            if not isinstance(fact_ids, list) or not fact_ids:
                raise RuntimeError(f"remember failed via {wrapper}: {remember}")

            _write_message(
                proc.stdin,
                {
                    "jsonrpc": "2.0",
                    "id": 3,
                    "method": "tools/call",
                    "params": {
                        "name": "recall_scored",
                        "arguments": {
                            "query": "alice",
                            "query_embedding": [1.0, 0.0, 0.0],
                            "use_hybrid": True,
                            "limit": 1,
                        },
                    },
                },
            )
            scored = _read_message(proc.stdout)
            rows = (
                scored.get("result", {})
                .get("structuredContent", {})
                .get("results", [])
            )
            if not rows:
                raise RuntimeError(f"recall_scored returned no rows via {wrapper}: {scored}")
            score_type = rows[0].get("score", {}).get("type")
            if score_type != "hybrid":
                raise RuntimeError(
                    f"expected hybrid score type via {wrapper}, got {score_type!r}: {scored}"
                )
        finally:
            if proc.stdin:
                proc.stdin.close()
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait(timeout=5)
            if proc.returncode not in (0, None):
                stderr = (proc.stderr.read() if proc.stderr else b"").decode(
                    "utf-8", errors="replace"
                )
                raise RuntimeError(
                    f"{wrapper} wrapper exited with code {proc.returncode}: {stderr}"
                )


def _write_summary(path: Path, summary: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser(description="Run wrapper-level MCP hybrid smoke tests.")
    parser.add_argument(
        "--wrapper",
        required=True,
        choices=["npm", "python"],
        help="Wrapper surface to verify.",
    )
    parser.add_argument(
        "--binary",
        required=True,
        help="Path to compiled kronroe-mcp binary to launch via wrapper.",
    )
    parser.add_argument(
        "--summary-out",
        required=False,
        help="Optional path to write a JSON summary file.",
    )
    args = parser.parse_args()

    binary = Path(args.binary).resolve()
    if not binary.exists():
        raise SystemExit(f"binary not found: {binary}")

    start = time.perf_counter()
    summary_path = Path(args.summary_out).resolve() if args.summary_out else None
    try:
        _run_smoke(args.wrapper, binary)
        duration_ms = int((time.perf_counter() - start) * 1000)
        if summary_path is not None:
            _write_summary(
                summary_path,
                {
                    "wrapper": args.wrapper,
                    "binary": str(binary),
                    "status": "pass",
                    "duration_ms": duration_ms,
                    "timestamp_unix": int(time.time()),
                },
            )
        print(f"{args.wrapper} wrapper hybrid smoke: PASS")
        return 0
    except Exception as exc:
        duration_ms = int((time.perf_counter() - start) * 1000)
        if summary_path is not None:
            _write_summary(
                summary_path,
                {
                    "wrapper": args.wrapper,
                    "binary": str(binary),
                    "status": "fail",
                    "duration_ms": duration_ms,
                    "timestamp_unix": int(time.time()),
                    "error": str(exc),
                },
            )
        print(f"{args.wrapper} wrapper hybrid smoke: FAIL: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
