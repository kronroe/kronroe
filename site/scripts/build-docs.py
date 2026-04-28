#!/usr/bin/env python3
"""
Build the Kronroe docs site from `site/docs/**/*.md`.

Reads markdown files, renders them to HTML matching the Kronroe design
system (Plus Jakarta Sans body + JetBrains Mono code + four-color logo
stripe), and emits per-doc HTML pages with auto-generated sidebar
navigation, on-page TOC, and prev/next links.

Phase 1 of the docs pipeline plan in
`.ideas/PLAN_docs_pipeline.md` — see that doc for the full strategy
(human-readable Phase 1 → agent formats Phase 2 → docs API/MCP Phase 3).

Usage (run from repo root):

    site/scripts/.venv/bin/python site/scripts/build-docs.py
    site/scripts/.venv/bin/python site/scripts/build-docs.py --check

Setup the venv once with:

    python3 -m venv site/scripts/.venv
    site/scripts/.venv/bin/pip install markdown-it-py mdit-py-plugins pygments

Output: writes rendered HTML to `site/docs-built/`. The deploy workflow
copies that directory into `site/dist/docs/`. Source `site/docs/` keeps
markdown files only — never mix source and build output.

Why custom (not VitePress / Mintlify):
  * Full control over the design system (three-voice typography,
    four-color logo stripe, code block styling).
  * Multi-format output planned (Phase 2 emits llms.txt + ?format=md
    alongside HTML).
  * Future Kronroe-MCP integration (Phase 3) is easier from a script
    we own than from inside a docs framework.

Conventions:
  * One markdown file = one rendered HTML page at /docs/<rel>/.
  * The first H1 in each markdown file is the page title.
  * The first paragraph after the title is the page description (used
    for meta tags).
  * Directory name = sidebar category (e.g. site/docs/getting-started
    becomes "Getting Started").
  * Output lives at site/docs-built/<rel>/index.html so URLs are clean
    (`/docs/getting-started/what-is-kronroe/` not `.html`).
"""

from __future__ import annotations

import argparse
import json
import re
import shutil
import sys
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

# Third-party (installed via site/scripts/.venv).
from markdown_it import MarkdownIt
from mdit_py_plugins.anchors import anchors_plugin
from mdit_py_plugins.front_matter import front_matter_plugin
from pygments import highlight
from pygments.formatters import HtmlFormatter
from pygments.lexers import get_lexer_by_name
from pygments.util import ClassNotFound

# ─── Paths ────────────────────────────────────────────────────

ROOT = Path(__file__).resolve().parents[2]
DOCS_SRC = ROOT / "site" / "docs"
OUTPUT = ROOT / "site" / "docs-built"

# ─── Data model ───────────────────────────────────────────────

@dataclass
class Heading:
    """One H2 or H3 heading, used for the on-page TOC."""

    level: int  # 2 or 3
    text: str
    anchor: str  # slug for href="#..."


@dataclass
class Doc:
    """One rendered markdown file ready to emit as HTML."""

    rel_path: str  # e.g. "getting-started/what-is-kronroe"
    category_slug: str  # e.g. "getting-started"
    category_title: str  # e.g. "Getting Started"
    title: str  # from first H1
    description: str  # from first paragraph after the title
    body_html: str  # rendered markdown content
    body_md: str  # raw markdown source (for Phase 2 ?format=md)
    headings: list[Heading] = field(default_factory=list)

    @property
    def url(self) -> str:
        return f"/docs/{self.rel_path}/"


# ─── Markdown parsing ─────────────────────────────────────────

def slugify(text: str) -> str:
    """Convert "What is Kronroe?" → "what-is-kronroe"."""
    text = text.lower()
    text = re.sub(r"[^\w\s-]", "", text)
    text = re.sub(r"[\s_-]+", "-", text)
    return text.strip("-")


def category_title_from_slug(slug: str) -> str:
    """`getting-started` → `Getting Started`. `api` → `API`."""
    if slug.upper() == "API":
        return "API"
    return slug.replace("-", " ").title()


def render_code_block(code: str, lang: str | None) -> str:
    """Render a fenced code block with Pygments syntax highlighting.

    Falls back to plain `<pre><code>` if the language isn't recognised
    so we never lose content to a bad lexer name.
    """
    try:
        lexer = get_lexer_by_name(lang or "text", stripall=True)
    except ClassNotFound:
        lexer = get_lexer_by_name("text", stripall=True)

    formatter = HtmlFormatter(
        cssclass="kr-code",
        nowrap=False,
        # Use Pygments' "default" classes — we ship our own CSS that
        # styles them in the Kronroe palette.
    )
    highlighted = highlight(code, lexer, formatter)

    label = (lang or "text").upper()
    return (
        f'<div class="kr-code-wrap" data-lang="{lang or "text"}">'
        f'<div class="kr-code-bar"><span class="kr-code-lang">{label}</span>'
        f'<button class="kr-code-copy" type="button" '
        f'aria-label="Copy code">Copy</button></div>'
        f"{highlighted}"
        f"</div>"
    )


def make_markdown_renderer() -> MarkdownIt:
    """Set up markdown-it with anchors, frontmatter, tables, and our
    Pygments-backed code block renderer."""
    md = (
        MarkdownIt("commonmark", {"html": True})
        .enable("table")
        .enable("strikethrough")
        .use(front_matter_plugin)
        .use(
            anchors_plugin,
            min_level=2,
            max_level=4,
            slug_func=slugify,
            permalink=False,
        )
    )

    # Override the fenced code block renderer to inject Pygments output.
    # markdown-it-py calls these as plain functions: (tokens, idx, options, env).
    def render_fence(tokens, idx, options, env):
        token = tokens[idx]
        lang = token.info.strip().split()[0] if token.info else None
        return render_code_block(token.content, lang)

    md.renderer.rules["fence"] = render_fence
    return md


# ─── Walk + parse ─────────────────────────────────────────────

def find_markdown_files(docs_root: Path) -> list[Path]:
    """Return all .md files under docs_root, sorted for stable output."""
    return sorted(docs_root.rglob("*.md"))


def extract_title_and_description(html: str) -> tuple[str, str]:
    """Pull the first H1 as title and the first paragraph as description.

    Both are returned as plain text (HTML-stripped) for use in <meta>
    tags. The H1 is removed from `html` after extraction since we
    render it ourselves in the page header.
    """
    title_match = re.search(r"<h1[^>]*>(.*?)</h1>", html, re.IGNORECASE | re.DOTALL)
    title = re.sub(r"<[^>]+>", "", title_match.group(1)).strip() if title_match else "Untitled"

    # First paragraph that follows the H1 (or the first paragraph anywhere).
    desc_match = re.search(r"<p>(.*?)</p>", html, re.IGNORECASE | re.DOTALL)
    description = (
        re.sub(r"<[^>]+>", "", desc_match.group(1)).strip() if desc_match else ""
    )
    # Trim to a sensible length for meta description.
    if len(description) > 200:
        description = description[:197].rstrip() + "..."

    # Remove the H1 from body since the page template renders it separately.
    html_without_title = re.sub(
        r"<h1[^>]*>.*?</h1>", "", html, count=1, flags=re.IGNORECASE | re.DOTALL
    )
    return title, description, html_without_title


def extract_headings(html: str) -> list[Heading]:
    """Pull H2 and H3 headings + their anchor IDs for the on-page TOC."""
    pattern = re.compile(
        r'<h(2|3)[^>]*id="([^"]+)"[^>]*>(.*?)</h\1>', re.IGNORECASE | re.DOTALL
    )
    headings = []
    for level_str, anchor, raw_text in pattern.findall(html):
        text = re.sub(r"<[^>]+>", "", raw_text).strip()
        headings.append(Heading(level=int(level_str), text=text, anchor=anchor))
    return headings


def parse_doc(md_path: Path, md: MarkdownIt) -> Doc:
    """Read one markdown file and return a fully-resolved Doc."""
    body_md = md_path.read_text(encoding="utf-8")
    rendered = md.render(body_md)
    title, description, body_html = extract_title_and_description(rendered)
    headings = extract_headings(body_html)

    rel = md_path.relative_to(DOCS_SRC).with_suffix("")
    rel_str = str(rel).replace("\\", "/")

    parts = rel_str.split("/")
    category_slug = parts[0] if len(parts) > 1 else ""
    category_title = category_title_from_slug(category_slug) if category_slug else ""

    return Doc(
        rel_path=rel_str,
        category_slug=category_slug,
        category_title=category_title,
        title=title,
        description=description,
        body_html=body_html,
        body_md=body_md,
        headings=headings,
    )


# ─── Corpus pipeline (Phase 3b.1) ─────────────────────────────
#
# Builds a per-section embedding corpus consumed by the
# `kronroe-docs-api` runtime (Phase 3b.2). The corpus is the data
# Phase 3 promises to expose at /api/docs/recall and similar.
#
# Why split at H2 rather than per-doc:
#   * 9 docs is too few for useful semantic recall — every query
#     would recall the same handful of giant blobs, defeating the
#     "find the precise relevant passage" property of vector search.
#   * Each H2 in our corpus is naturally one self-contained idea
#     (a method on the API, a concept, a setup step) — exactly the
#     unit a query like "how do I correct a fact" wants to land on.
#
# Why fenced-code-block awareness matters:
#   * `quick-start-python.md` has lines starting with `# ` that
#     aren't headings — they're Python comments inside fenced code
#     blocks (`# Basic assertion`, `# With confidence score`).
#     A naive line-based heading splitter creates ghost sections
#     from these. The state machine below tracks fence depth.

@dataclass
class Section:
    """One H2-bounded chunk of a doc. Phase 3b.1 emits these into
    `corpus.json`; Phase 3b.2 loads them as Kronroe facts with
    embeddings.

    The `id` follows the URL — `<doc_path>/<anchor>` for H2 sections
    and `<doc_path>/intro` for the doc's preamble (everything between
    the H1 and the first H2). Globally unique, human-readable for
    debugging, and trivially derives the canonical URL by appending
    `#<anchor>` (or no fragment for `intro`).
    """

    id: str
    doc_path: str  # e.g. "concepts/bi-temporal-model"
    doc_url: str  # e.g. "/docs/concepts/bi-temporal-model/"
    doc_title: str  # e.g. "Bi-Temporal Model"
    category: str  # e.g. "Concepts"
    heading: str  # e.g. "Two Time Dimensions" — or "" for intro
    anchor: str  # slug of heading — or "" for intro
    body: str  # plain markdown text of the section, headings excluded
    symbols: list[str] = field(default_factory=list)


def split_doc_into_sections(doc: Doc) -> list[Section]:
    """Walk `doc.body_md` and emit one Section per H2 block, plus
    one for the doc's preamble (the lede paragraph between H1 and the
    first H2 — usually the doc's most valuable summary text).

    The walk is line-by-line with a simple fenced-code-block depth
    counter — enter a fence when we see a line that's exactly
    `` ``` `` (optionally followed by a language tag), exit when we
    see another such line. Heading detection only fires when fence
    depth is 0.
    """
    lines = doc.body_md.split("\n")
    fence_open = False
    sections: list[Section] = []

    # Buffer for the current section. The "intro" section starts
    # implicitly at line 1 (after the H1).
    current_heading = ""  # empty = intro section
    current_anchor = ""
    buffer: list[str] = []

    def flush(heading: str, anchor: str, body_lines: list[str]) -> None:
        body = "\n".join(body_lines).strip()
        if not body:
            # Skip empty sections — happens when a doc has no preamble
            # before its first H2, or two H2s back-to-back.
            return
        anchor_part = anchor if anchor else "intro"
        sections.append(
            Section(
                id=f"{doc.rel_path}/{anchor_part}",
                doc_path=doc.rel_path,
                doc_url=doc.url,
                doc_title=doc.title,
                category=doc.category_title,
                heading=heading,
                anchor=anchor,
                body=body,
            )
        )

    for line in lines:
        stripped = line.lstrip()

        # Fenced code-block boundary detection. Real fences are
        # exactly 3+ backticks at the start of a line (after
        # optional indentation), optionally followed by a language
        # tag. Inline backticks like `foo` aren't fences.
        if stripped.startswith("```"):
            fence_open = not fence_open
            buffer.append(line)
            continue

        if fence_open:
            buffer.append(line)
            continue

        # Skip the doc's H1 line (we've already extracted it as
        # doc.title and it would be the only "section heading"
        # before the intro otherwise).
        if line.startswith("# ") and not buffer and not sections and not current_heading:
            continue

        # H2 boundary outside a code fence — emit the previous
        # section and start a new one.
        if line.startswith("## "):
            flush(current_heading, current_anchor, buffer)
            current_heading = line[3:].strip()
            current_anchor = slugify(current_heading)
            buffer = []
            continue

        buffer.append(line)

    # Final flush at EOF.
    flush(current_heading, current_anchor, buffer)
    return sections


# Curated allowlist of Kronroe API symbols that appear in the docs.
# Sourced from `CLAUDE.md`'s "Key Types" tables. The point isn't to
# be exhaustive — it's to flag the strings that uniquely refer to a
# Kronroe surface, so `/api/docs/symbols/<name>` resolves cleanly.
#
# Anything not on this list is ignored even if backtick-wrapped
# (e.g. random `created_at` references in prose). This keeps the
# extracted symbol set high-precision rather than high-recall.
KRONROE_SYMBOL_ALLOWLIST: set[str] = {
    # Core types
    "TemporalGraph", "AgentMemory", "KronroeDb", "Fact", "FactId",
    "FactIdParseError", "Value", "KronroeError", "KronroeTimestamp",
    # Hybrid + temporal
    "HybridSearchParams", "TemporalIntent", "TemporalOperator",
    # Contradiction model
    "Contradiction", "PredicateCardinality", "ConflictPolicy",
    # Uncertainty model
    "PredicateVolatility", "SourceWeight", "EffectiveConfidence",
    # AgentMemory ergonomics
    "AssertParams", "RecallOptions", "RecallScore",
    "ConfidenceFilterMode",
    # Error infrastructure
    "ErrorCode", "ErrorContext", "OptionContext",
    # Value variants — useful for symbol queries on graph edges
    "Text", "Number", "Boolean", "Entity",
    # KronroeError variants — appear bare in docs prose ("returns NotFound")
    # so the symbol resolver can land on the right page when an agent
    # asks about a specific error mode.
    "NotFound", "Storage", "Serialization", "InvalidFactId",
    "InvalidEmbedding", "ContradictionRejected", "SchemaMismatch",
    # TemporalIntent variants (backticked everywhere in agent-memory docs)
    "Timeless", "CurrentState", "HistoricalPoint", "HistoricalInterval",
    # TemporalOperator variants
    "Current", "AsOf", "Before", "By", "During", "After", "Unknown",
    # PredicateCardinality variants
    "Singleton", "MultiValued",
    # ConflictPolicy variants
    "Allow", "Warn", "Reject",
    # ConfidenceFilterMode variants
    "Base", "Effective",
    # TemporalGraph methods (the most commonly cross-referenced)
    "open", "open_in_memory", "assert_fact",
    "assert_fact_with_confidence", "assert_fact_with_source",
    "assert_fact_with_embedding", "assert_fact_idempotent",
    "assert_fact_checked", "current_facts", "facts_at",
    "all_facts_about", "fact_by_id", "correct_fact",
    "invalidate_fact", "search", "search_by_vector",
    "search_hybrid",
    # AgentMemory methods
    "remember", "recall", "recall_scored", "recall_with_options",
    "assemble_context", "assert_with_confidence",
    "assert_with_source", "facts_about",
    # MCP tools (also section headings in api/mcp-tools.md)
    "what_changed", "memory_health", "recall_for_task",
}


# Two-stage extraction: first find every backtick-delimited segment
# (single-line, since triple-backtick fences span multiple lines and
# would be matched by their fences instead), then pull every
# identifier substring within. This handles the cases the simpler
# `\`(\w+)\`` regex misses:
#
#   `Value::Entity("acme-corp")`    → captures Value AND Entity
#   `ConfidenceFilterMode::Effective` → captures both
#   `assert_fact("a", "b")`         → captures assert_fact (the args
#                                      are filtered out by the allowlist)
#
# Found during the Phase 3b.1 audit — the simpler regex silently
# missed every `Foo::Bar`-style symbol reference in our docs.
_BACKTICKED_RE = re.compile(r"`([^`\n]+)`")
_IDENT_RE = re.compile(r"[A-Za-z_][A-Za-z0-9_]*")


def extract_symbols(body: str) -> list[str]:
    """Find all Kronroe API symbols mentioned in a section body.

    Two-stage scan: find every backtick-delimited segment, then
    extract every identifier within that intersects with our curated
    allowlist. De-duplicated and stable-ordered (insertion order) so
    the output is deterministic and `corpus.json` doesn't churn
    between builds.
    """
    seen: dict[str, None] = {}
    for backticked in _BACKTICKED_RE.findall(body):
        for token in _IDENT_RE.findall(backticked):
            if token in KRONROE_SYMBOL_ALLOWLIST and token not in seen:
                seen[token] = None
    return list(seen)


def embed_section_bodies(bodies: list[str]) -> list[list[float]]:
    """Compute embeddings for a list of section bodies using
    `fastembed-python` with the same model the Rust runtime uses
    (`sentence-transformers/all-MiniLM-L6-v2`, 384-dim).

    Imported lazily so that running `build-docs.py` without the
    `--corpus` flag doesn't require `fastembed` to be installed.
    The Rust side's `fastembed-rs` and Python's `fastembed` use the
    same upstream ONNX model files, so embeddings are essentially
    bit-identical between sides — the dot-product cosine similarity
    won't drift from build-time to query-time.

    Returns float32 lists rather than numpy arrays so JSON
    serialisation is straightforward and the output file size is
    half what float64 would produce.
    """
    # Lazy import — keeps default builds free of the ML stack.
    from fastembed import TextEmbedding

    model = TextEmbedding(model_name="sentence-transformers/all-MiniLM-L6-v2")
    raw = list(model.embed(bodies))  # numpy arrays, dtype=float64

    out: list[list[float]] = []
    for v in raw:
        # Cast to float32 (4 bytes/value vs 8) before listification.
        out.append([float(x) for x in v.astype("float32")])
    return out


def render_corpus(docs_flat: list[Doc]) -> dict[str, Any]:
    """Build the corpus.json payload — the contract between this
    script and the `kronroe-docs-api` runtime.

    Schema:
        {
          "build_id":  ISO8601 UTC timestamp,
          "model":     model name (frozen for the lifetime of a build),
          "dim":       embedding dimensionality,
          "sections":  [
            { id, doc_path, doc_url, doc_title, category,
              heading, anchor, body, symbols, embedding },
            ...
          ]
        }

    Phase 3b.2 turns each section into a small set of Kronroe facts;
    `embedding` becomes the vector for `assert_fact_with_embedding`,
    and `symbols` become graph edges via `Value::Entity(name)`.
    """
    import datetime

    all_sections: list[Section] = []
    for doc in docs_flat:
        all_sections.extend(split_doc_into_sections(doc))

    for section in all_sections:
        section.symbols = extract_symbols(section.body)

    embeddings = embed_section_bodies([s.body for s in all_sections])
    if not embeddings:
        raise RuntimeError("embedder returned no vectors")
    dim = len(embeddings[0])

    return {
        "build_id": datetime.datetime.now(datetime.timezone.utc)
        .isoformat(timespec="seconds")
        .replace("+00:00", "Z"),
        "model": "sentence-transformers/all-MiniLM-L6-v2",
        "dim": dim,
        "sections": [
            {
                "id": s.id,
                "doc_path": s.doc_path,
                "doc_url": s.doc_url,
                "doc_title": s.doc_title,
                "category": s.category,
                "heading": s.heading,
                "anchor": s.anchor,
                "body": s.body,
                "symbols": s.symbols,
                "embedding": e,
            }
            for s, e in zip(all_sections, embeddings)
        ],
    }


# ─── Sidebar ──────────────────────────────────────────────────

# Order in which categories appear in the sidebar.
CATEGORY_ORDER = ["getting-started", "concepts", "api"]


def organise_by_category(docs: list[Doc]) -> dict[str, list[Doc]]:
    """Group docs by category and apply the deterministic CATEGORY_ORDER."""
    by_cat: dict[str, list[Doc]] = {}
    for doc in docs:
        by_cat.setdefault(doc.category_slug, []).append(doc)

    ordered: dict[str, list[Doc]] = {}
    for cat in CATEGORY_ORDER:
        if cat in by_cat:
            ordered[cat] = by_cat[cat]
    # Append any uncategorised docs at the end (e.g. site/docs/foo.md).
    for cat, items in by_cat.items():
        if cat not in ordered:
            ordered[cat] = items
    return ordered


def render_sidebar(by_cat: dict[str, list[Doc]], current: Doc) -> str:
    """Produce the full sidebar HTML, with the current page highlighted."""
    sections: list[str] = []
    for cat_slug, docs_in_cat in by_cat.items():
        cat_title = category_title_from_slug(cat_slug) if cat_slug else "Other"
        items: list[str] = []
        for doc in docs_in_cat:
            classes = "kr-sidebar-link"
            if doc.rel_path == current.rel_path:
                classes += " is-current"
            items.append(
                f'<li><a href="{doc.url}" class="{classes}">{doc.title}</a></li>'
            )
        sections.append(
            f'<div class="kr-sidebar-section">'
            f'<h3 class="kr-sidebar-cat">{cat_title}</h3>'
            f'<ul class="kr-sidebar-list">{"".join(items)}</ul>'
            f"</div>"
        )
    return '<nav class="kr-sidebar" aria-label="Docs navigation">' + "".join(sections) + "</nav>"


# ─── On-page TOC ──────────────────────────────────────────────

def render_toc(headings: list[Heading]) -> str:
    """Mini table-of-contents for the current page (H2 + H3 only)."""
    if not headings:
        return ""
    items: list[str] = []
    for h in headings:
        cls = "kr-toc-h2" if h.level == 2 else "kr-toc-h3"
        items.append(
            f'<li class="{cls}"><a href="#{h.anchor}">{h.text}</a></li>'
        )
    return (
        '<aside class="kr-toc" aria-label="On this page">'
        '<h4 class="kr-toc-title">On this page</h4>'
        f'<ul class="kr-toc-list">{"".join(items)}</ul>'
        "</aside>"
    )


# ─── Agent-readable formats (Phase 2) ─────────────────────────
#
# Three outputs designed for LLMs and AI agents — emitted alongside
# the human-facing HTML so the same canonical URLs can serve both.
#
#   * llms.txt  — site root index, per llmstxt.org spec. Title +
#     description + grouped list of doc URLs (pointing at the .md
#     companion files for clean ingestion).
#
#   * llms-full.txt — site root concatenation of all docs as plain
#     markdown for LLMs that want to ingest everything in one fetch.
#
#   * /docs/<path>/index.md — companion file alongside every rendered
#     index.html. Lets agents fetch the same URL with `.md` appended
#     and get the source markdown without HTML chrome.
#
# Phase 3 (separate plan) adds a structured query API + MCP server
# on top of these primitives.

LLMSTXT_DESCRIPTION = (
    "Kronroe is an embedded bi-temporal property graph database for AI "
    "agent memory and mobile/edge applications. It treats temporal facts "
    "as a first-class engine primitive — every fact carries four "
    "timestamps tracking both real-world validity and database "
    "transaction time. Runs on-device with no server, no cloud, no "
    "data leaving the user's machine. Ships as Rust crate, Python "
    "package, iOS framework, Android library, WASM bundle, and MCP "
    "server, all from one codebase."
)


def render_llms_txt(by_cat: dict[str, list[Doc]]) -> str:
    """Generate the root /llms.txt file per llmstxt.org spec.

    Format: `# Title` then `> Description` blockquote, then sections
    (`## Category`) with bullet links to each doc's `.md` companion.
    Pointing at .md (not .html) gives LLM crawlers clean markdown
    they don't have to strip HTML from.
    """
    lines = [
        "# Kronroe",
        "",
        f"> {LLMSTXT_DESCRIPTION}",
        "",
        "## Site links",
        "",
        "- [Homepage](https://kronroe.dev/): What Kronroe is and why it exists",
        "- [About](https://kronroe.dev/about/): One-person engine, built in the open",
        "- [Blog](https://kronroe.dev/blog/): Build notes, technical decisions",
        "- [Pricing](https://kronroe.dev/pricing/): AGPL-3.0 + commercial licence",
        "- [GitHub](https://github.com/kronroe/kronroe): Source code",
        "",
    ]

    for cat_slug, docs_in_cat in by_cat.items():
        cat_title = category_title_from_slug(cat_slug) if cat_slug else "Other"
        lines.append(f"## {cat_title}")
        lines.append("")
        for doc in docs_in_cat:
            md_url = f"https://kronroe.dev{doc.url}index.md"
            summary = doc.description or ""
            if len(summary) > 160:
                summary = summary[:157].rstrip() + "..."
            lines.append(f"- [{doc.title}]({md_url}): {summary}")
        lines.append("")

    return "\n".join(lines).rstrip() + "\n"


def render_llms_full_txt(docs_flat: list[Doc]) -> str:
    """Generate /llms-full.txt — every doc concatenated as plain markdown.

    Each doc separated by `\\n---\\n` with a header noting the URL,
    category, and title. LLMs that want to ingest the whole docs
    corpus in one fetch can grab this file.
    """
    parts: list[str] = []
    parts.append("# Kronroe — full documentation\n")
    parts.append(
        "Concatenated source markdown of every doc page on kronroe.dev. "
        "For programmatic / LLM ingestion. The canonical home for each "
        "doc is at `https://kronroe.dev/docs/<...>/`. The plain markdown "
        "for any single doc is also available at "
        "`https://kronroe.dev/docs/<...>/index.md`.\n"
    )

    for doc in docs_flat:
        parts.append("\n---\n")
        parts.append(
            f"## {doc.category_title} → {doc.title}\n"
            f"\n"
            f"URL: https://kronroe.dev{doc.url}\n"
            f"Markdown: https://kronroe.dev{doc.url}index.md\n"
            f"\n"
        )
        # Trim the original H1 from each doc's markdown to avoid duplication
        # — the line above already renders title + URL.
        body = re.sub(r"^# .*?\n+", "", doc.body_md, count=1, flags=re.MULTILINE)
        parts.append(body.rstrip() + "\n")

    return "".join(parts).strip() + "\n"


def yaml_quote(value: str) -> str:
    """Wrap a string in YAML double-quotes with `"` and `\\` escaped.

    Required for any frontmatter value that might contain a colon
    (`Quick Start: Rust` is the canonical case in our corpus), `#`,
    `&`, `*`, `!`, `|`, `>`, leading whitespace, or other YAML-special
    characters. Quoting unconditionally is safer than trying to detect
    "is this string ambiguous to a YAML parser?" — that's a long list
    that grows over time as new fields get added.

    YAML's double-quoted form recognises `\\"` and `\\\\` escape
    sequences; we escape both to preserve the original string verbatim.
    """
    escaped = value.replace("\\", "\\\\").replace('"', '\\"')
    return f'"{escaped}"'


def render_doc_md_companion(doc: Doc) -> str:
    """Return the markdown bytes to write at /docs/<path>/index.md.

    Includes a small frontmatter-style preamble identifying the doc,
    then the original markdown source unchanged. Frontmatter helps
    agents parse without having to infer metadata from the body.

    All values are YAML-quoted because doc titles routinely contain
    characters that are YAML-special (notably `:` in titles like
    "Quick Start: Rust") — unquoted, those produce invalid YAML that
    parsers either reject or misinterpret.
    """
    preamble = (
        f"---\n"
        f"title: {yaml_quote(doc.title)}\n"
        f"category: {yaml_quote(doc.category_title)}\n"
        f"url: {yaml_quote(f'https://kronroe.dev{doc.url}')}\n"
        f"format: {yaml_quote('markdown')}\n"
        f"---\n\n"
    )
    return preamble + doc.body_md


def render_jsonld(doc: Doc) -> str:
    """Build a TechArticle JSON-LD block for a doc's <head>.

    Helps both Google's AI Overviews and LLM crawlers correctly
    classify the page. Schema.org `TechArticle` is the right type for
    technical documentation.
    """
    payload = {
        "@context": "https://schema.org",
        "@type": "TechArticle",
        "headline": doc.title,
        "description": doc.description,
        "url": f"https://kronroe.dev{doc.url}",
        "inLanguage": "en",
        "isPartOf": {
            "@type": "TechArticle",
            "name": "Kronroe Documentation",
            "url": "https://kronroe.dev/docs/",
        },
        "author": {
            "@type": "Person",
            "name": "Rebekah Cole",
            "url": "https://kronroe.dev/about/",
        },
        "publisher": {
            "@type": "Organization",
            "name": "Kronroe",
            "url": "https://kronroe.dev/",
            "logo": {
                "@type": "ImageObject",
                "url": "https://kronroe.dev/og-image.png",
            },
        },
        "articleSection": doc.category_title or "Documentation",
        "keywords": [h.text for h in doc.headings] or [doc.title],
    }
    return (
        '<script type="application/ld+json">'
        + json.dumps(payload, indent=2)
        + "</script>"
    )


# ─── Prev / Next ──────────────────────────────────────────────

def prev_next(docs_flat: list[Doc], current: Doc) -> tuple[Doc | None, Doc | None]:
    """Find sibling docs in the flat ordered list."""
    try:
        i = next(idx for idx, d in enumerate(docs_flat) if d.rel_path == current.rel_path)
    except StopIteration:
        return None, None
    prev = docs_flat[i - 1] if i > 0 else None
    nxt = docs_flat[i + 1] if i < len(docs_flat) - 1 else None
    return prev, nxt


def render_prev_next(prev: Doc | None, nxt: Doc | None) -> str:
    parts: list[str] = []
    if prev is not None:
        parts.append(
            f'<a class="kr-pagenav kr-pagenav-prev" href="{prev.url}">'
            f'<span class="kr-pagenav-label">&larr; Previous</span>'
            f'<span class="kr-pagenav-title">{prev.title}</span></a>'
        )
    else:
        parts.append('<span class="kr-pagenav-spacer"></span>')

    if nxt is not None:
        parts.append(
            f'<a class="kr-pagenav kr-pagenav-next" href="{nxt.url}">'
            f'<span class="kr-pagenav-label">Next &rarr;</span>'
            f'<span class="kr-pagenav-title">{nxt.title}</span></a>'
        )
    else:
        parts.append('<span class="kr-pagenav-spacer"></span>')

    return f'<nav class="kr-pagenav-row" aria-label="Page navigation">{"".join(parts)}</nav>'


# ─── Search index ─────────────────────────────────────────────

def build_search_index(docs: list[Doc]) -> list[dict[str, Any]]:
    """Lightweight search index for client-side keyword matching.

    Phase 1: keyword/substring match over titles, categories, headings,
    and a stripped-text excerpt of the body. Phase 3 will replace this
    with semantic search via Kronroe + precomputed embeddings.
    """
    entries = []
    for doc in docs:
        body_text = re.sub(r"<[^>]+>", " ", doc.body_html)
        body_text = re.sub(r"\s+", " ", body_text).strip()[:1500]
        entries.append({
            "url": doc.url,
            "title": doc.title,
            "category": doc.category_title,
            "description": doc.description,
            "headings": [h.text for h in doc.headings],
            "body": body_text,
        })
    return entries


# ─── Page template ────────────────────────────────────────────

def render_page(
    doc: Doc,
    sidebar: str,
    toc: str,
    pagenav: str,
) -> str:
    """Compose the full HTML page from the template + per-doc data."""
    return PAGE_TEMPLATE.format(
        title=html_escape(doc.title),
        description=html_escape(doc.description),
        url=doc.url,
        category=html_escape(doc.category_title or "Docs"),
        md_url=f"{doc.url}index.md",
        sidebar=sidebar,
        body=doc.body_html,
        toc=toc,
        pagenav=pagenav,
        jsonld=render_jsonld(doc),
    )


def html_escape(s: str) -> str:
    return (
        s.replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
        .replace('"', "&quot;")
    )


# Pull the long template string out of the function for readability.
PAGE_TEMPLATE = """<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8"/>
<meta name="viewport" content="width=device-width, initial-scale=1.0"/>
<script>/* Redirect Firebase default hostnames to canonical kronroe.dev */
(function(){{var h=location.hostname;if(h==='kronroe.web.app'||h==='kronroe.firebaseapp.com'){{location.replace('https://kronroe.dev'+location.pathname+location.search+location.hash);}}}})();</script>
<title>{title} — Kronroe Docs</title>
<meta name="description" content="{description}"/>
<meta property="og:title" content="{title} — Kronroe Docs"/>
<meta property="og:description" content="{description}"/>
<meta property="og:type" content="article"/>
<meta property="og:url" content="https://kronroe.dev{url}"/>
<meta property="og:image" content="https://kronroe.dev/og-image.png"/>
<meta name="twitter:card" content="summary_large_image"/>
<meta name="twitter:image" content="https://kronroe.dev/og-image.png"/>
<link rel="canonical" href="https://kronroe.dev{url}"/>
<link rel="alternate" type="text/markdown" href="{md_url}" title="Markdown source"/>
<link rel="icon" type="image/svg+xml" href="/favicon.svg"/>
<link rel="stylesheet" href="/docs/_assets/docs.css"/>
<script defer src="/js/analytics-consent.js"></script>
<script defer src="/docs/_assets/docs.js"></script>
{jsonld}
</head>
<body class="kr-docs-body">

<header class="kr-docs-topbar">
  <div class="kr-docs-topbar-inner">
    <a href="/" class="kr-docs-brand">
      <span class="kr-docs-brand-mark"></span>
      <span class="kr-docs-brand-text">Kronroe</span>
    </a>
    <nav class="kr-docs-topnav" aria-label="Site sections">
      <a href="/docs/" class="is-active">Docs</a>
      <a href="/blog/">Blog</a>
      <a href="/about/">About</a>
      <a href="/pricing/">Pricing</a>
      <a href="https://github.com/kronroe/kronroe">GitHub</a>
    </nav>
    <button type="button" class="kr-docs-search-trigger" aria-label="Search docs">
      <span class="kr-docs-search-text">Search docs</span>
      <kbd class="kr-docs-search-kbd">⌘K</kbd>
    </button>
    <button type="button" class="kr-docs-theme-toggle" aria-label="Toggle dark mode">
      <span class="kr-docs-theme-icon" aria-hidden="true"></span>
    </button>
    <button type="button" class="kr-docs-burger" aria-label="Open navigation">
      <span></span><span></span><span></span>
    </button>
  </div>
  <div class="kr-docs-stripe" aria-hidden="true"></div>
</header>

<div class="kr-docs-shell">
  {sidebar}

  <main class="kr-docs-main">
    <p class="kr-docs-eyebrow">{category}</p>
    <h1 class="kr-docs-h1">{title}</h1>
    <article class="kr-docs-prose">
      {body}
    </article>
    {pagenav}
  </main>

  {toc}
</div>

<dialog class="kr-docs-search-dialog" aria-label="Search docs">
  <input type="search" class="kr-docs-search-input"
         placeholder="Search docs..." aria-label="Search query"/>
  <div class="kr-docs-search-results" aria-live="polite"></div>
  <button type="button" class="kr-docs-search-close" aria-label="Close search">Esc</button>
</dialog>

<footer class="kr-docs-footer">
  <p>
    &copy; 2026 Rebekah Cole &middot;
    <a href="/">Home</a> &middot;
    <a href="/blog/">Blog</a> &middot;
    <a href="/about/">About</a> &middot;
    <a href="/privacy/">Privacy</a> &middot;
    <a href="https://github.com/kronroe/kronroe">GitHub</a>
  </p>
</footer>

</body>
</html>
"""


# ─── Main build ───────────────────────────────────────────────

def build(check: bool = False, corpus: bool = False) -> int:
    """Render all markdown to HTML. Returns exit code (0 ok, 1 drift).

    If `corpus=True`, additionally generates `corpus.json` with
    per-section embeddings. This is the Phase 3b.1 output consumed
    by `kronroe-docs-api`. Skipped by default because:

      * It requires `fastembed-python` (a heavy ML dependency).
      * It downloads a ~80MB ONNX model on first run.
      * CI doesn't need the corpus — only the deploy workflow does.

    Combine flags as needed:
      build()                               # HTML + .md + llms.txt
      build(corpus=True)                    # the above + corpus.json
      build(check=True)                     # drift-check the HTML
      build(check=True, corpus=True)        # drift-check including corpus

    Drift checks on corpus are intentionally skipped — embedding
    output is non-deterministic at the float-bit level across
    different runtimes, so a strict equality check would false-
    positive. The corpus is regenerated on every deploy instead.
    """
    if not DOCS_SRC.is_dir():
        print(f"error: {DOCS_SRC.relative_to(ROOT)} not found", file=sys.stderr)
        return 1

    md_files = find_markdown_files(DOCS_SRC)
    if not md_files:
        print(f"warning: no markdown files under {DOCS_SRC.relative_to(ROOT)}", file=sys.stderr)
        return 0

    md = make_markdown_renderer()
    docs = [parse_doc(p, md) for p in md_files]

    # Flat ordered list for prev/next navigation across categories.
    by_cat = organise_by_category(docs)
    docs_flat: list[Doc] = []
    for cat in by_cat.values():
        docs_flat.extend(cat)

    # Render every page (HTML + .md companion).
    pages: dict[Path, str] = {}
    for doc in docs:
        sidebar = render_sidebar(by_cat, doc)
        toc = render_toc(doc.headings)
        prev, nxt = prev_next(docs_flat, doc)
        pagenav = render_prev_next(prev, nxt)
        html = render_page(doc, sidebar, toc, pagenav)
        out_path = OUTPUT / doc.rel_path / "index.html"
        pages[out_path] = html

        # Phase 2: companion markdown file at /docs/<path>/index.md
        # served alongside the HTML so agents can fetch raw source.
        md_path = OUTPUT / doc.rel_path / "index.md"
        pages[md_path] = render_doc_md_companion(doc)

    # Search index (Phase 1: client-side keyword scoring).
    search_index = build_search_index(docs)
    search_index_path = OUTPUT / "_assets" / "search.json"
    search_index_text = json.dumps({"docs": search_index}, indent=2)

    # Phase 2: agent-readable site-root files.
    # These get copied to site/dist/ root by the deploy workflow,
    # not /docs/. The build emits them at OUTPUT / "_root" / ... so
    # the deploy step knows what to lift to the site root.
    llms_txt_path = OUTPUT / "_root" / "llms.txt"
    llms_full_txt_path = OUTPUT / "_root" / "llms-full.txt"
    llms_txt_content = render_llms_txt(by_cat)
    llms_full_txt_content = render_llms_full_txt(docs_flat)

    # Files we need to write (or compare against in --check mode):
    # - Per-doc HTML + .md companions (already collected in `pages`)
    # - Search index (search_index_path)
    # - Site-root agent files (llms.txt + llms-full.txt) — written under
    #   OUTPUT/_root/ so the deploy step can lift them to site/dist/.
    extra_outputs = [
        (search_index_path, search_index_text),
        (llms_txt_path, llms_txt_content),
        (llms_full_txt_path, llms_full_txt_content),
    ]

    if check:
        drift = 0
        all_files = list(pages.items()) + extra_outputs
        for path, content in all_files:
            existing = path.read_text(encoding="utf-8") if path.exists() else ""
            if existing != content:
                print(
                    f"drift: {path.relative_to(ROOT)} would change",
                    file=sys.stderr,
                )
                drift += 1
        if drift:
            print(
                f"error: {drift} file(s) out of date — "
                "run: site/scripts/.venv/bin/python site/scripts/build-docs.py",
                file=sys.stderr,
            )
            return 1
        print(
            f"ok: {len(pages)} page(s) + search index + agent files up to date"
        )
        return 0

    # Fresh build: clear output dir, then write everything.
    if OUTPUT.exists():
        shutil.rmtree(OUTPUT)
    OUTPUT.mkdir(parents=True)

    for path, content in pages.items():
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding="utf-8")

    # Copy static assets (CSS, JS) into _assets/.
    assets_src = ROOT / "site" / "scripts" / "docs-assets"
    assets_dst = OUTPUT / "_assets"
    assets_dst.mkdir(parents=True, exist_ok=True)
    if assets_src.is_dir():
        for asset in assets_src.iterdir():
            if asset.is_file():
                shutil.copy2(asset, assets_dst / asset.name)

    # Write search index.
    search_index_path.write_text(search_index_text, encoding="utf-8")

    # Write Phase 2 agent-readable site-root files.
    llms_txt_path.parent.mkdir(parents=True, exist_ok=True)
    llms_txt_path.write_text(llms_txt_content, encoding="utf-8")
    llms_full_txt_path.write_text(llms_full_txt_content, encoding="utf-8")

    # Phase 3b.1: per-section embedding corpus consumed by
    # `kronroe-docs-api`. Lives at /docs/corpus.json on production
    # (also publicly accessible to any agent that wants the raw
    # embeddings without going through the API).
    corpus_section_count = 0
    if corpus:
        corpus_payload = render_corpus(docs_flat)
        corpus_section_count = len(corpus_payload["sections"])
        corpus_path = OUTPUT / "corpus.json"
        # `separators=(",", ":")` shaves ~25% off the file size by
        # dropping pretty-printing whitespace. The file is for
        # machine consumption, not human reading — humans should
        # use the API endpoints instead.
        corpus_path.write_text(
            json.dumps(corpus_payload, separators=(",", ":")),
            encoding="utf-8",
        )

    page_count = sum(1 for p in pages if p.suffix == ".html")
    md_count = sum(1 for p in pages if p.suffix == ".md")
    summary = (
        f"wrote {page_count} HTML page(s), {md_count} markdown companion(s), "
        f"+ search index + llms.txt + llms-full.txt"
    )
    if corpus:
        summary += f" + corpus.json ({corpus_section_count} sections)"
    print(f"{summary} → {OUTPUT.relative_to(ROOT)}")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n", 1)[0])
    parser.add_argument(
        "--check",
        action="store_true",
        help="Exit 1 if any page would change (CI drift detection).",
    )
    parser.add_argument(
        "--corpus",
        action="store_true",
        help=(
            "Additionally generate corpus.json with per-section embeddings "
            "(consumed by kronroe-docs-api). Requires `fastembed` + downloads "
            "a ~80MB ONNX model on first run. Off by default so CI builds "
            "stay lightweight; deploy workflows pass --corpus."
        ),
    )
    args = parser.parse_args()
    return build(check=args.check, corpus=args.corpus)


if __name__ == "__main__":
    sys.exit(main())
