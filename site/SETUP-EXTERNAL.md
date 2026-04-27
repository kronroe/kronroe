# Site setup — external services

A short reference for the external services that can't be set up in
code: search engine indexing and the newsletter provider.

---

## Status (as of 2026-04-27)

| Service | Status | Notes |
|---|---|---|
| **Google Search Console** | ✅ Verified, sitemap submitted | 4-URL sitemap will reflect on next deploy |
| **Bing Webmaster Tools** | ✅ Verified via GSC import, sitemap submitted | IndexNow not yet wired (see deferred work below) |
| **Buttondown** | ✅ Wired up — username `Kronroe` | Free tier, Keila is the long-term target (see migration plan) |

---

## 1. Google Search Console

Tells you what queries you rank for, surfaces indexing errors,
notifies you of broken pages.

### Setup (already done)

1. https://search.google.com/search-console/welcome
2. Domain property type, entered `kronroe.dev`
3. DNS TXT record added at registrar, verified
4. Submitted `https://kronroe.dev/sitemap.xml`

### What to watch for

- **Coverage report** (left nav → Pages) — should show all 4 URLs as
  "Indexed" within ~7 days. If any show "Discovered - currently not
  indexed" after a week, content quality may be flagged as thin.
- **Performance report** — first impressions and clicks. Don't expect
  much for the first 4-6 weeks; SEO is slow.
- **Core Web Vitals** — should be green. Re-check after a week of GA4
  data accumulates.

### When to re-submit the sitemap

Re-submission isn't needed for routine content updates — Google
recrawls automatically based on `lastmod` tags. Only re-submit if:

- The sitemap URL itself changes (it won't — `sitemap.xml` is stable)
- You've made a major site restructure with many URL changes
- You see "Couldn't fetch" status and need to retry

---

## 2. Bing Webmaster Tools

Bing search powers ChatGPT search results, Copilot, and DuckDuckGo's
fallback. Non-trivial AI-developer audience uses these.

### Setup (already done)

1. https://www.bing.com/webmasters
2. Imported from Google Search Console (same DNS verification)
3. Submitted same `https://kronroe.dev/sitemap.xml`

### Deferred: IndexNow integration

[IndexNow](https://www.indexnow.org/) lets you ping Bing/Yandex/DuckDuckGo
the moment a new page goes live, instead of waiting for crawl. Useful
once posting cadence is established (say, ≥4 posts).

To enable later:

1. In Bing Webmaster Tools, generate an IndexNow key
2. Drop the key file at `site/public/<key>.txt` (Bing tells you the
   exact filename)
3. Add a step in `.github/workflows/deploy-site.yml` that POSTs the
   list of changed URLs to `https://api.indexnow.org/indexnow` after
   each deploy

Skip until you're posting weekly+ — manual "Request indexing" via the
URL Inspection tool is faster than building this for infrequent posts.

---

## 3. Newsletter — Buttondown (current) → Keila (target)

### Why this two-stage decision

We evaluated Buttondown, Keila (managed + self-hosted), Listmonk,
Ghost (Pro + self-hosted), Beehiiv, Substack, and GCP-native options.
The two real contenders were **Buttondown** and **Keila**.

**Keila is our long-term target** — AGPL-3.0 (matches Kronroe), EU
hosted, privacy-first, has a clean self-host migration path. But Keila
has no free tier; managed costs ~€9/mo from day one.

**Buttondown is our on-ramp** — free tier covers up to 100 subscribers
(£0/mo), 5-minute setup, has native RSS-to-email automation. Lets us
launch newsletter capture with zero ongoing cost until traction
justifies the spend.

### Buttondown configuration

What's wired in code (already done):

- **Username**: `Kronroe`
- **Subscribe endpoint**: `https://buttondown.com/api/emails/embed-subscribe/Kronroe`
- **Form-submit redirect**: `/newsletter/thanks/` (handled in `email-capture.js`)

What you need to configure in the Buttondown UI:

- **Settings → General → Newsletter name**: `Kronroe Notes`
- **Settings → General → Description**: see suggested copy below
- **Settings → Email setup → From email**: `rebekah@kindlyroe.com`
- **Settings → Email setup → From name**: `Rebekah Cole`
- **Settings → Subscribers → Confirmation emails**: enable double opt-in
- **Settings → Subscribing → After subscribing**: `https://kronroe.dev/newsletter/thanks/`
   *(only used as a fallback for non-JS users — JS handler redirects there directly)*
- **Settings → Subscribing → After confirming**: `https://kronroe.dev/newsletter/confirmed/`
   *(this one is critical — Buttondown handles the email-link confirmation server-side and redirects here)*
- **Automations → RSS feeds**: feed URL `https://kronroe.dev/blog/feed.xml`, mode **Draft**
- **Automations → Welcome email**: enable + paste in welcome copy

#### Suggested newsletter description

> Build notes from Kronroe — the embedded bi-temporal graph database
> for AI agent memory and mobile/edge apps. New posts roughly every
> two weeks: technical decisions, what changed in the engine,
> occasional deep dives. Built and written by Rebekah Cole.

#### Suggested welcome email

```
Subject: Welcome to Kronroe Notes — what's next

Hi,

You just confirmed your subscription to Kronroe Notes — build updates
from the embedded bi-temporal graph database I'm working on in the
open. Thanks for that.

A few quick links:

→ Why we built Kronroe (long-form):
  https://kronroe.dev/blog/why-kronroe/

→ The repo:
  https://github.com/kronroe/kronroe

→ The docs:
  https://kronroe.dev/docs/

I'll send updates roughly every 2 weeks — what changed in the engine,
what I'm thinking about, occasional deep technical posts. No marketing
fluff.

If you ever want to reply, hit me back at rebekah@kindlyroe.com.

— Rebekah
```

### What's wired into the site

- `site/blog/index.html` and `site/blog/why-kronroe/index.html` have
  `<form data-kr-subscribe action="...buttondown.com/...Kronroe">`
- `site/js/email-capture.js` sends both `email` and `email_address`
  body keys (provider-agnostic — works for Keila/Kit/Listmonk too)
- CSP `connect-src` allows `https://buttondown.com` and
  `https://buttondown.email`
- GA4 `generate_lead` event fires on successful subscribe
  (consent-gated automatically)

### Migration triggers — when to switch to Keila

Move to Keila managed when **any one** of these is true:

| Signal | Threshold | Why this matters |
|---|---|---|
| Subscriber count | **≥80** | Approaching Buttondown's free-tier limit (100); migration before forced upgrade keeps optionality |
| PyPI downloads | **≥500/mo** for `kronroe-mcp` or `kronroe-py` | Real adoption signal — we're past idle phase |
| Commercial license interest | **First serious enquiry** | Brand alignment matters more once we're in commercial conversations; AGPL-on-AGPL is a clean story |
| Posting cadence | **6+ posts published over 3 months** | Content marketing is sticking; investing in a better newsletter stack is justified |

### Migration steps (for when the trigger fires)

The migration is intentionally small because we built it that way.

1. **Sign up at keila.io managed** (€9/mo) — pick `Kronroe` workspace
2. **Export subscribers from Buttondown** as CSV — Settings →
   Subscribers → Export
3. **Import the CSV into Keila** — Subscribers → Import. Keila
   preserves consent timestamps; double-check after import.
4. **Get the Keila form endpoint** — Settings → Forms → Embed
5. **Update two HTML files** — replace
   `https://buttondown.com/api/emails/embed-subscribe/Kronroe` with
   the new Keila form URL in:
   - `site/blog/index.html`
   - `site/blog/why-kronroe/index.html`
6. **Update CSP in `firebase.json`** — replace
   `https://buttondown.com https://buttondown.email` in `connect-src`
   with `https://keila.io` (or the self-hosted domain if you've gone
   that far)
7. **Reconfigure RSS-to-email in Keila** — point at
   `https://kronroe.dev/blog/feed.xml`, draft mode
8. **Cancel Buttondown** — keep the account dormant for 30 days as a
   read-only fallback in case the migration revealed any issues, then
   delete

Estimated migration time: **30-60 minutes**. Most of that is waiting
for DNS/CSP to propagate.

### Open improvement (deferred)

The RSS feed at `/blog/feed.xml` currently has `<description>` summaries
but no `<content:encoded>` with the full post body. This means
RSS-to-email creates "click to read more" emails rather than embedding
the full post.

For "draft mode" workflow (where you review each newsletter), this is
fine — you'd add the body manually before sending. If you ever want
full-post auto-emails, extend `site/scripts/build-sitemap.py` into a
sister script `build-feed.py` that walks each post's HTML body and
embeds it as `<![CDATA[...]]>` in `<content:encoded>`. Estimated 30
minutes of work.

---

## 4. Verification — confirm Buttondown wiring works end-to-end

Once the next deploy lands (PR #178 merging), test the full subscribe
flow:

1. Open `https://kronroe.dev/blog/why-kronroe/` in a private window
2. **Accept cookies** in the consent banner (otherwise GA events won't
   fire — that's correct gating, not a bug)
3. Scroll to the subscribe card at the bottom
4. Enter a fresh email address you control
5. Click **Subscribe**
6. Check that:
   - [ ] The button shows "Subscribed ✓"
   - [ ] The status text reads "Thanks — check your inbox to confirm."
   - [ ] Your inbox receives the Buttondown confirmation email
   - [ ] After confirming, the welcome email arrives
   - [ ] In **Buttondown subscribers list**, the email appears
   - [ ] In **GA4 Realtime → Events**, a `generate_lead` event fires

If any step fails, the issue is one of:

- **CSP blocking the POST** — open browser console, look for CSP
  violations. The fix is almost always a missing host in
  `connect-src`.
- **Username case mismatch** — Buttondown's URLs are case-sensitive.
  We're using `Kronroe` (capital K). If the form 404s, double-check.
- **Consent denied** — analytics events won't fire. This is *correct*
  behavior, not a bug. Subscribe will still work; just no GA event.

---

Last updated: 2026-04-27
