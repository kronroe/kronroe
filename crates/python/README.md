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
# TestPyPI
maturin publish --repository testpypi

# PyPI
maturin publish
```
