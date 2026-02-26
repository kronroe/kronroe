# Kronroe Typography System v1.0

> Three fonts. Three jobs. No overlap.

## The Trio

### 🎸 Virgil — The Soul
- **Role:** Wordmark, accent phrases, fact chips, hint labels, empty states, magic moments
- **Rule:** Keep to ≤6 words. Virgil is charming at a phrase, exhausting at a paragraph.
- **Weight:** Regular only (single weight font)
- **CSS variable:** `--hand`
- **Source:** Self-hosted `Virgil.woff2` (originally from Excalidraw, SIL Open Font License)
- **Fallback:** `'Segoe Print', cursive`

### 🎵 Quicksand — The Voice  
- **Role:** Everything readable — body text, UI labels, buttons, navigation, panel headers, headlines, forms, errors
- **Rule:** Minimum weight 500. Never use 300 or 400 — too wispy.
- **Weights:**
  - `500` (--w-body) → Body text, nav links, footer, input placeholders, subtitles
  - `600` (--w-label) → Buttons, panel headers, form labels, emphasis
  - `700` (--w-heading) → Page headlines, section titles, strong emphasis
- **CSS variable:** `--body`
- **Source:** Google Fonts (SIL Open Font License — free for commercial use)
- **Fallback:** `sans-serif`

### 🥁 JetBrains Mono — The Precision
- **Role:** Code snippets, timestamps, version badges, technical identifiers, CLI output
- **Rule:** Anything a developer would copy-paste goes in Mono.
- **Weights:**
  - `400` (--w-code) → Code blocks, terminal output, timestamps
  - `500` → License text, technical labels
  - `600` (--w-code-bold) → Version badges, status indicators
- **CSS variable:** `--mono`
- **Source:** Google Fonts (SIL Open Font License — free for commercial use)
- **Fallback:** `"Cascadia Code", ui-monospace, monospace`

## Element Map

| Element | Font | Weight | Variable |
|---------|------|--------|----------|
| Wordmark "Kronroe" | Virgil | regular | `--hand` |
| Page headlines (H1) | Quicksand | 700 | `--body` + `--w-heading` |
| Section titles (H2, H3) | Quicksand | 700 | `--body` + `--w-heading` |
| Hero accent ("In your browser.") | Virgil | regular | `--hand` |
| Body text / descriptions | Quicksand | 500 | `--body` + `--w-body` |
| Nav links | Quicksand | 500 | `--body` + `--w-body` |
| Panel headers (ASSERT FACT) | Quicksand | 600 | `--body` + `--w-label` |
| Buttons (Assert →, Clear) | Quicksand | 600 | `--body` + `--w-label` |
| Input placeholders | Quicksand | 500 | `--body` + `--w-body` |
| User-typed input text | Quicksand | 500 | `--body` + `--w-body` |
| Fact chips (alice · works_at) | Virgil | regular | `--hand` |
| Hint labels (valid from) | Virgil | regular | `--hand` |
| Time-travel button | Virgil | regular | `--hand` |
| Empty state messages | Virgil | regular | `--hand` |
| Code snippets | JetBrains Mono | 400 | `--mono` + `--w-code` |
| Timestamps / dates | JetBrains Mono | 400 | `--mono` + `--w-code` |
| Version badges (WASM, v0.1) | JetBrains Mono | 600 | `--mono` + `--w-code-bold` |
| License text (AGPL-3.0) | JetBrains Mono | 500 | `--mono` |
| Open source badge | Virgil | regular | `--hand` |
| Footer text | Quicksand | 500 | `--body` + `--w-body` |
| Error messages | Quicksand | 500 | `--body` + `--w-body` |
| Documentation body | Quicksand | 500 | `--body` + `--w-body` |
| CLI / terminal output | JetBrains Mono | 400 | `--mono` + `--w-code` |

## Decision Flowchart

```
Is it a headline?           → Quicksand 700
Is it code or data?         → JetBrains Mono
Is it a personality moment? → Virgil (≤6 words)
Everything else             → Quicksand 500-600
```

## Golden Rules

### ✓ Do
- Use Virgil for short personality moments — wordmark, accents, chips, hints, badges
- Use Quicksand for everything the user needs to read — body, UI, forms, buttons, headlines
- Use JetBrains Mono for anything a developer would copy-paste
- Keep Virgil to ≤6 words per usage
- Quicksand minimum weight: 500 in production
- Mix fonts within a section: Quicksand headline → Virgil accent → Quicksand body

### ✗ Don't  
- Don't write paragraphs in Virgil — charming at a sentence, exhausting at a paragraph
- Don't write code in anything except JetBrains Mono
- Don't use Virgil for error messages — handwriting feels dismissive when something's wrong
- Don't use Quicksand below weight 500 — it goes invisible
- Don't mix more than 2 fonts in a single UI element

## Context Tuning

The same three fonts, different mix per product:

| Context | Virgil | Quicksand | Mono |
|---------|--------|-----------|------|
| **Playground** | 25% — chips, hints, wordmark, time-travel | 55% — body, UI, headlines | 20% — code, data |
| **Marketing / Landing** | 15% — accents, testimonials | 70% — body, headlines | 15% — code examples |
| **Documentation** | 5% — callout tips only | 55% — body, nav | 40% — code-heavy |
| **Kindly Roe (App)** | 20% — encouragement, child-friendly | 75% — primary UI voice | 5% — minimal |

## Licensing

All three fonts are licensed under the **SIL Open Font License (OFL)**:
- ✓ Free for commercial use
- ✓ Can be modified and redistributed  
- ✓ No attribution required in UI
- ✓ Keep license file if redistributing font files

## CSS Variables Reference

```css
/* Font stacks */
--mono:    "JetBrains Mono", "Cascadia Code", ui-monospace, monospace;
--body:    "Quicksand", sans-serif;
--hand:    'Virgil', 'Segoe Print', cursive;

/* Weight scale */
--w-body:      500;   /* Quicksand body / nav / footer       */
--w-label:     600;   /* Quicksand buttons / panel headers    */
--w-heading:   700;   /* Quicksand headlines / bold emphasis   */
--w-code:      400;   /* JetBrains Mono code blocks           */
--w-code-bold: 600;   /* JetBrains Mono badges / labels       */
```
