// Kronroe WASM Playground
// Interfaces with the kronroe-wasm crate's WasmGraph export.

// ── WASM types ──────────────────────────────────────────────────────────────

// The kronroe-wasm crate exports `WasmGraph` (not `KronroeGraph`).
// Rust `#[wasm_bindgen(constructor)]` → JS `new WasmGraph()`.
// All query methods return a JSON string that must be JSON.parse()d.
// `free()` exists and releases the in-memory redb backend.
type WasmModule = {
  WasmGraph: new () => WasmGraph;
};

type WasmGraph = {
  assert_fact(subject: string, predicate: string, object: string): string; // returns fact ID
  current_facts(subject: string, predicate: string): string;               // JSON → WasmFact[]
  all_facts_about(subject: string): string;                                // JSON → WasmFact[]
  invalidate_fact(fact_id: string): void;
  free(): void; // releases the underlying redb InMemoryBackend
};

// Serde-serialised Fact from the Rust core.
// Value is an adjacently-tagged enum: { type: "Text", value: "..." }
type WasmFactObject =
  | { type: "Text";    value: string  }
  | { type: "Number";  value: number  }
  | { type: "Boolean"; value: boolean }
  | { type: "Entity";  value: string  };

type WasmFact = {
  id: string;
  subject: string;
  predicate: string;
  object: WasmFactObject;
  valid_from: string;
  valid_to: string | null;
  recorded_at: string;
  expired_at: string | null;
  confidence: number;
  source: string | null;
};

// ── Example data ────────────────────────────────────────────────────────────

const EXAMPLES: [string, string, string][] = [
  ["alice", "works_at", "Acme"],
  ["alice", "role", "engineer"],
  ["bob", "works_at", "Acme"],
  ["bob", "knows", "alice"],
  ["Acme", "industry", "technology"],
];

// ── Helpers ──────────────────────────────────────────────────────────────────

function esc(s: string): string {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

function fmtValue(v: WasmFactObject): string {
  if (v.type === "Entity") return `@${v.value}`;
  return String(v.value);
}

function fmtTime(iso: string): string {
  try {
    return new Date(iso).toLocaleTimeString([], {
      hour: "2-digit", minute: "2-digit", second: "2-digit",
    });
  } catch {
    return iso;
  }
}

function factRowHtml(f: WasmFact): string {
  return `<div class="fact-row" data-id="${esc(f.id)}">
    <span class="tag tag-s">${esc(f.subject)}</span>
    <span class="sep">·</span>
    <span class="tag tag-p">${esc(f.predicate)}</span>
    <span class="sep">→</span>
    <span class="tag tag-o">${esc(fmtValue(f.object))}</span>
    <span class="fact-time">${fmtTime(f.valid_from)}</span>
  </div>`;
}

// ── Init ────────────────────────────────────────────────────────────────────

async function init() {
  const loading     = document.getElementById("loading")!;
  const loadingText = loading.querySelector(".loading-label")!;

  // Load the WASM module built by wasm-pack --target web
  let wasm: WasmModule;
  try {
    wasm = (await import("../public/pkg/kronroe_wasm.js")) as unknown as WasmModule;
    // wasm-bindgen generates a default export that initialises the .wasm binary
    await (wasm as unknown as { default?: (path: string) => Promise<void> }).default?.(
      "../public/pkg/kronroe_wasm_bg.wasm"
    );
  } catch (e) {
    loadingText.textContent = "Failed to load WASM — try refreshing.";
    console.error(e);
    return;
  }

  loading.classList.add("hidden");

  // Open an in-memory graph (redb InMemoryBackend under the hood)
  let graph = new wasm.WasmGraph();

  // ── DOM refs ────────────────────────────────────────────────────────────

  const subjectEl    = document.getElementById("subject")!    as HTMLInputElement;
  const predicateEl  = document.getElementById("predicate")!  as HTMLInputElement;
  const objectEl     = document.getElementById("object")!     as HTMLInputElement;
  const assertBtn    = document.getElementById("assert-btn")!;
  const clearBtn     = document.getElementById("clear-btn")!;
  const assertStatus = document.getElementById("assert-status")!;

  const queryEntityEl = document.getElementById("query-entity")! as HTMLInputElement;
  const queryPredEl   = document.getElementById("query-pred")!   as HTMLInputElement;
  const queryBtn      = document.getElementById("query-btn")!;
  const showAllBtn    = document.getElementById("show-all-btn")!;
  const queryStatus   = document.getElementById("query-status")!;

  const streamBody  = document.getElementById("stream-body")!;
  const streamCount = document.getElementById("stream-count")!;
  const streamMode  = document.getElementById("stream-mode")!;
  const examplesEl  = document.getElementById("examples")!;

  // Local mirror of every asserted fact (WASM has no "list all" endpoint)
  let allFacts: WasmFact[] = [];
  // Track whether the stream is showing all facts or a query result
  let viewMode: "all" | "query" = "all";

  // ── Example chips ───────────────────────────────────────────────────────

  EXAMPLES.forEach(([s, p, o]) => {
    const btn = document.createElement("button");
    btn.className = "chip";
    btn.textContent = `${s} · ${p} · ${o}`;
    btn.addEventListener("click", () => {
      subjectEl.value   = s;
      predicateEl.value = p;
      objectEl.value    = o;
      subjectEl.focus();
    });
    examplesEl.appendChild(btn);
  });

  // ── Render helpers ──────────────────────────────────────────────────────

  function setStatus(el: Element, msg: string, kind: "ok" | "err" | "") {
    el.textContent = msg;
    el.className   = `status${kind ? " " + kind : ""}`;
  }

  function showEmpty(message: string) {
    streamBody.innerHTML = `<div class="empty"><span class="empty-glyph">◈</span>${message}</div>`;
  }

  function renderFacts(facts: WasmFact[], modeLabel: string) {
    streamMode.textContent  = modeLabel;
    streamCount.textContent = `${facts.length} fact${facts.length !== 1 ? "s" : ""}`;
    if (facts.length === 0) {
      showEmpty("No facts found.");
    } else {
      streamBody.innerHTML = facts.map(factRowHtml).join("");
    }
  }

  // ── Assert ──────────────────────────────────────────────────────────────

  function assertFact() {
    const s = subjectEl.value.trim();
    const p = predicateEl.value.trim();
    const o = objectEl.value.trim();

    if (!s || !p || !o) {
      setStatus(assertStatus, "⚠  Fill in subject, predicate, and object.", "err");
      return;
    }

    try {
      const factId = graph.assert_fact(s, p, o);
      // Build a local mirror so we can display the stream without re-querying
      const now = new Date().toISOString();
      const localFact: WasmFact = {
        id: factId,
        subject: s,
        predicate: p,
        object: { type: "Text", value: o },
        valid_from: now,
        valid_to: null,
        recorded_at: now,
        expired_at: null,
        confidence: 1.0,
        source: null,
      };
      allFacts.push(localFact);

      setStatus(assertStatus, `✓  ${s} · ${p} · ${o}`, "ok");
      objectEl.value = "";
      objectEl.focus();

      // Keep stream updated if we're in "all" mode
      if (viewMode === "all") {
        renderFacts(allFacts, "ALL");
        streamBody.scrollTop = streamBody.scrollHeight;
      }
    } catch (e) {
      setStatus(assertStatus, `Error: ${e}`, "err");
    }
  }

  assertBtn.addEventListener("click", assertFact);
  [subjectEl, predicateEl, objectEl].forEach((el) =>
    el.addEventListener("keydown", (e) => { if (e.key === "Enter") assertFact(); })
  );

  // ── Clear ───────────────────────────────────────────────────────────────

  clearBtn.addEventListener("click", () => {
    graph.free(); // release the old redb InMemoryBackend
    graph    = new wasm.WasmGraph();
    allFacts = [];
    viewMode = "all";

    setStatus(assertStatus, "Graph cleared.", "");
    setStatus(queryStatus,  "", "");
    streamMode.textContent  = "ALL";
    streamCount.textContent = "0 facts";
    showEmpty("No facts yet.<br>Assert one above to begin.");
  });

  // ── Query ───────────────────────────────────────────────────────────────

  function doQuery() {
    const entity = queryEntityEl.value.trim();
    const pred   = queryPredEl.value.trim();

    if (!entity) {
      setStatus(queryStatus, "⚠  Enter an entity name.", "err");
      return;
    }

    try {
      let facts: WasmFact[];
      let label: string;

      if (pred) {
        // current_facts returns only facts with valid_to = None for this predicate
        facts = JSON.parse(graph.current_facts(entity, pred)) as WasmFact[];
        label = `${entity} · ${pred}`;
      } else {
        // all_facts_about returns every fact (including invalidated) for this entity
        facts = JSON.parse(graph.all_facts_about(entity)) as WasmFact[];
        label = `all:${entity}`;
      }

      viewMode = "query";
      renderFacts(facts, label);
      setStatus(
        queryStatus,
        facts.length === 0
          ? `No facts found for "${label}".`
          : `${facts.length} result${facts.length !== 1 ? "s" : ""}.`,
        facts.length === 0 ? "err" : "ok"
      );
    } catch (e) {
      setStatus(queryStatus, `Error: ${e}`, "err");
    }
  }

  queryBtn.addEventListener("click", doQuery);
  queryEntityEl.addEventListener("keydown", (e) => { if (e.key === "Enter") doQuery(); });
  queryPredEl.addEventListener(  "keydown", (e) => { if (e.key === "Enter") doQuery(); });

  // ── Show all ────────────────────────────────────────────────────────────

  showAllBtn.addEventListener("click", () => {
    viewMode = "all";
    setStatus(queryStatus, "", "");
    renderFacts(allFacts, "ALL");
  });
}

init();
