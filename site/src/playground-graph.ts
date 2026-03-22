// Kronroe Playground — Cytoscape.js Knowledge Graph
// Renders facts as a live, interactive force-directed graph.
// Entity-type facts become edges; all other facts become property badges on nodes.

import cytoscape, { type Core, type ElementDefinition } from "cytoscape";

// ── Brand colours (source of truth: live site CSS variables) ──────────────────

const COLOURS = {
  violet: "#7C5CFC",
  violetLight: "#9B7EFF",
  copper: "#E87D4A",
  aqua: "#3EC9C9",
  lime: "#5A8A00",
  espresso: "#2A1D12",
  cream: "#FBF8F2",
  surface: "#FFFFFF",
  textMid: "rgba(42, 29, 18, 0.65)",
  textDim: "rgba(42, 29, 18, 0.46)",
  border: "rgba(42, 29, 18, 0.12)",
  invalidated: "rgba(42, 29, 18, 0.25)",
};

// ── Types ─────────────────────────────────────────────────────────────────────

type WasmFact = {
  id: string;
  subject: string;
  predicate: string;
  object: { type: string; value: string | number | boolean };
  valid_from: string;
  valid_to: string | null;
  recorded_at: string;
  expired_at: string | null;
  confidence: number;
  source: string | null;
};

// ── Entity classification ─────────────────────────────────────────────────────

type EntityKind = "person" | "company" | "location" | "concept" | "default";

function classifyEntity(name: string, facts: WasmFact[]): EntityKind {
  const lower = name.toLowerCase();

  // @ prefix = company/org
  if (name.startsWith("@")) return "company";

  // Check predicates for hints
  for (const f of facts) {
    if (f.subject !== name) continue;
    const p = f.predicate;
    if (p === "works_at" || p === "job_title" || p === "role" || p === "knows" || p === "age")
      return "person";
    if (p === "industry" || p === "founded" || p === "hq") return "company";
    if (p === "country" || p === "region" || p === "population") return "location";
  }

  // Check if it's referenced as a target of specific predicates
  for (const f of facts) {
    const target = f.object.type === "Entity" ? String(f.object.value) : (f.object.type === "Text" ? String(f.object.value) : null);
    if (target !== name) continue;

    if (f.predicate === "lives_in" || f.predicate === "born_in" || f.predicate === "located_in") return "location";
    if (f.predicate === "works_at" || f.predicate === "employed_by") return "company";
  }

  return "default";
}

const KIND_STYLES: Record<EntityKind, { bg: string; shape: string; border: string }> = {
  person: { bg: COLOURS.violet, shape: "ellipse", border: COLOURS.violet },
  company: { bg: COLOURS.copper, shape: "round-rectangle", border: COLOURS.copper },
  location: { bg: COLOURS.aqua, shape: "diamond", border: COLOURS.aqua },
  concept: { bg: COLOURS.lime, shape: "round-hexagon", border: COLOURS.lime },
  default: { bg: COLOURS.textMid, shape: "ellipse", border: COLOURS.textMid },
};

// ── Build graph elements from facts ───────────────────────────────────────────

function buildElements(facts: WasmFact[]): ElementDefinition[] {
  const activeFacts = facts.filter((f) => f.expired_at === null);
  const nodes = new Set<string>();
  const elements: ElementDefinition[] = [];

  // Collect all entity names (subjects + entity-type objects)
  for (const f of activeFacts) {
    nodes.add(f.subject);
    if (f.object.type === "Entity") {
      nodes.add(String(f.object.value));
    }
  }

  // Create node elements
  for (const name of nodes) {
    const kind = classifyEntity(name, activeFacts);
    const style = KIND_STYLES[kind];
    const propCount = activeFacts.filter(
      (f) => f.subject === name && f.object.type !== "Entity"
    ).length;

    elements.push({
      data: {
        id: name,
        label: name,
        kind,
        propCount,
        bg: style.bg,
        borderColor: style.border,
        shape: style.shape,
      },
    });
  }

  // Create edge elements from Entity-type facts
  for (const f of activeFacts) {
    if (f.object.type === "Entity") {
      elements.push({
        data: {
          id: f.id,
          source: f.subject,
          target: String(f.object.value),
          label: f.predicate,
          validFrom: f.valid_from,
        },
      });
    }
  }

  return elements;
}

// ── Cytoscape stylesheet ──────────────────────────────────────────────────────

const STYLESHEET: cytoscape.Stylesheet[] = [
  {
    selector: "node",
    style: {
      label: "data(label)",
      "background-color": "data(bg)" as any,
      "border-color": "data(borderColor)" as any,
      "border-width": 2,
      shape: "data(shape)" as any,
      width: 50,
      height: 50,
      "font-family": "Quicksand, system-ui, sans-serif",
      "font-size": "11px",
      "font-weight": 600,
      color: COLOURS.espresso,
      "text-valign": "bottom",
      "text-margin-y": 6,
      "text-outline-color": COLOURS.cream,
      "text-outline-width": 2,
      "overlay-opacity": 0,
      "transition-property": "background-color, border-color, width, height",
      "transition-duration": 200,
    } as any,
  },
  {
    selector: "node:active",
    style: {
      "overlay-opacity": 0.08,
      "overlay-color": COLOURS.violet,
    },
  },
  {
    selector: "node.highlighted",
    style: {
      "border-width": 3,
      "border-color": COLOURS.violet,
      width: 60,
      height: 60,
    },
  },
  {
    selector: "edge",
    style: {
      label: "data(label)",
      width: 2,
      "line-color": COLOURS.copper,
      "target-arrow-color": COLOURS.copper,
      "target-arrow-shape": "triangle",
      "curve-style": "bezier",
      "font-family": "JetBrains Mono, monospace",
      "font-size": "9px",
      "font-weight": 500,
      color: COLOURS.textMid,
      "text-rotation": "autorotate",
      "text-margin-y": -8,
      "text-outline-color": COLOURS.cream,
      "text-outline-width": 2,
      "overlay-opacity": 0,
      "transition-property": "line-color, target-arrow-color, width",
      "transition-duration": 200,
    } as any,
  },
  {
    selector: "edge.highlighted",
    style: {
      "line-color": COLOURS.violet,
      "target-arrow-color": COLOURS.violet,
      width: 3,
    },
  },
];

// ── Public API ────────────────────────────────────────────────────────────────

let cy: Core | null = null;

export function initPlaygroundGraph(containerId: string): Core {
  const container = document.getElementById(containerId);
  if (!container) throw new Error(`Graph container #${containerId} not found`);

  cy = cytoscape({
    container,
    elements: [],
    style: STYLESHEET,
    layout: { name: "preset" },
    userZoomingEnabled: true,
    userPanningEnabled: true,
    boxSelectionEnabled: false,
    autoungrabify: false,
    minZoom: 0.3,
    maxZoom: 3,
  });

  // Highlight connected elements on tap
  cy.on("tap", "node", (evt) => {
    cy!.elements().removeClass("highlighted");
    const node = evt.target;
    node.addClass("highlighted");
    node.connectedEdges().addClass("highlighted");
    node.connectedEdges().connectedNodes().addClass("highlighted");
  });

  // Clear highlight on background tap
  cy.on("tap", (evt) => {
    if (evt.target === cy) {
      cy!.elements().removeClass("highlighted");
    }
  });

  return cy;
}

export function updatePlaygroundGraph(facts: WasmFact[]): void {
  if (!cy) return;

  const elements = buildElements(facts);

  // Batch update: remove old, add new
  cy.elements().remove();
  cy.add(elements);

  // Run force-directed layout
  if (elements.length > 0) {
    cy.layout({
      name: "cose",
      animate: true,
      animationDuration: 400,
      animationEasing: "ease-out",
      nodeRepulsion: () => 8000,
      idealEdgeLength: () => 120,
      edgeElasticity: () => 100,
      gravity: 0.3,
      numIter: 200,
      padding: 30,
      randomize: false,
      fit: true,
    } as any).run();
  }
}

export function highlightNode(entityName: string): void {
  if (!cy) return;
  cy.elements().removeClass("highlighted");
  const node = cy.getElementById(entityName);
  if (node.length) {
    node.addClass("highlighted");
    node.connectedEdges().addClass("highlighted");
    node.connectedEdges().connectedNodes().addClass("highlighted");
    cy.animate({ center: { eles: node }, zoom: 1.5 }, { duration: 300 });
  }
}

export function clearPlaygroundGraph(): void {
  if (!cy) return;
  cy.elements().remove();
}

export function destroyPlaygroundGraph(): void {
  if (cy) {
    cy.destroy();
    cy = null;
  }
}
