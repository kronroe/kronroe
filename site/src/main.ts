// Kronroe WASM Playground
// Interfaces with the kronroe-wasm crate's WasmGraph export.

// ── WASM types ──────────────────────────────────────────────────────────────

type WasmModule = {
  WasmGraph: {
    new (): WasmGraph;
    open?: () => WasmGraph;
  };
};

type WasmGraph = {
  // Text facts
  assert_fact(subject: string, predicate: string, object: string): string;
  assert_fact_at(subject: string, predicate: string, object: string, valid_from_iso: string): string;
  // Numeric facts
  assert_number_fact(subject: string, predicate: string, value: number): string;
  assert_number_fact_at(subject: string, predicate: string, value: number, valid_from_iso: string): string;
  // Boolean facts
  assert_boolean_fact(subject: string, predicate: string, value: boolean): string;
  assert_boolean_fact_at(subject: string, predicate: string, value: boolean, valid_from_iso: string): string;
  // Entity-reference facts (graph edges)
  assert_entity_fact(subject: string, predicate: string, entity: string): string;
  assert_entity_fact_at(subject: string, predicate: string, entity: string, valid_from_iso: string): string;
  // Query
  current_facts(subject: string, predicate: string): string;       // JSON → WasmFact[]
  facts_at(subject: string, predicate: string, at_iso: string): string; // JSON → WasmFact[]
  all_facts_about(subject: string): string;                         // JSON → WasmFact[]
  invalidate_fact(fact_id: string): void;
  free(): void;
};

// Serde adjacently-tagged enum from Rust `Value`.
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

type ObjType = "Text" | "Number" | "Boolean" | "Entity";

// ── localStorage persistence ─────────────────────────────────────────────────

const LS_KEY = "kronroe_facts_v1";
const MAX_REPLAY_FACTS = 2000;
const MAX_STORED_FACTS = 5000;
const MAX_RENDER_FACTS = 500;
const MAX_FIELD_LEN = 256;

type StoredFact = {
  s: string;
  p: string;
  objType: ObjType;
  oValue: string;         // always a string — parsed on replay
  valid_from_iso: string; // ISO 8601 UTC
  fact_id: string;        // current engine ID — used to match on invalidation
};

function saveToLocalStorage(facts: StoredFact[]): void {
  try {
    localStorage.setItem(LS_KEY, JSON.stringify(facts));
  } catch {
    // quota exceeded or private-browsing restriction — silently ignore
  }
}

function loadFromLocalStorage(): StoredFact[] {
  try {
    const raw = localStorage.getItem(LS_KEY);
    return raw ? (JSON.parse(raw) as StoredFact[]) : [];
  } catch {
    return [];
  }
}

function exceedsFieldLen(value: string): boolean {
  return value.length > MAX_FIELD_LEN;
}

// ── Example data ─────────────────────────────────────────────────────────────

const EXAMPLES: [string, string, string, ObjType][] = [
  ["alice", "works_at",  "Acme",       "Entity"],
  ["alice", "role",      "engineer",   "Text"],
  ["alice", "score",     "0.95",       "Number"],
  ["bob",   "works_at",  "Acme",       "Entity"],
  ["bob",   "knows",     "alice",      "Entity"],
  ["Acme",  "industry",  "technology", "Text"],
];

// ── Helpers ───────────────────────────────────────────────────────────────────

function esc(s: string): string {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

function fmtValue(v: WasmFactObject): string {
  if (v.type === "Entity")  return `@${v.value}`;
  if (v.type === "Boolean") return v.value ? "true" : "false";
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

/** Convert a datetime-local input value ("YYYY-MM-DDTHH:mm") to UTC ISO 8601. */
function localInputToISO(val: string): string | null {
  if (!val) return null;
  const d = new Date(val);
  return isNaN(d.getTime()) ? null : d.toISOString();
}

/** Convert an ISO timestamp to a datetime-local value ("YYYY-MM-DDTHH:mm"). */
function isoToLocalInput(iso: string): string {
  const d = new Date(iso);
  if (isNaN(d.getTime())) return "";
  const local = new Date(d.getTime() - d.getTimezoneOffset() * 60_000);
  return local.toISOString().slice(0, 16);
}

function factRowHtml(f: WasmFact): string {
  const expired = f.expired_at !== null;
  const cls     = expired ? " invalidated" : "";
  return `<div class="fact-row${cls}" data-id="${esc(f.id)}">
    <span class="tag tag-s">${esc(f.subject)}</span>
    <span class="sep">·</span>
    <span class="tag tag-p">${esc(f.predicate)}</span>
    <span class="sep">→</span>
    <span class="tag tag-o">${esc(fmtValue(f.object))}</span>
    <span class="fact-time">${fmtTime(f.valid_from)}</span>
    <button class="btn-invalidate" data-id="${esc(f.id)}" title="Invalidate fact"${expired ? " disabled" : ""}>×</button>
  </div>`;
}

// ── Init ──────────────────────────────────────────────────────────────────────

async function init() {
  const loading     = document.getElementById("loading")!;
  const loadingText = loading.querySelector(".loading-label")!;

  let wasm: WasmModule;
  try {
    const wasmImport = (await import("../public/pkg/kronroe_wasm.js")) as unknown as
      WasmModule & { default?: (arg?: unknown) => Promise<void> };
    wasm = wasmImport;
    await wasmImport.default?.({
      module_or_path: new URL("../public/pkg/kronroe_wasm_bg.wasm", import.meta.url),
    });
  } catch (e) {
    loadingText.textContent = "Failed to load WASM — try refreshing.";
    console.error(e);
    return;
  }

  loading.classList.add("hidden");

  function createGraph(module: WasmModule): WasmGraph {
    if (typeof module.WasmGraph.open === "function") {
      return module.WasmGraph.open();
    }
    return new module.WasmGraph();
  }

  let graph = createGraph(wasm);

  // ── DOM refs ──────────────────────────────────────────────────────────────

  const subjectEl    = document.getElementById("subject")!    as HTMLInputElement;
  const predicateEl  = document.getElementById("predicate")!  as HTMLInputElement;
  const objectEl     = document.getElementById("object")!     as HTMLInputElement;
  const objTypeEl    = document.getElementById("obj-type")!   as HTMLSelectElement;
  const assertAtEl   = document.getElementById("assert-at")!  as HTMLInputElement;
  const assertBtn    = document.getElementById("assert-btn")!;
  const clearBtn     = document.getElementById("clear-btn")!;
  const timeDemoBtn  = document.getElementById("time-demo-btn")! as HTMLButtonElement;
  const assertStatus = document.getElementById("assert-status")!;

  const queryEntityEl = document.getElementById("query-entity")! as HTMLInputElement;
  const queryPredEl   = document.getElementById("query-pred")!   as HTMLInputElement;
  const queryAtEl     = document.getElementById("query-at")!     as HTMLInputElement;
  const queryBtn      = document.getElementById("query-btn")!;
  const showAllBtn    = document.getElementById("show-all-btn")!;
  const queryStatus   = document.getElementById("query-status")!;

  const streamBody  = document.getElementById("stream-body")!;
  const streamCount = document.getElementById("stream-count")!;
  const streamMode  = document.getElementById("stream-mode")!;
  const examplesEl  = document.getElementById("examples")!;

  // ── State ─────────────────────────────────────────────────────────────────

  let allFacts: WasmFact[]   = []; // complete history (including invalidated)
  let storedFacts: StoredFact[] = []; // mirror for localStorage
  let viewMode: "all" | "query" = "all";

  /** Facts that have not been invalidated — used for the "ALL" stream view. */
  function activeFacts(): WasmFact[] {
    return allFacts.filter(f => f.expired_at === null);
  }

  // ── Replay from localStorage ──────────────────────────────────────────────

  const persisted = loadFromLocalStorage();
  if (persisted.length > 0) {
    const replaySlice = persisted.slice(0, MAX_REPLAY_FACTS);
    if (persisted.length > MAX_REPLAY_FACTS) {
      setStatus(
        assertStatus,
        `Loaded first ${MAX_REPLAY_FACTS} persisted facts (of ${persisted.length}) to keep the playground responsive.`,
        "err"
      );
    }
    const replayed: StoredFact[] = [];
    for (const sf of replaySlice) {
      try {
        const factId    = assertIntoEngine(graph, sf.s, sf.p, sf.objType, sf.oValue, sf.valid_from_iso);
        const localFact = buildLocalFact(factId, sf.s, sf.p, sf.objType, sf.oValue, sf.valid_from_iso);
        allFacts.push(localFact);
        // Update the stored entry with the new engine-assigned ID
        replayed.push({ ...sf, fact_id: factId });
      } catch {
        // silently skip facts that fail to replay
      }
    }
    storedFacts = replayed;
    saveToLocalStorage(storedFacts); // write back updated IDs
  }

  // ── Example chips ─────────────────────────────────────────────────────────

  EXAMPLES.forEach(([s, p, o, t]) => {
    const btn = document.createElement("button");
    btn.className   = "chip";
    btn.textContent = `${s} · ${p} · ${o}`;
    btn.addEventListener("click", () => {
      subjectEl.value   = s;
      predicateEl.value = p;
      objectEl.value    = o;
      objTypeEl.value   = t;
      updatePlaceholder();
      subjectEl.focus();
    });
    examplesEl.appendChild(btn);
  });

  function runTimeTravelDemo() {
    const demoSubjectBase = "alice-demo";
    const demoSubject = `${demoSubjectBase}-${Date.now().toString(36).slice(-4)}`;
    const demoPred = "works_at";
    const demoObject = "Acme";
    const assertedAtISO = "2024-01-01T09:00:00.000Z";
    const pastQueryISO = "2024-06-01T12:00:00.000Z";
    const futureQueryISO = new Date(Date.now() + 60_000).toISOString();

    try {
      const factId = assertIntoEngine(graph, demoSubject, demoPred, "Entity", demoObject, assertedAtISO);
      const localFact = buildLocalFact(factId, demoSubject, demoPred, "Entity", demoObject, assertedAtISO);
      allFacts.push(localFact);
      storedFacts.push({
        s: demoSubject,
        p: demoPred,
        objType: "Entity",
        oValue: demoObject,
        valid_from_iso: assertedAtISO,
        fact_id: factId,
      });

      graph.invalidate_fact(factId);
      localFact.expired_at = new Date().toISOString();

      const idx = storedFacts.findIndex(sf => sf.fact_id === factId);
      if (idx !== -1) storedFacts.splice(idx, 1);
      saveToLocalStorage(storedFacts);

      const pastFacts = JSON.parse(graph.facts_at(demoSubject, demoPred, pastQueryISO)) as WasmFact[];
      const futureFacts = JSON.parse(graph.facts_at(demoSubject, demoPred, futureQueryISO)) as WasmFact[];

      queryEntityEl.value = demoSubject;
      queryPredEl.value = demoPred;
      queryAtEl.value = isoToLocalInput(pastQueryISO);

      viewMode = "query";
      renderFacts(pastFacts, `${demoSubject} · ${demoPred} @ ${fmtTime(pastQueryISO)}`);

      setStatus(
        assertStatus,
        `Demo loaded for ${demoSubjectBase} (${demoSubject}): asserted "${demoPred} · ${demoObject}" at 2024-01-01, then retracted now.`,
        "ok"
      );
      setStatus(
        queryStatus,
        `Past query: ${pastFacts.length} result. Future query: ${futureFacts.length} results. Press Query to explore more times.`,
        "ok"
      );
    } catch (e) {
      setStatus(assertStatus, `Time-travel demo failed: ${e}`, "err");
    }
  }

  timeDemoBtn.addEventListener("click", runTimeTravelDemo);

  // ── Placeholder hint ──────────────────────────────────────────────────────

  function updatePlaceholder() {
    const t = objTypeEl.value as ObjType;
    objectEl.placeholder =
      t === "Number"  ? "e.g. 0.95" :
      t === "Boolean" ? "true / false" :
      t === "Entity"  ? "entity name" :
      "value";
  }

  objTypeEl.addEventListener("change", updatePlaceholder);

  // ── Render helpers ────────────────────────────────────────────────────────

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
      const visibleFacts = facts.slice(0, MAX_RENDER_FACTS);
      streamBody.innerHTML = visibleFacts.map(factRowHtml).join("");
    }
  }

  // ── Assert helpers ────────────────────────────────────────────────────────

  function assertIntoEngine(
    g: WasmGraph,
    s: string, p: string,
    t: ObjType, oRaw: string,
    validFromISO: string | null
  ): string {
    if (t === "Number") {
      const n = parseFloat(oRaw);
      if (isNaN(n)) throw new Error(`"${oRaw}" is not a valid number`);
      return validFromISO
        ? g.assert_number_fact_at(s, p, n, validFromISO)
        : g.assert_number_fact(s, p, n);
    }
    if (t === "Boolean") {
      const b = oRaw.trim().toLowerCase() === "true";
      return validFromISO
        ? g.assert_boolean_fact_at(s, p, b, validFromISO)
        : g.assert_boolean_fact(s, p, b);
    }
    if (t === "Entity") {
      return validFromISO
        ? g.assert_entity_fact_at(s, p, oRaw, validFromISO)
        : g.assert_entity_fact(s, p, oRaw);
    }
    // Text (default)
    return validFromISO
      ? g.assert_fact_at(s, p, oRaw, validFromISO)
      : g.assert_fact(s, p, oRaw);
  }

  function buildLocalFact(
    factId: string,
    s: string, p: string,
    t: ObjType, oRaw: string,
    validFromISO: string | null
  ): WasmFact {
    const now     = new Date().toISOString();
    const vfISO   = validFromISO ?? now;
    let obj: WasmFactObject;
    if (t === "Number")  obj = { type: "Number",  value: parseFloat(oRaw) };
    else if (t === "Boolean") obj = { type: "Boolean", value: oRaw.trim().toLowerCase() === "true" };
    else if (t === "Entity")  obj = { type: "Entity",  value: oRaw };
    else                      obj = { type: "Text",    value: oRaw };
    return {
      id: factId,
      subject: s,
      predicate: p,
      object: obj,
      valid_from: vfISO,
      valid_to: null,
      recorded_at: now,
      expired_at: null,
      confidence: 1.0,
      source: null,
    };
  }

  // ── Assert ────────────────────────────────────────────────────────────────

  function assertFact() {
    const s      = subjectEl.value.trim();
    const p      = predicateEl.value.trim();
    const oRaw   = objectEl.value.trim();
    const t      = objTypeEl.value as ObjType;
    const atISO  = localInputToISO(assertAtEl.value);

    if (!s || !p || !oRaw) {
      setStatus(assertStatus, "⚠  Fill in subject, predicate, and value.", "err");
      return;
    }
    if (exceedsFieldLen(s) || exceedsFieldLen(p) || exceedsFieldLen(oRaw)) {
      setStatus(assertStatus, `⚠  Keep subject, predicate, and value under ${MAX_FIELD_LEN} characters.`, "err");
      return;
    }
    if (storedFacts.length >= MAX_STORED_FACTS) {
      setStatus(assertStatus, `⚠  Fact limit reached (${MAX_STORED_FACTS}). Clear graph to continue.`, "err");
      return;
    }

    try {
      const factId    = assertIntoEngine(graph, s, p, t, oRaw, atISO);
      const localFact = buildLocalFact(factId, s, p, t, oRaw, atISO);
      allFacts.push(localFact);

      const sf: StoredFact = { s, p, objType: t, oValue: oRaw, valid_from_iso: localFact.valid_from, fact_id: factId };
      storedFacts.push(sf);
      saveToLocalStorage(storedFacts);

      setStatus(assertStatus, `✓  ${s} · ${p} · ${oRaw}`, "ok");
      objectEl.value = "";
      objectEl.focus();

      if (viewMode === "all") {
        renderFacts(activeFacts(), "ALL");
        streamBody.scrollTop = streamBody.scrollHeight;
      }
    } catch (e) {
      setStatus(assertStatus, `Error: ${e}`, "err");
    }
  }

  assertBtn.addEventListener("click", assertFact);
  [subjectEl, predicateEl, objectEl, assertAtEl].forEach((el) =>
    el.addEventListener("keydown", (e) => { if (e.key === "Enter") assertFact(); })
  );

  // ── Invalidate (event delegation on stream-body) ──────────────────────────

  streamBody.addEventListener("click", (e) => {
    const target = e.target as HTMLElement;
    if (!target.classList.contains("btn-invalidate")) return;

    const factId = target.dataset.id;
    if (!factId) return;

    const fact = allFacts.find(f => f.id === factId);
    if (!fact || fact.expired_at !== null) return;

    try {
      graph.invalidate_fact(factId);
      fact.expired_at = new Date().toISOString();

      // Remove from storedFacts by engine ID (expired facts are not replayed on reload)
      const idx = storedFacts.findIndex(sf => sf.fact_id === factId);
      if (idx !== -1) storedFacts.splice(idx, 1);
      saveToLocalStorage(storedFacts);

      // Re-render current view
      if (viewMode === "all") {
        renderFacts(activeFacts(), "ALL");
      } else {
        // In query view, mark the row as invalidated in-place (no full re-render)
        const row = streamBody.querySelector(`.fact-row[data-id="${CSS.escape(factId)}"]`);
        if (row) {
          row.classList.add("invalidated");
          const btn = row.querySelector(".btn-invalidate") as HTMLButtonElement | null;
          if (btn) btn.disabled = true;
        }
      }
      setStatus(queryStatus, `✗  retracted: ${fact.subject} · ${fact.predicate} · ${fmtValue(fact.object)}`, "err");
    } catch (e) {
      setStatus(queryStatus, `Invalidation error: ${e}`, "err");
    }
  });

  // ── Clear ─────────────────────────────────────────────────────────────────

  clearBtn.addEventListener("click", () => {
    if (!confirm("Clear all facts from this in-browser graph? This cannot be undone.")) {
      return;
    }
    graph.free();
    graph       = createGraph(wasm);
    allFacts    = [];
    storedFacts = [];
    viewMode    = "all";

    saveToLocalStorage([]);
    setStatus(assertStatus, "Graph cleared.", "");
    setStatus(queryStatus,  "", "");
    streamMode.textContent  = "ALL";
    streamCount.textContent = "0 facts";
    showEmpty("No facts yet.<br>Assert one above to begin.");
  });

  // ── Query ─────────────────────────────────────────────────────────────────

  function doQuery() {
    const entity = queryEntityEl.value.trim();
    const pred   = queryPredEl.value.trim();
    const atISO  = localInputToISO(queryAtEl.value);

    if (!entity) {
      setStatus(queryStatus, "⚠  Enter an entity name.", "err");
      return;
    }
    if (exceedsFieldLen(entity) || exceedsFieldLen(pred)) {
      setStatus(queryStatus, `⚠  Keep query fields under ${MAX_FIELD_LEN} characters.`, "err");
      return;
    }

    try {
      let facts: WasmFact[];
      let label: string;

      if (pred && atISO) {
        // facts_at: entity + predicate + point-in-time
        facts = JSON.parse(graph.facts_at(entity, pred, atISO)) as WasmFact[];
        label = `${entity} · ${pred} @ ${fmtTime(atISO)}`;
      } else if (pred) {
        // current_facts: entity + predicate, currently valid
        facts = JSON.parse(graph.current_facts(entity, pred)) as WasmFact[];
        label = `${entity} · ${pred}`;
      } else if (atISO) {
        // entity + point-in-time — use all_facts_about + client-side temporal filter
        const allAbout = JSON.parse(graph.all_facts_about(entity)) as WasmFact[];
        const at = new Date(atISO).getTime();
        facts = allAbout.filter(f => {
          const from = new Date(f.valid_from).getTime();
          const to   = f.valid_to ? new Date(f.valid_to).getTime() : Infinity;
          return from <= at && at < to && f.expired_at === null;
        });
        label = `${entity} @ ${fmtTime(atISO)}`;
      } else {
        // all_facts_about: everything ever recorded for entity
        facts = JSON.parse(graph.all_facts_about(entity)) as WasmFact[];
        label = `all:${entity}`;
      }

      viewMode = "query";
      renderFacts(facts, label);
      const truncatedNote = facts.length > MAX_RENDER_FACTS
        ? ` Showing first ${MAX_RENDER_FACTS} results.`
        : "";
      setStatus(
        queryStatus,
        facts.length === 0
          ? `No facts found for "${label}".`
          : `${facts.length} result${facts.length !== 1 ? "s" : ""}.${truncatedNote}`,
        facts.length === 0 ? "err" : "ok"
      );
    } catch (e) {
      setStatus(queryStatus, `Error: ${e}`, "err");
    }
  }

  queryBtn.addEventListener("click", doQuery);
  queryEntityEl.addEventListener("keydown", (e) => { if (e.key === "Enter") doQuery(); });
  queryPredEl.addEventListener(  "keydown", (e) => { if (e.key === "Enter") doQuery(); });
  queryAtEl.addEventListener(    "keydown", (e) => { if (e.key === "Enter") doQuery(); });

  // ── Show all ──────────────────────────────────────────────────────────────

  showAllBtn.addEventListener("click", () => {
    viewMode = "all";
    setStatus(queryStatus, "", "");
    renderFacts(activeFacts(), "ALL");
  });

  // ── Initial render ────────────────────────────────────────────────────────

  if (allFacts.length > 0) {
    renderFacts(activeFacts(), "ALL");
  } else {
    showEmpty("No facts yet.<br>Assert one above to begin.");
    streamMode.textContent  = "ALL";
    streamCount.textContent = "0 facts";
  }
}

init();
