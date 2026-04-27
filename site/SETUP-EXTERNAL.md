# Site setup — external services

A short, do-once checklist for the things that can't be set up in code:
search engine indexing and the newsletter provider. Estimated time:
**~45 minutes total**.

Work through the sections in order. Each section is independent — you
can stop after any one and pick up later.

---

## 1. Google Search Console (15 min)

Tells you what queries you rank for, surfaces indexing errors,
notifies you of broken pages.

### Steps

1. Go to https://search.google.com/search-console/welcome
2. Pick **Domain** property type (not URL prefix) and enter `kronroe.dev`
3. Google will give you a TXT record to add to DNS — paste it into your
   domain registrar's DNS panel. Wait ~5 min for propagation.
4. Click **Verify** in Search Console
5. Once verified, go to **Sitemaps** in the left nav
6. Submit `https://kronroe.dev/sitemap.xml`
7. Done. First crawl usually shows up in 24-48 hrs.

### What to watch for in the first month

- **Coverage** report — should show all 4 URLs from the sitemap as
  "Indexed". If any show "Discovered - currently not indexed" after a
  week, content quality may be flagged as thin.
- **Performance** report — first impressions and clicks. Don't expect
  much for the first 4-6 weeks; SEO is slow.
- **Core Web Vitals** — should be green. If not, we'll fix it.

---

## 2. Bing Webmaster Tools (10 min)

Bing search powers ChatGPT search results, Copilot, and DuckDuckGo's
fallback. Non-trivial AI-developer audience uses these.

### Steps

1. Go to https://www.bing.com/webmasters
2. Sign in with a Microsoft account
3. Pick **Import from Google Search Console** (saves verification —
   uses the same DNS record). If GSC isn't done yet, use the manual
   verification with a meta tag.
4. Submit the same `https://kronroe.dev/sitemap.xml`
5. Done.

---

## 3. Buttondown newsletter (20 min)

Indie newsletter service, $9/mo, Markdown editor, has API + RSS-to-newsletter.

### Steps

1. Sign up at https://buttondown.com — pick a username (this becomes
   part of your subscribe URL, so make it short and brand-aligned, e.g.
   `kronroe`)
2. In Buttondown settings, set:
   - **From email**: `rebekah@kindlyroe.com` (or wherever you want
     replies to go)
   - **Welcome email**: short, warm, links to the why-kronroe post +
     GitHub repo. Sample below.
   - **Confirmation email**: enable double opt-in (GDPR-safe, prevents
     spam signups poisoning your list)
3. Enable **RSS-to-email** and point it at `https://kronroe.dev/blog/feed.xml`
   so new posts auto-trigger a newsletter draft for review.

### Wire the form into the site

Once you have your Buttondown username, replace the placeholder in
two files:

```bash
# Repo-relative paths
site/blog/index.html
site/blog/why-kronroe/index.html
```

Find every occurrence of `REPLACE_WITH_BUTTONDOWN_USERNAME` and replace
with your actual username (e.g. `kronroe`). Then update the CSP in
`firebase.json` to allow Buttondown:

```diff
-connect-src 'self' ws: wss: https://www.google-analytics.com https://analytics.google.com https://px.ads.linkedin.com;
+connect-src 'self' ws: wss: https://www.google-analytics.com https://analytics.google.com https://px.ads.linkedin.com https://buttondown.com;
```

Test locally before pushing:

```bash
# Start the static preview server
python3 -m http.server 5178 --bind 127.0.0.1 --directory site

# Open http://localhost:5178/blog/why-kronroe/, scroll to the form,
# enter your own email, and check that:
# - the button shows "Subscribed ✓"
# - the status text reads "Thanks — check your inbox to confirm."
# - your inbox gets the Buttondown confirmation email
```

### Sample welcome email

```
Subject: Welcome to Kronroe — what's next

Hi,

You just subscribed to updates from Kronroe — the embedded bi-temporal
graph database I'm building in the open. Thanks for that.

A few quick links to get started:

- Why we built Kronroe (the long-form version):
  https://kronroe.dev/blog/why-kronroe/

- The repo:
  https://github.com/kronroe/kronroe

- The docs:
  https://kronroe.dev/docs/

I send out updates roughly every 2 weeks. No marketing fluff — just
what changed in the engine, what I'm thinking about, and the
occasional deep technical post.

If you ever want to reply, hit me back at rebekah@kindlyroe.com.

— Rebekah
```

---

## 4. Verify everything works (5 min)

Once Buttondown is wired and the CSP is updated, push the changes and
let the deploy go. Then:

- [ ] Open https://kronroe.dev/blog/why-kronroe/ in a private window
- [ ] Accept cookies (so the consent banner doesn't block the test)
- [ ] Subscribe with a fresh email address
- [ ] Confirm the email arrives in inbox + welcome email lands
- [ ] In **Google Analytics 4 Realtime**, confirm a `generate_lead`
      event fires
- [ ] In **Buttondown's subscribers list**, confirm the email appears

If any of these fail, the issue is one of:
- CSP blocking the POST (check browser console for CSP errors)
- Buttondown username mismatch (check the form's `action` URL)
- Consent denied (analytics events won't fire — that's *correct*
  behavior, not a bug)

---

## What's deliberately not in this list

A few things I considered but decided to skip until you have signal:

- **Plausible / Fathom side-by-side with GA4** — diminishing returns
  until you have >1k visitors/mo. GA4 + LinkedIn covers the core
  questions for now.
- **Twitter / X / Bluesky / Mastodon meta tags** — `og:` and `twitter:`
  cards already cover the major scrapers. Adding more rarely changes
  click-through.
- **A `humans.txt`** — cute but no real signal. Skip.
- **`security.txt`** — worth doing once you have CVE-able surface area
  (i.e. a published Rust crate getting downloaded). Phase 1 task.

---

Last updated: 2026-04-27
