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
  limeLight: "#8BBF20",
  espresso: "#2A1D12",
  cream: "#FBF8F2",
  surface: "#FFFFFF",
  textMid: "rgba(42, 29, 18, 0.65)",
  textDim: "rgba(42, 29, 18, 0.46)",
  border: "rgba(42, 29, 18, 0.12)",
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
  // @ prefix = company/org
  if (name.startsWith("@")) return "company";

  // Check predicates for hints (as subject)
  for (const f of facts) {
    if (f.subject !== name) continue;
    const p = f.predicate;
    if (p === "works_at" || p === "job_title" || p === "role" || p === "knows" || p === "age" || p === "born_in")
      return "person";
    if (p === "industry" || p === "founded" || p === "hq" || p === "sector") return "company";
    if (p === "country" || p === "region" || p === "population" || p === "timezone") return "location";
  }

  // Check if it's referenced as a target of specific predicates
  for (const f of facts) {
    const target = f.object.type === "Entity" ? String(f.object.value) : (f.object.type === "Text" ? String(f.object.value) : null);
    if (target !== name) continue;
    if (f.predicate === "lives_in" || f.predicate === "born_in" || f.predicate === "located_in") return "location";
    if (f.predicate === "works_at" || f.predicate === "employed_by") return "company";
    if (f.predicate === "knows") return "person";
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
  const nodeNames = new Set<string>();
  const elements: ElementDefinition[] = [];

  // Count edges per node for sizing
  const edgeCount = new Map<string, number>();

  for (const f of activeFacts) {
    nodeNames.add(f.subject);
    if (f.object.type === "Entity") {
      nodeNames.add(String(f.object.value));
      edgeCount.set(f.subject, (edgeCount.get(f.subject) || 0) + 1);
      const target = String(f.object.value);
      edgeCount.set(target, (edgeCount.get(target) || 0) + 1);
    }
  }

  // Create node elements — size scales with connection count
  for (const name of nodeNames) {
    const kind = classifyEntity(name, activeFacts);
    const style = KIND_STYLES[kind];
    const connections = edgeCount.get(name) || 0;
    const propCount = activeFacts.filter(
      (f) => f.subject === name && f.object.type !== "Entity"
    ).length;
    // Base size 44, grows with connections (max ~72)
    const size = Math.min(72, 44 + connections * 7);

    elements.push({
      data: {
        id: name,
        label: name,
        kind,
        propCount,
        connections,
        bg: style.bg,
        borderColor: style.border,
        shape: style.shape,
        nodeSize: size,
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
      "border-width": 2.5,
      shape: "data(shape)" as any,
      width: "data(nodeSize)" as any,
      height: "data(nodeSize)" as any,
      "font-family": "Quicksand, system-ui, sans-serif",
      "font-size": "12px",
      "font-weight": 700,
      color: COLOURS.espresso,
      "text-valign": "bottom",
      "text-margin-y": 8,
      "text-outline-color": COLOURS.cream,
      "text-outline-width": 2.5,
      "overlay-opacity": 0,
      "transition-property": "background-color, border-color, width, height, border-width",
      "transition-duration": 250,
    } as any,
  },
  {
    selector: "node:active",
    style: {
      "overlay-opacity": 0.1,
      "overlay-color": COLOURS.violet,
    },
  },
  {
    selector: "node.highlighted",
    style: {
      "border-width": 4,
      "border-color": COLOURS.violet,
      "background-opacity": 1,
      "z-index": 10,
    },
  },
  {
    selector: "node.pulse",
    style: {
      "border-width": 5,
      "border-color": COLOURS.limeLight,
    },
  },
  {
    selector: "node.dimmed",
    style: {
      opacity: 0.25,
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
      "arrow-scale": 0.8,
      "curve-style": "bezier",
      "font-family": "JetBrains Mono, monospace",
      "font-size": "9px",
      "font-weight": 500,
      color: COLOURS.textMid,
      "text-rotation": "autorotate",
      "text-margin-y": -10,
      "text-outline-color": COLOURS.cream,
      "text-outline-width": 2,
      "overlay-opacity": 0,
      "transition-property": "line-color, target-arrow-color, width, opacity",
      "transition-duration": 250,
    } as any,
  },
  {
    selector: "edge.highlighted",
    style: {
      "line-color": COLOURS.violet,
      "target-arrow-color": COLOURS.violet,
      width: 3,
      "z-index": 10,
    },
  },
  {
    selector: "edge.dimmed",
    style: {
      opacity: 0.15,
    },
  },
];

// ── Track previous node set for pulse animation ──────────────────────────────

let previousNodeIds = new Set<string>();

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

  // Highlight connected elements on tap — dim everything else
  cy.on("tap", "node", (evt) => {
    const node = evt.target;
    const connected = node.closedNeighborhood();
    cy!.elements().addClass("dimmed").removeClass("highlighted");
    connected.removeClass("dimmed").addClass("highlighted");
  });

  // Clear highlight on background tap
  cy.on("tap", (evt) => {
    if (evt.target === cy) {
      cy!.elements().removeClass("highlighted").removeClass("dimmed");
    }
  });

  return cy;
}

export function updatePlaygroundGraph(facts: WasmFact[]): void {
  if (!cy) return;

  const elements = buildElements(facts);
  const newNodeIds = new Set(elements.filter(e => !e.data.source).map(e => e.data.id as string));

  // Batch update: remove old, add new
  cy.elements().remove();
  cy.add(elements);

  // Pulse newly added nodes
  for (const id of newNodeIds) {
    if (!previousNodeIds.has(id) && previousNodeIds.size > 0) {
      const node = cy.getElementById(id);
      node.addClass("pulse");
      setTimeout(() => node.removeClass("pulse"), 800);
    }
  }
  previousNodeIds = newNodeIds;

  // Run force-directed layout
  if (elements.length > 0) {
    cy.layout({
      name: "cose",
      animate: true,
      animationDuration: 500,
      animationEasing: "ease-out-cubic" as any,
      nodeRepulsion: () => 12000,
      idealEdgeLength: () => 140,
      edgeElasticity: () => 80,
      gravity: 0.25,
      numIter: 300,
      padding: 40,
      randomize: false,
      fit: true,
      nodeDimensionsIncludeLabels: true,
    } as any).run();
  }
}

export function highlightNode(entityName: string): void {
  if (!cy) return;
  const node = cy.getElementById(entityName);
  if (node.length) {
    const connected = node.closedNeighborhood();
    cy.elements().addClass("dimmed").removeClass("highlighted");
    connected.removeClass("dimmed").addClass("highlighted");
    cy.animate({ center: { eles: node }, zoom: 1.2 }, { duration: 400 });
  }
}

export function highlightFact(subjectName: string): void {
  if (!cy) return;
  const node = cy.getElementById(subjectName);
  if (node.length) {
    node.addClass("pulse");
    setTimeout(() => node.removeClass("pulse"), 600);
  }
}

export function clearPlaygroundGraph(): void {
  if (!cy) return;
  cy.elements().remove();
  previousNodeIds = new Set();
}

export function destroyPlaygroundGraph(): void {
  if (cy) {
    cy.destroy();
    cy = null;
    previousNodeIds = new Set();
  }
}
