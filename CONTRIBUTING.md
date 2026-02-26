# Contributing to Kronroe

Thank you for your interest in contributing! Kronroe is an embedded temporal property graph
database written in pure Rust, and contributions are very welcome.

## Before You Start

### Sign the CLA

Before your first pull request is merged, you must sign the
[Contributor Licence Agreement](./CLA.md).

A bot will prompt you automatically when you open a PR — just post the exact comment it
asks for. You do not need to sign in advance.

### Check existing issues and milestones

Browse the [GitHub milestones](https://github.com/kronroe/kronroe/milestones) to see what's
in scope for the current phase. Check [open issues](https://github.com/kronroe/kronroe/issues)
before starting work to avoid duplication.

**High-value contributions right now (Phase 0):**

| Area | Label |
|------|-------|
| Full-text index (tantivy) — `crates/core` | `phase-0` |
| Python bindings (PyO3) — new `crates/python/` | `python`, `phase-0` |
| MCP server — new `crates/mcp-server/` | `mcp`, `phase-0` |
| iOS XCFramework — new `crates/ios/` | `ios`, `phase-0` |
| CI pipeline improvements | `ci` |

## Development Environment

### Prerequisites

- Rust stable toolchain — install from https://rustup.rs
- `cargo` (included with Rust)
- macOS + Xcode (for iOS targets only)

### Setup

```bash
git clone https://github.com/kronroe/kronroe.git
cd kronroe
cargo build --all
```

### Running tests

```bash
cargo test --all --all-features
```

Tests use `tempfile` for temporary databases — no setup required.

### Linting and formatting

```bash
# Must pass with no warnings
cargo clippy --all --all-features -- -D warnings

# Check formatting
cargo fmt --all -- --check

# Apply formatting
cargo fmt --all
```

All three must pass before a PR can be merged. CI enforces this automatically.

### iOS target (macOS only)

```bash
rustup target add aarch64-apple-ios aarch64-apple-ios-sim
cargo build --target aarch64-apple-ios-sim -p kronroe
```

## Making a Pull Request

1. Fork the repository and create a branch from `main`
   ```bash
   git checkout -b feat/your-feature-name
   ```

2. Make your changes

3. Add tests for new behaviour. All public API changes require tests.

4. Run the full check suite:
   ```bash
   cargo test --all --all-features
   cargo clippy --all --all-features -- -D warnings
   cargo fmt --all -- --check
   ```

5. Commit with a clear message describing what and why

6. Push and open a PR against `main`

7. The CLA bot will prompt you to sign the CLA if you haven't already

## Coding Standards

### Documentation

All public items (`pub struct`, `pub fn`, `pub enum`, etc.) must have doc comments (`///`).
Include a short description and at least one usage example for new API surface.

```rust
/// Returns all facts currently known about the given entity.
///
/// "Current" means `valid_to` is `None` and `expired_at` is `None`.
///
/// # Example
///
/// ```rust,no_run
/// let facts = graph.all_facts_about("alice")?;
/// ```
pub fn all_facts_about(&self, subject: &str) -> Result<Vec<Fact>> {
    // ...
}
```

### Tests

- Every new public function needs at least one test
- Use `tempfile::NamedTempFile` for temporary databases in tests (see existing tests for examples)
- Test both the happy path and relevant error cases

### Error handling

Return `Result<T, KronroeError>` from fallible functions. Do not panic in library code.

### No unsafe (unless unavoidable)

Avoid `unsafe` code in the core crate. If you need `unsafe` (e.g. for FFI in `crates/ios/` or
`crates/python/`), add a comment explaining why it is sound.

## Architecture Notes

Kronroe is split into layered crates:

```
kronroe-agent-memory   ← high-level AgentMemory API
        ↓
   kronroe (core)      ← TemporalGraph, bi-temporal storage, redb
```

Future crates (`crates/python/`, `crates/ios/`, etc.) layer on top of `kronroe` core.

### Naming conventions

Use the repository naming standard in [`docs/NAMING-CONVENTIONS.md`](./docs/NAMING-CONVENTIONS.md).
Library entrypoints are explicitly declared in each crate `Cargo.toml` via `[lib] path` and must use named source files.

The core crate has **no C dependencies**. Keep it that way. If you need a C library, it belongs
in a separate crate with an explicit feature flag.

## Playground Security Notes (`site/`)

The browser playground is intentionally offline-first (no backend). Keep these protections in
place when making frontend changes:

- **Content Security Policy (CSP)** is defined in `/site/index.html` and should stay strict:
  - `default-src 'self'`
  - no plugin/object embedding (`object-src 'none'`)
  - narrow script/style/font/connect sources only as needed
- **Vite dev file serving scope** is restricted in `/site/vite.config.ts` (`server.fs.allow`).
  Do not broaden this without a strong reason.
- **Client-side safety limits** in `/site/src/main.ts` prevent easy browser lockups:
  - `MAX_REPLAY_FACTS` limits localStorage replay volume
  - `MAX_STORED_FACTS` limits in-memory/local persisted growth
  - `MAX_RENDER_FACTS` limits one-shot DOM rendering cost
  - `MAX_FIELD_LEN` caps user-entered field sizes

If you change any of the above, include rationale in the PR and validate with:

```bash
cd site
npm run build
```

## Scope Discipline

Phase 0 explicitly excludes the following. Please do not add them:

- Full Cypher/GQL query language parser
- Distributed or multi-node operation
- Cloud sync
- Schema migrations
- User-facing ACID transaction API

## Licences

Kronroe is dual-licensed under AGPL-3.0 and a commercial licence. By contributing (and signing
the CLA), you grant the project owner a perpetual licence to use your contribution under both
licences. You retain your own copyright. See [CLA.md](./CLA.md) for the full terms.

## Questions?

Open a [GitHub issue](https://github.com/kronroe/kronroe/issues) or email rebekah@kindlyroe.com.
