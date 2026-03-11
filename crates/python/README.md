# kronroe (Python)

Python bindings for Kronroe via PyO3/maturin.

## Quickstart

```python
from kronroe import AgentMemory

memory = AgentMemory.open("./my-agent.kronroe")
memory.assert_fact("alice", "works_at", "Acme")
results = memory.recall("where does Alice work?", limit=10)
scored = memory.recall_scored("where does Alice work?", limit=10)
# Options-dict alternative for scored recall (easier to extend over time)
scored2 = memory.recall_scored_with_options(
    "where does Alice work?",
    {
        "limit": 10,
        "min_confidence": 0.5,
        "confidence_filter_mode": "base",
    },
)
memory.assert_with_confidence("alice", "works_at", "Acme", 0.95, "user:notes")
print(results)
print(scored)
print(scored2)
```

`assert_fact` uses default confidence (`1.0`) with no source provenance.
Use `assert_with_confidence(..., source=...)` when you need explicit confidence/source metadata.

## Local build

```bash
cd crates/python
python -m pip install maturin
maturin develop
```

## Runtime validation

```bash
# Python runtime smoke test (builds local extension with cargo, then executes tests/runtime_smoke.py)
./scripts/run_runtime_smoke.sh

# Rust-side embedded-interpreter tests (runs without extension-module feature)
./scripts/run_rust_runtime_tests.sh
```

Optional feature toggles:

```bash
KRONROE_PY_FEATURES="hybrid uncertainty" ./scripts/run_runtime_smoke.sh
KRONROE_PY_RUST_TEST_FEATURES="hybrid uncertainty" ./scripts/run_rust_runtime_tests.sh
```

## Publish flow

```bash
# Build wheel(s)
python3 -m maturin build --release -o dist

# Upload to TestPyPI (token auth)
python3 -m pip install twine
TWINE_USERNAME=__token__ \
TWINE_PASSWORD="pypi-your-testpypi-token" \
python3 -m twine upload --repository-url https://test.pypi.org/legacy/ dist/*

# Upload to PyPI (token auth)
TWINE_USERNAME=__token__ \
TWINE_PASSWORD="pypi-your-pypi-token" \
python3 -m twine upload dist/*
```

Recommended for CI/release automation: configure PyPI Trusted Publisher for this
repository and publish from GitHub Actions without storing long-lived API tokens.
