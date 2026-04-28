"""Regression tests for `build-docs.py`'s section splitter and symbol
extractor (Phase 3b.1).

Run from the repo root:

    site/scripts/.venv/bin/python -m unittest site.scripts.test_build_docs
    # or:
    site/scripts/.venv/bin/python site/scripts/test_build_docs.py

CI runs this in the `site-build` job (.github/workflows/ci.yml) on
every PR that touches `site/**`. Adds about a second to CI; locks in
the splitter behaviour against the pathological cases that don't
appear in our real corpus today but will be encountered as docs grow.

The two production bugs these tests prevent — both found during the
Phase 3b.1 audit before merge:

  1. Symbol allowlist coverage gap. Enum variants like `Timeless`,
     `Singleton`, `Reject` were backticked in docs but missing from
     the curated allowlist, so `/api/docs/symbols/Timeless` would
     have returned empty. Fix added the missing variants.

  2. Regex missed module-path notation. `\\`Value::Entity\\`` would
     match nothing because the simple identifier regex couldn't
     traverse `::`. Fix: two-stage extraction (find every backticked
     segment, then pull every identifier within).

The 23 synthetic cases below cover splitter behaviour against fenced
`## ` lines, missing intros, unclosed fences, H3-doesn't-split, and
the symbol extractor against module paths and method calls.
"""

from __future__ import annotations

import importlib.util
import sys
import unittest
from pathlib import Path


# ─── Module loader ────────────────────────────────────────────
#
# `build-docs.py` has a hyphen in the filename, which makes it an
# invalid Python module identifier and prevents normal `import`. We
# load it via importlib instead. Registering in sys.modules BEFORE
# exec_module is required so dataclass introspection (Python 3.14+)
# can resolve fully-qualified class names — `_is_type` walks
# sys.modules to find each annotation's owning module.
_THIS = Path(__file__).resolve()
_BUILD_DOCS = _THIS.parent / "build-docs.py"

_spec = importlib.util.spec_from_file_location("build_docs", str(_BUILD_DOCS))
build_docs = importlib.util.module_from_spec(_spec)
sys.modules["build_docs"] = build_docs
_spec.loader.exec_module(build_docs)


def _doc(body_md: str, rel_path: str = "test/page", title: str = "Test"):
    """Construct a `Doc` with just the fields the splitter looks at.

    Most fields are unused by `split_doc_into_sections` but the
    dataclass requires them — defaulting to empty/sensible values
    keeps each test focused on the input that actually matters.
    """
    return build_docs.Doc(
        rel_path=rel_path,
        category_slug="test",
        category_title="Test",
        title=title,
        description="",
        body_html="",
        body_md=body_md,
        headings=[],
    )


# ─── Splitter cases ───────────────────────────────────────────

class SectionSplitterTests(unittest.TestCase):
    """Each test case describes a property the splitter must hold.

    Test names read as the rule itself (e.g.
    `test_fenced_h2_inside_code_must_not_split`) so a failure
    immediately tells you what invariant broke.
    """

    def _headings(self, body_md: str) -> list[str]:
        return [s.heading for s in build_docs.split_doc_into_sections(_doc(body_md))]

    # 1. The most important case: code blocks containing `## ` lines.
    # Without fenced-state tracking, the splitter would create a
    # ghost section every time a doc shows a Python comment that
    # happens to look like a heading.
    def test_fenced_h2_inside_code_must_not_split(self):
        body = (
            "# Title\n"
            "intro\n"
            "```\n"
            "## fake heading inside fence\n"
            "more code\n"
            "```\n"
            "## Real H2\n"
            "body\n"
        )
        self.assertEqual(self._headings(body), ["", "Real H2"])

    def test_no_h2_emits_only_intro_section(self):
        body = "# Title\njust intro\nno headings\n"
        self.assertEqual(self._headings(body), [""])

    def test_h1_immediately_followed_by_h2_emits_no_intro(self):
        body = "# Title\n## First\nbody\n"
        self.assertEqual(self._headings(body), ["First"])

    def test_empty_h2_skipped_when_two_h2s_back_to_back(self):
        body = "# Title\nintro\n## First\n## Second\nbody\n"
        self.assertEqual(self._headings(body), ["", "Second"])

    def test_h3_does_not_create_new_section(self):
        body = "# Title\nintro\n## Section\n### Subsection\nbody\n"
        self.assertEqual(self._headings(body), ["", "Section"])

    def test_h4_does_not_create_new_section(self):
        body = "# Title\n## Section\n#### Deep\nbody\n"
        self.assertEqual(self._headings(body), ["Section"])

    def test_empty_doc_emits_no_sections(self):
        self.assertEqual(self._headings(""), [])

    def test_doc_starting_with_fence_no_h1(self):
        body = "```\ncode\n```\n## H2\nbody\n"
        self.assertEqual(self._headings(body), ["", "H2"])

    def test_unclosed_fence_absorbs_rest_into_current_section(self):
        # An unclosed fence is bad markdown but the splitter must
        # degrade gracefully rather than panic. Everything after the
        # unclosed fence should remain in the current section's body.
        body = (
            "# Title\n## Section\n```\nunclosed\n## not a heading\ntext\n"
        )
        self.assertEqual(self._headings(body), ["Section"])

    def test_h1_inside_fence_preserved_not_skipped(self):
        # Python comments inside fenced code blocks shouldn't trigger
        # the H1-skip logic — that's only for the doc's own H1 line.
        body = (
            "# Title\n## Section\n```python\n# import statement\n"
            "foo()\n```\nend\n"
        )
        self.assertEqual(self._headings(body), ["Section"])

    def test_many_h2s_all_emit_in_order(self):
        body = "# T\n## A\na body\n## B\nb body\n## C\nc body\n"
        self.assertEqual(self._headings(body), ["A", "B", "C"])

    def test_heading_with_punctuation_preserved(self):
        body = "# T\n## What is Kronroe?\nbody\n"
        self.assertEqual(self._headings(body), ["What is Kronroe?"])

    def test_tilde_fences_not_supported_documented_limitation(self):
        # CommonMark also allows ~~~ fences, but our splitter only
        # tracks ``` ones. Real corpus uses no tildes, so this is
        # a documented limitation rather than a bug. If we ever
        # introduce ~~~ in docs, the splitter needs an update — and
        # this test should switch to expecting `["Real"]`.
        body = (
            "# T\n## Real\nbody\n~~~\n"
            "## inside tilde fence\n~~~\n"
        )
        self.assertEqual(
            self._headings(body),
            ["Real", "inside tilde fence"],
        )


# ─── Symbol extractor cases ───────────────────────────────────

class SymbolExtractorTests(unittest.TestCase):
    """The extractor's job is high-precision identification of
    Kronroe API symbols mentioned in section bodies. False positives
    (random `created_at` or `Vec` references in prose) are explicitly
    filtered by the curated allowlist; false negatives are the bug
    we audit against here.
    """

    def _extract(self, body: str) -> set[str]:
        return set(build_docs.extract_symbols(body))

    def test_basic_backticked_symbol(self):
        self.assertEqual(self._extract("use `TemporalGraph` for..."), {"TemporalGraph"})

    def test_non_kronroe_symbols_filtered_out_by_allowlist(self):
        self.assertEqual(self._extract("use `HashMap` and `Vec` from std"), set())

    def test_multiple_symbols_in_one_section(self):
        self.assertEqual(
            self._extract("`recall` returns `RecallScore` plus `AgentMemory`"),
            {"recall", "RecallScore", "AgentMemory"},
        )

    def test_repeated_symbol_deduplicated(self):
        self.assertEqual(self._extract("`recall` then `recall` again"), {"recall"})

    def test_temporal_intent_variant_captured(self):
        # Audit-driven addition: `Timeless`, `CurrentState`, etc.
        # were missing from the original allowlist.
        self.assertEqual(
            self._extract("pass `Timeless` to recall_with_options"),
            {"Timeless"},
        )

    def test_kronroe_error_variant_captured(self):
        self.assertEqual(
            self._extract("returns `NotFound` on missing fact"),
            {"NotFound"},
        )

    def test_module_path_extracts_both_identifiers(self):
        # Audit-driven fix: the original regex matched nothing here
        # because :: blocked the closing-backtick boundary.
        self.assertEqual(
            self._extract("use `Value::Text`"),
            {"Value", "Text"},
        )

    def test_module_path_in_function_call(self):
        self.assertEqual(
            self._extract("store `Value::Entity(\"acme\")` to create an edge"),
            {"Value", "Entity"},
        )

    def test_method_call_with_args_only_method_extracted(self):
        # `assert_fact` is in the allowlist; the string args aren't.
        self.assertEqual(
            self._extract('call `assert_fact("a", "b")` to write'),
            {"assert_fact"},
        )

    def test_non_backticked_symbol_ignored(self):
        # Bare prose mentions of API names aren't extracted —
        # backticking is the high-precision signal that the author
        # meant a code identifier, not the English word.
        self.assertEqual(
            self._extract("the TemporalGraph type — not in backticks"),
            set(),
        )


if __name__ == "__main__":
    unittest.main(verbosity=2)
