#!/usr/bin/env node
/**
 * Render an OG card HTML file to a PNG screenshot at 1200×630.
 *
 * Reuses the Playwright install in tests/site/node_modules (already
 * present for the consent compliance suite). Single-purpose script,
 * not part of the consent test suite — kept here so we don't have
 * to install Playwright in another location.
 *
 * Usage (from repo root):
 *
 *   # 1. Make sure the static site preview server is running on port 5178.
 *   #    (e.g. python3 -m http.server 5178 --bind 127.0.0.1 --directory site)
 *   #
 *   # 2. Run from inside tests/site/ so node_modules is found:
 *   cd tests/site
 *   node scripts/render-og-image.mjs \
 *     --url=http://localhost:5178/og-images/post-why-kronroe.html \
 *     --out=../../site/blog/why-kronroe/og-image.png
 *
 * For future blog posts, copy site/og-images/post-template.html (when we
 * make one), edit the title/subtitle/date, then re-run this with the new
 * URL and out path.
 */

import { chromium } from 'playwright';
import { mkdirSync } from 'fs';
import { dirname, resolve } from 'path';

// ── Argument parsing ─────────────────────────────────────────

function parseArgs() {
  const args = process.argv.slice(2);
  const opts = {};
  for (const arg of args) {
    if (!arg.startsWith('--')) continue;
    const [k, v] = arg.slice(2).split('=');
    opts[k] = v ?? true;
  }
  if (!opts.url || !opts.out) {
    console.error('error: both --url=... and --out=... are required');
    console.error('example:');
    console.error('  node scripts/render-og-image.mjs \\');
    console.error('    --url=http://localhost:5178/og-images/post-why-kronroe.html \\');
    console.error('    --out=../../site/blog/why-kronroe/og-image.png');
    process.exit(1);
  }
  return opts;
}

// ── Render ──────────────────────────────────────────────────

async function render({ url, out }) {
  const outPath = resolve(out);
  mkdirSync(dirname(outPath), { recursive: true });

  const browser = await chromium.launch();

  // deviceScaleFactor=2 produces a 2400×1260 image that LinkedIn /
  // Twitter still scale to fit but renders sharper on retina previews.
  // Slightly larger file, but PNGs at this size compress well.
  const context = await browser.newContext({
    viewport: { width: 1200, height: 630 },
    deviceScaleFactor: 2,
  });
  const page = await context.newPage();

  // `networkidle` waits for fonts + any other late-loaded resources.
  // Critical because the card uses self-hosted woff2 fonts that need
  // to be ready before screenshot — otherwise we'd capture system-
  // font fallback rendering.
  await page.goto(url, { waitUntil: 'networkidle' });

  // Belt-and-braces: wait for document.fonts.ready in case
  // networkidle fired before all fonts finished loading.
  await page.evaluate(() => document.fonts.ready);

  await page.screenshot({
    path: outPath,
    type: 'png',
    omitBackground: false,
  });

  await browser.close();
  console.log(`wrote ${outPath}`);
}

const opts = parseArgs();
await render(opts);
