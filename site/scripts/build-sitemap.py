#!/usr/bin/env python3
"""
Generate sitemap.xml for kronroe.dev.

Walks site/blog/*/index.html, extracts each post's `article:published_time`
meta tag, and emits a complete sitemap including the static landmarks
(homepage, /docs/, /blog/) plus every discovered blog post.

Usage (from repo root):
    python3 site/scripts/build-sitemap.py            # write to site/public/sitemap.xml
    python3 site/scripts/build-sitemap.py --check    # exit 1 if file would change

Why pure stdlib: this script runs in CI (GitHub Actions Ubuntu runners
have Python 3 preinstalled) and locally without any pip install. No
external deps.

Exit codes:
    0 — sitemap written (or unchanged in --check mode)
    1 — --check mode found drift; or fatal error
"""

import argparse
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]  # repo root
SITE_DIR = ROOT / "site"
BLOG_DIR = SITE_DIR / "blog"
OUTPUT = SITE_DIR / "public" / "sitemap.xml"

BASE_URL = "https://kronroe.dev"

# Static landmarks that always exist. Lastmod here represents the date
# of the last meaningful content/structure change for the page — bump
# manually when you ship a redesign or major copy change.
STATIC_PAGES = [
    {"loc": "/", "lastmod": "2026-04-13", "changefreq": "weekly", "priority": "1.0"},
    {"loc": "/docs/", "lastmod": "2026-04-13", "changefreq": "weekly", "priority": "0.9"},
    {"loc": "/blog/", "lastmod": "2026-04-13", "changefreq": "weekly", "priority": "0.8"},
    {"loc": "/about/", "lastmod": "2026-04-27", "changefreq": "monthly", "priority": "0.7"},
    {"loc": "/pricing/", "lastmod": "2026-04-27", "changefreq": "monthly", "priority": "0.7"},
    {"loc": "/privacy/", "lastmod": "2026-04-27", "changefreq": "yearly", "priority": "0.4"},
]

PUBLISHED_TIME_RE = re.compile(
    r'<meta\s+property="article:published_time"\s+content="([^"]+)"',
    re.IGNORECASE,
)


def find_blog_posts() -> list[dict[str, str]]:
    """Walk site/blog/*/index.html and return {loc, lastmod} for each post."""
    posts = []
    if not BLOG_DIR.is_dir():
        return posts

    for post_dir in sorted(BLOG_DIR.iterdir()):
        if not post_dir.is_dir():
            continue
        index_html = post_dir / "index.html"
        if not index_html.is_file():
            continue

        html = index_html.read_text(encoding="utf-8")
        match = PUBLISHED_TIME_RE.search(html)
        if not match:
            print(
                f"warning: {index_html.relative_to(ROOT)} has no "
                f"<meta property=\"article:published_time\"> — skipping",
                file=sys.stderr,
            )
            continue

        # ISO datetime → date (sitemap.xml only needs YYYY-MM-DD)
        lastmod = match.group(1).split("T", 1)[0]
        posts.append({
            "loc": f"/blog/{post_dir.name}/",
            "lastmod": lastmod,
            "changefreq": "monthly",
            "priority": "0.7",
        })

    return posts


def render_sitemap(entries: list[dict[str, str]]) -> str:
    lines = [
        '<?xml version="1.0" encoding="UTF-8"?>',
        '<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">',
    ]
    for e in entries:
        lines.extend([
            "  <url>",
            f"    <loc>{BASE_URL}{e['loc']}</loc>",
            f"    <lastmod>{e['lastmod']}</lastmod>",
            f"    <changefreq>{e['changefreq']}</changefreq>",
            f"    <priority>{e['priority']}</priority>",
            "  </url>",
        ])
    lines.append("</urlset>")
    return "\n".join(lines) + "\n"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--check",
        action="store_true",
        help="Exit 1 if sitemap.xml would change (for CI drift detection).",
    )
    args = parser.parse_args()

    entries = STATIC_PAGES + find_blog_posts()
    new_content = render_sitemap(entries)

    if args.check:
        existing = OUTPUT.read_text(encoding="utf-8") if OUTPUT.exists() else ""
        if existing != new_content:
            print(
                f"error: {OUTPUT.relative_to(ROOT)} is out of date — "
                "run: python3 site/scripts/build-sitemap.py",
                file=sys.stderr,
            )
            return 1
        print(f"ok: {OUTPUT.relative_to(ROOT)} is up to date")
        return 0

    OUTPUT.write_text(new_content, encoding="utf-8")
    print(f"wrote: {OUTPUT.relative_to(ROOT)} ({len(entries)} URLs)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
