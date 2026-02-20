# kronroe (Python)

Python bindings for Kronroe via PyO3/maturin.

## Quickstart

```python
from kronroe import AgentMemory

memory = AgentMemory.open("./my-agent.kronroe")
memory.assert_fact("alice", "works_at", "Acme")
results = memory.search("where does Alice work?", 10)
print(results)
```

## Local build

```bash
cd crates/python
python -m pip install maturin
maturin develop
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
