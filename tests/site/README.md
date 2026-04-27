# Kronroe site — consent compliance harness

Playwright tests covering the cookie-consent + analytics gating on
`kronroe.dev`. Run these whenever you touch:

- `site/js/analytics-consent.js`
- The footer "Cookie preferences" link on any page
- Anything that sets cookies or fires analytics

## Setup (once)

```bash
cd tests/site
npm install
npx playwright install chromium
```

## Run

```bash
# Headless (default) — CI mode
npm test

# Headed — watch the tests drive the browser
npm run test:headed

# Interactive runner with timeline UI
npm run test:ui
```

Playwright auto-starts a `python3 -m http.server` against the source
`site/` directory on port 5180. If that port is already in use (e.g. the
`site-static` launch profile is still running on 5178 — different port,
no clash) the tests will reuse it locally; in CI it fails fast.

## What's covered

| # | Flow | Asserts |
|---|------|---------|
| 1 | First visit | banner shown, no cookie, GA `consent default = denied`, no LinkedIn |
| 2 | Accept all | versioned record written, all 4 GA signals → `granted`, LinkedIn injected, ~12-month expiry |
| 3 | Reject all | record written denied, GA stays denied, LinkedIn never injected |
| 4 | Customise (analytics only) | exact toggle combination persists; ad signals stay denied |
| 5 | Withdraw via footer link | GA flips back to denied; LinkedIn script remains in DOM (withdraw = stop future) |
| 6 | Expired record | banner re-prompts even though browser cookie is still alive |
| 7 | Re-open + focus | modal reopens with prefilled choices; Escape closes it; focus returns to trigger |
| + | Policy version bump | stored cookie with old `policyVersion` triggers re-prompt |
| + | ARIA dialog | `role=dialog`, `aria-modal=true`, `aria-labelledby` set, background `inert`, focus trap wraps |

## Adding new tests

Follow the existing pattern in `specs/consent.spec.ts`:

- Each test starts on a fresh `/` with no cookies (Playwright contexts are
  isolated by default — no extra setup needed)
- Use the helper functions at the top (`getConsentCookie`,
  `getConsentRecord`, `getLastConsentUpdate`, `bannerVisible`,
  `modalVisible`, `linkedInLoaded`)
- Assert on **shapes** (cookie contents, dataLayer entries, DOM presence),
  not on real GA4/LinkedIn beacons. Those external services are out of
  scope for CI stability.

## CI integration (future)

To wire into GitHub Actions, add a `.github/workflows/site-tests.yml`
that runs on changes to `site/**` or `tests/site/**`:

```yaml
- run: cd tests/site && npm ci && npx playwright install --with-deps chromium
- run: cd tests/site && npm test
```
