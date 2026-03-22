---
layout: home

hero:
  name: Kronroe
  text: Bi-temporal graph database
  tagline: Track not just what's true — but when it became true, and when you recorded it.
  actions:
    - theme: brand
      text: Get Started
      link: /getting-started/what-is-kronroe
    - theme: alt
      text: API Reference
      link: /api/core
    - theme: alt
      text: Try the Playground
      link: https://kronroe.dev#playground

features:
  - title: Bi-temporal by design
    details: Every fact carries four timestamps — valid_from, valid_to, recorded_at, expired_at. Query any point in time. Corrections are non-destructive.
  - title: Zero server required
    details: Single .kronroe file. No Neo4j, no Docker, no network. Runs embedded inside your process or in-browser via WebAssembly.
  - title: Multi-platform
    details: Pure Rust engine compiles to iOS, Android, Python, WASM, and native. Under 6 MB per platform. All data stays on-device.
  - title: AI agent memory
    details: Give your agents persistent memory that tracks how knowledge evolves over time. Hybrid BM25 + vector recall with confidence scoring.
  - title: MCP integration
    details: Eleven tools out of the box for Claude Desktop, Cursor, and any MCP-compatible client. Install in seconds via npx or pip.
  - title: No LLM required
    details: Structural and temporal reasoning is engine-native. Zero tokens spent storing or retrieving graph facts.
---
