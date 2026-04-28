# OG card templates

Per-post Open Graph (social-share) preview images for blog posts.
LinkedIn / Twitter / Slack / Bluesky scrape `<meta property="og:image">`
when a URL is shared and lock the preview into their cache for **~30
days**, so getting these right *before* sharing is more important than
fixing them after.

## What's here

`post-<slug>.html` files — one per blog post. Each is a self-contained
HTML page that renders at exactly 1200×630 pixels (the Open Graph
standard, also LinkedIn-recommended). The page is rendered headlessly
to PNG and the PNG is checked into the post's directory.

This directory is **not deployed** — `.github/workflows/deploy-site.yml`
copies `site/blog/`, `site/docs/`, etc. but skips `site/og-images/` —
so the template HTML stays as build source-of-truth without leaking
to production.

## Workflow for a new post

When you ship a new blog post:

1. Copy an existing `post-<slug>.html` and rename for your post:
   ```bash
   cp site/og-images/post-why-kronroe.html \
      site/og-images/post-<your-slug>.html
   ```

2. Edit the file. The fields that change per post:
   - `<title>` (browser tab — irrelevant to the rendered PNG but tidy)
   - The **eyebrow row**: category and date
   - The **title** (h1) — the dominant element
   - The **subtitle/lede** — 1-2 lines explaining what the post is about
   - The **annotation** — optional Virgil-handwritten one-liner
   - **Author block** — usually unchanged

3. Make sure the static preview server is running on port 5178:
   ```bash
   python3 -m http.server 5178 --bind 127.0.0.1 --directory site
   ```

4. Render the PNG:
   ```bash
   cd tests/site
   node scripts/render-og-image.mjs \
     --url=http://localhost:5178/og-images/post-<your-slug>.html \
     --out=../../site/blog/<your-slug>/og-image.png
   ```

5. Verify the result by opening the PNG and checking it looks right.
   Common issues:
   - Title too long → it'll truncate at 4 lines but might look cramped.
     Shorten the title in the post itself.
   - Custom font didn't load → the renderer waits for `document.fonts.ready`
     so this should be reliable. If it fails, the screenshot would show
     system-font fallback. Re-run.

6. Update the blog post's `<meta property="og:image">` if needed
   (though by convention it's always `<post-url>/og-image.png` so
   should already match).

7. Commit the new `.html` template and the rendered `.png` together.

## Why we don't auto-generate at build time

For a 1-3-post blog, manual rendering is fine and produces better
results than a generic auto-template would. When the blog hits 4+
posts, revisit: extend the template to take title/subtitle/date as
URL params, integrate the render script into the deploy workflow,
remove the manual step.

The render script (`tests/site/scripts/render-og-image.mjs`) is
already CLI-friendly so this future automation is a small refactor,
not a rewrite.

## File-size budget

LinkedIn caps OG images at 5MB; under 200KB is the sweet spot for
fast scraping. The render script outputs at `deviceScaleFactor: 2`
(2400×1260 actual pixels) for sharp retina previews — typical PNG
output is 100-200KB, well under the cap.

## Don't

- Don't put copyrighted imagery (we use only Kronroe brand assets)
- Don't put user-input data into a template that gets shared widely
- Don't make the title smaller than ~80px — the card needs to be
  readable when scaled to LinkedIn feed size (~480px wide)
