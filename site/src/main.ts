// Kronroe WASM Playground
// Loads the kronroe-wasm pkg and wires up the UI

type WasmModule = {
  KronroeGraph: new () => KronroeGraph;
};

type KronroeGraph = {
  assert_fact(subject: string, predicate: string, object: string): void;
  facts_about(entity: string): FactResult[];
  search(query: string, limit: number): FactResult[];
  free(): void;
};

type FactResult = {
  subject: string;
  predicate: string;
  value: string;
  valid_from: string;
  recorded_at: string;
};

const EXAMPLES: [string, string, string][] = [
  ["alice", "works_at", "Acme"],
  ["alice", "role", "engineer"],
  ["bob", "works_at", "Acme"],
  ["bob", "knows", "alice"],
  ["Acme", "industry", "technology"],
];

async function init() {
  const loading = document.getElementById("loading")!;

  let wasm: WasmModule;
  try {
    // pkg/ is built by wasm-pack at the repo root level, copied into site/public/pkg during build
    wasm = await import("../public/pkg/kronroe_wasm.js") as unknown as WasmModule;
    // @ts-ignore — init is the default wasm-bindgen entry point
    await (wasm as any).default?.("../public/pkg/kronroe_wasm_bg.wasm");
  } catch (e) {
    loading.querySelector(".loading-text")!.textContent =
      "Failed to load WASM — try refreshing.";
    console.error(e);
    return;
  }

  loading.classList.add("hidden");

  let graph = new wasm.KronroeGraph();

  // --- Elements ---
  const subjectEl = document.getElementById("subject") as HTMLInputElement;
  const predicateEl = document.getElementById("predicate") as HTMLInputElement;
  const objectEl = document.getElementById("object") as HTMLInputElement;
  const assertBtn = document.getElementById("assert-btn")!;
  const clearBtn = document.getElementById("clear-btn")!;
  const assertStatus = document.getElementById("assert-status")!;
  const queryEntityEl = document.getElementById("query-entity") as HTMLInputElement;
  const queryBtn = document.getElementById("query-btn")!;
  const searchTextEl = document.getElementById("search-text") as HTMLInputElement;
  const searchBtn = document.getElementById("search-btn")!;
  const output = document.getElementById("output")!;
  const examplesEl = document.querySelector(".examples")!;

  // --- Example buttons ---
  EXAMPLES.forEach(([s, p, o]) => {
    const btn = document.createElement("button");
    btn.className = "example-btn";
    btn.textContent = `${s} · ${p} · ${o}`;
    btn.addEventListener("click", () => {
      subjectEl.value = s;
      predicateEl.value = p;
      objectEl.value = o;
      subjectEl.focus();
    });
    examplesEl.appendChild(btn);
  });

  // --- Assert ---
  function assertFact() {
    const s = subjectEl.value.trim();
    const p = predicateEl.value.trim();
    const o = objectEl.value.trim();
    if (!s || !p || !o) {
      setStatus(assertStatus, "Fill in subject, predicate, and object.", "err");
      return;
    }
    try {
      graph.assert_fact(s, p, o);
      setStatus(assertStatus, `✓ Asserted: ${s} · ${p} · ${o}`, "ok");
      objectEl.value = "";
      objectEl.focus();
    } catch (e) {
      setStatus(assertStatus, `Error: ${e}`, "err");
    }
  }

  assertBtn.addEventListener("click", assertFact);

  [subjectEl, predicateEl, objectEl].forEach((el) => {
    el.addEventListener("keydown", (e) => {
      if (e.key === "Enter") assertFact();
    });
  });

  clearBtn.addEventListener("click", () => {
    graph.free();
    graph = new wasm.KronroeGraph();
    setStatus(assertStatus, "Graph cleared.", "");
    output.innerHTML = `<span style="color:var(--text-muted)">Graph cleared.</span>`;
  });

  // --- Query ---
  queryBtn.addEventListener("click", () => {
    const entity = queryEntityEl.value.trim();
    if (!entity) {
      renderOutput([], "Enter an entity name.");
      return;
    }
    try {
      const facts = graph.facts_about(entity);
      renderOutput(facts, facts.length === 0 ? `No facts found for "${entity}".` : null);
    } catch (e) {
      renderOutput([], `Error: ${e}`);
    }
  });

  queryEntityEl.addEventListener("keydown", (e) => {
    if (e.key === "Enter") queryBtn.click();
  });

  // --- Search ---
  searchBtn.addEventListener("click", () => {
    const q = searchTextEl.value.trim();
    if (!q) return;
    try {
      const facts = graph.search(q, 20);
      renderOutput(facts, facts.length === 0 ? `No results for "${q}".` : null);
    } catch (e) {
      renderOutput([], `Error: ${e}`);
    }
  });

  searchTextEl.addEventListener("keydown", (e) => {
    if (e.key === "Enter") searchBtn.click();
  });

  // --- Helpers ---
  function setStatus(el: Element, msg: string, type: "ok" | "err" | "") {
    el.textContent = msg;
    el.className = `status${type ? " " + type : ""}`;
  }

  function renderOutput(facts: FactResult[], empty: string | null) {
    if (empty !== null && facts.length === 0) {
      output.innerHTML = `<span style="color:var(--text-muted)">${empty}</span>`;
      return;
    }
    output.innerHTML = facts
      .map((f) => {
        const ts = f.valid_from ? new Date(f.valid_from).toLocaleTimeString() : "";
        return `<div class="fact-row">
          <span class="tag tag-s">${esc(f.subject)}</span>
          <span class="tag tag-p">${esc(f.predicate)}</span>
          <span class="tag tag-o">${esc(f.value)}</span>
          <span class="fact-time">${ts}</span>
        </div>`;
      })
      .join("");
  }

  function esc(s: string) {
    return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  }
}

init();
