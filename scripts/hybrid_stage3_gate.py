#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import sys
import time
from pathlib import Path


def _load_summary(path: Path) -> dict:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError as exc:
        raise RuntimeError(f"missing summary file: {path}") from exc
    except json.JSONDecodeError as exc:
        raise RuntimeError(f"invalid JSON in summary file: {path}") from exc


def _write_json(path: Path, payload: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Compute a single PASS/FAIL gate decision from wrapper smoke summaries."
    )
    parser.add_argument(
        "--input",
        action="append",
        required=True,
        help="Path to a wrapper smoke JSON summary. Repeat for multiple wrappers.",
    )
    parser.add_argument(
        "--output",
        required=False,
        help="Optional path to write gate decision JSON.",
    )
    parser.add_argument(
        "--max-duration-ms",
        type=int,
        default=60000,
        help="Maximum allowed duration per wrapper run (default: 60000).",
    )
    args = parser.parse_args()

    input_paths = [Path(raw).resolve() for raw in args.input]
    summaries = [_load_summary(path) for path in input_paths]

    required_wrappers = {"npm", "python"}
    seen_wrappers = {
        item.get("wrapper")
        for item in summaries
        if isinstance(item.get("wrapper"), str) and item.get("wrapper")
    }

    failures: list[str] = []
    per_wrapper: dict[str, dict] = {}
    for item in summaries:
        wrapper = item.get("wrapper")
        if not isinstance(wrapper, str) or not wrapper:
            failures.append("summary missing wrapper field")
            continue

        status = item.get("status")
        duration_ms = item.get("duration_ms")
        error = item.get("error")

        wrapper_result = {
            "status": status,
            "duration_ms": duration_ms,
        }
        if error:
            wrapper_result["error"] = error
        per_wrapper[wrapper] = wrapper_result

        if status != "pass":
            failures.append(f"{wrapper}: status is {status!r}")
        if not isinstance(duration_ms, int):
            failures.append(f"{wrapper}: duration_ms is missing or non-integer")
        elif duration_ms > args.max_duration_ms:
            failures.append(
                f"{wrapper}: duration {duration_ms}ms exceeds limit {args.max_duration_ms}ms"
            )

    missing = sorted(required_wrappers - seen_wrappers)
    if missing:
        failures.append(f"missing required wrapper summaries: {', '.join(missing)}")

    decision = "PASS" if not failures else "FAIL"
    payload = {
        "schema_version": "1.0",
        "decision": decision,
        "timestamp_unix": int(time.time()),
        "max_duration_ms": args.max_duration_ms,
        "required_wrappers": sorted(required_wrappers),
        "inputs": [str(path) for path in input_paths],
        "per_wrapper": per_wrapper,
        "failures": failures,
    }

    if args.output:
        _write_json(Path(args.output).resolve(), payload)

    print(f"HYBRID_STAGE3_WRAPPER_GATE={decision}")
    if failures:
        print("Gate failures:", file=sys.stderr)
        for reason in failures:
            print(f"- {reason}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
