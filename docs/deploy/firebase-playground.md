# Firebase Playground Deploy (Issue #36)

## Why we do this
Kronroe's Phase 0 scope includes a live, publicly testable WASM playground. This deploy path ensures every `main` merge touching site/WASM ships to Firebase Hosting and is validated with a browser smoke test.

## Live URL
- Primary: https://kronroe.web.app
- Alt default domain: https://kronroe.firebaseapp.com

## Prerequisites
1. GitHub Actions secret exists:
   - `FIREBASE_SERVICE_ACCOUNT_KRONROE`
2. Firebase project id remains `kronroe` in `.firebaserc`.
3. (Optional custom domain) DNS + Firebase Hosting custom domain mapping completed in Firebase Console.

## CI workflow path
Workflow file: `.github/workflows/deploy-site.yml`

What it does on `main` push (site/wasm/firebase config changes):
1. Builds `crates/wasm` with `wasm-pack` into `site/public/pkg`.
2. Builds the Vite site.
3. Deploys to Firebase live channel.
4. Runs post-deploy Playwright smoke test against deployed URL.

## Smoke test contract
Script: `site/scripts/smoke-playground.mjs`

Checks:
1. Load page + WASM boot completes.
2. Assert fact succeeds.
3. Query by entity + predicate returns result.
4. Retract action succeeds.

## Manual rerun
From GitHub UI: Actions -> `Deploy Site` -> `Run workflow`.

## Recording evidence after deploy
For each successful run, capture:
1. Deployed URL (`steps.deploy.outputs.details_url`).
2. Smoke test log JSON output from the `Run post-deploy smoke test` step.
3. Workflow run URL in issue/PR notes.

This satisfies issue #36 acceptance criteria:
- Live URL documented
- Deploy workflow green
- Post-deploy assert/query/retract smoke recorded
