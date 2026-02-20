---
name: kronroe-check
description: >
  Run the Kronroe Rust build verification pipeline — tests, clippy lint, and format check —
  and report pass/fail for each step. Use this skill whenever working on Kronroe code and you
  need to verify everything builds cleanly, before committing, before creating a PR, after
  making changes, or whenever the user says "check", "verify", "run tests", "lint", "clippy",
  "fmt", or "CI" in the context of the Kronroe repo. Also trigger this after completing any
  code change in the kronroe/ directory, even if the user doesn't explicitly ask — catching
  problems early saves time.
---

# Kronroe Check

Run the three-step Kronroe CI pipeline locally and report results clearly.

The Kronroe repo uses a standard Rust quality gate that mirrors what CI runs on every PR.
Running all three steps together catches the most common issues: broken logic (tests),
subtle bugs and style issues (clippy), and formatting drift (fmt).

## Environment Setup

The Rust toolchain is installed via rustup at `$HOME/.cargo/bin`. Set PATH before
running any cargo commands:

```
export PATH="$HOME/.cargo/bin:$PATH"
```

The working directory for all commands is the Kronroe repo root (the directory
containing `Cargo.toml` with `[workspace]`).

## Pipeline Steps

Run these three commands in order. Each step is independent — run all three even
if an earlier one fails, so the user gets the full picture in one go.

### Step 1: Tests

```bash
cargo test --all 2>&1
```

This runs the full test suite across all crates (core, agent-memory, wasm).
Look at the exit code and the summary line (e.g., "test result: ok. 11 passed").

### Step 2: Clippy

```bash
cargo clippy --all -- -D warnings 2>&1
```

Clippy is Rust's linter. The `-D warnings` flag treats every warning as an error,
which is what CI enforces. A clean clippy run means no warnings at all.

### Step 3: Format Check

```bash
cargo fmt --all -- --check 2>&1
```

Checks that all code matches `rustfmt` style. If this fails, it prints a diff
showing what needs to change. The fix is `cargo fmt --all` (without `--check`).

## Reporting Results

After running all three steps, present a clear summary like this:

```
## Kronroe Check Results

| Step        | Result |
|-------------|--------|
| Tests       | PASS   |
| Clippy      | PASS   |
| Format      | FAIL   |

### Details
- **Tests**: 11 passed, 0 failed
- **Clippy**: Clean — no warnings
- **Format**: 2 files need formatting (run `cargo fmt --all` to fix)
```

If everything passes, keep it short — just the table and a one-liner like
"All clear, good to commit."

If something fails, include the relevant error output so the user (or you)
can fix it without re-running. For test failures, show the failing test name
and assertion. For clippy, show the warning. For fmt, mention which files
need formatting.

## Auto-fix

If format check fails and the user asks to fix it (or if you're about to commit
and formatting is the only issue), run:

```bash
cargo fmt --all
```

Then re-run the format check to confirm it's clean.

For clippy warnings, don't auto-fix — show the warning and let the user decide
how to address it, since clippy fixes can change behavior.
