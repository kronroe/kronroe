# kronroe-mcp (Python wrapper)

Python CLI wrapper for the Rust `kronroe-mcp` MCP server.

## Install

```bash
pip install kronroe-mcp
```

## Run

```bash
kronroe-mcp
```

The wrapper delegates to the Rust `kronroe-mcp` binary. It resolves binary path by:

1. `KRONROE_MCP_BIN` environment variable
2. `kronroe-mcp` on `PATH`

If not found, install the binary, for example:

```bash
cargo install --path crates/mcp-server
```

## Publish

```bash
cd python/kronroe-mcp
python3 -m pip install --upgrade build twine
python3 -m build

# TestPyPI
TWINE_USERNAME=__token__ \
TWINE_PASSWORD="pypi-your-testpypi-token" \
python3 -m twine upload --repository-url https://test.pypi.org/legacy/ dist/*

# PyPI
TWINE_USERNAME=__token__ \
TWINE_PASSWORD="pypi-your-pypi-token" \
python3 -m twine upload dist/*
```
