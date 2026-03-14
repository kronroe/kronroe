# Kronroe Logo System

## The Concept

The Kronroe logo is a **three-node graph** — the simplest possible representation of what Kronroe stores. Three coloured dots connected by edges, forming a triangle.

Each node represents a part of a Kronroe fact:
- **Violet dot** (`#7C5CFC`) — the **subject/entity** (e.g. "alice")
- **Orange dot** (`#C04800`) — the **predicate/relationship** (e.g. "works_at") — sits at the apex
- **Lime dot** (`#5A8A00`) — the **value/object** (e.g. "Acme")

The edges between them are subtle (low opacity) — the nodes are the focus, but the connections are visible. The colours match exactly what users see in the playground fact cards: violet subject tags, orange predicate tags, lime value bars.

**The orange node sits at the apex** because the predicate is the keystone — it's the relationship that bridges subject and object, just as the predicate bridges entities in a graph.

## Design Principles

- **Honest:** It's literally a graph. No metaphor to decode.
- **Colourful with meaning:** Three colours, each with a semantic role.
- **Scales cleanly:** Three dots read at any size, even 16px favicon.
- **No pink/rose:** The palette is violet/orange/lime only.
- **Works on any background:** Edge opacity adjusts for dark vs light.

## File Inventory

### Primary Marks

| File | Use case | Background |
|---|---|---|
| `kronroe-mark-light.svg` | **Primary.** Nav bar on cream site, PyPI, crates.io, white surfaces | Light / transparent |
| `kronroe-mark-dark.svg` | Dark nav bar, dark mode contexts | Dark (higher edge opacity) |
| `kronroe-mark-v-shape.svg` | Same as light mark (canonical V-shape triangle) | Light / transparent |

### Contained Marks (for avatars and icons)

| File | Use case | Container colour |
|---|---|---|
| `kronroe-mark-contained-purple.svg` | GitHub org avatar, social media, marketing | Deep purple `#1E1640` |
| `kronroe-mark-contained-black.svg` | Monochrome contexts, alternative social | Black `#0d0d0d` |
| `kronroe-mark-ios.svg` | iOS app icon (super-rounded corners) | Deep purple `#1E1640` |

### Favicon

| File | Use case |
|---|---|
| `kronroe-favicon.svg` | Browser tab favicon (32×32 optimised, dark bg) |

### Alternative Arrangements

| File | Description | When to use |
|---|---|---|
| `kronroe-mark-horizontal.svg` | Three nodes in a horizontal line | Very wide/inline contexts, banners |
| `kronroe-mark-flowing.svg` | S-curve connecting three nodes | Expressive/editorial contexts, blog headers |
| `kronroe-mark-fully-connected.svg` | All three edges visible in source colours | Marketing materials emphasising interconnection |

## Colour Reference

| Name | Hex | Role in logo | Role on site |
|---|---|---|---|
| Violet | `#7C5CFC` | Subject node + edges from subject | Primary brand, entity/subject tags, CTAs |
| Orange | `#C04800` | Predicate node (apex) | Predicate tags, timestamp labels, attention |
| Lime | `#5A8A00` | Value node + edge from value | Value/object tags, success indicators |
| Deep purple | `#1E1640` | Contained mark background | — |
| Espresso | `#2E211C` | — | Body text, nav background |
| Teal | `#4DBDAD` | NOT used in logo (was in bridge version) | Reserved for future use |

## Size Guidelines

The V-shape triangle mark works at all these sizes:

| Context | Recommended size | Notes |
|---|---|---|
| README header | 80-120px wide | Full detail visible |
| crates.io / PyPI | 40-60px | Dots clearly distinct |
| Nav bar | 34-48px wide | Dots still readable |
| Favicon | 16px (use favicon SVG) | Simplified — only two edges |
| GitHub avatar | 256×256 (use contained) | Contained mark fills the square |

## For Claude Code

When deploying the logo to the site:

1. Copy `kronroe-mark-dark.svg` → `site/public/logo-v3x6-violet.svg` (this is the file referenced in index.html)
2. Copy `kronroe-favicon.svg` → `site/public/favicon.svg`
3. Copy `kronroe-mark-contained-purple.svg` → `site/public/logo-contained-dark.svg`
4. Update the `<img>` tag in `index.html` to use appropriate height (36-40px)

The logo is wider than tall (roughly 1.7:1 aspect ratio), so set height and let width auto-calculate.

## History

- **v1-v7**: Various K-marks, pillar concepts, orbit gates, chronicle seals (files in `site/public/`)
- **v8 Bridge**: Twin violet pillars with three temporal curves (violet/teal/orange) — concept was good but curves blurred at small size
- **v9 Graph Triangle** (current): Three-node graph with coloured nodes. Honest, scalable, meaningful.
