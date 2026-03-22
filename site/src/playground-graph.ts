// ── Kronroe Playground — Custom Canvas Knowledge Graph ─────────────────────────
// Zero external dependencies. Same physics engine as hero-graph.ts (Coulomb +
// Hooke + Verlet), adapted for 2D + dynamic data from WASM engine.
// Renders on cream background to match the playground panel.

// ── Brand colours (source of truth: live site CSS variables) ──────────────────

const C = {
  violet:      '#7C5CFC',
  violetFill:  'rgba(124,92,252,0.22)',
  copper:      '#E87D4A',
  copperFill:  'rgba(232,125,74,0.22)',
  aqua:        '#3EC9C9',
  aquaFill:    'rgba(62,201,201,0.22)',
  lime:        '#5A8A00',
  limeFill:    'rgba(90,138,0,0.22)',
  limeLight:   '#8BBF20',
  espresso:    '#2A1D12',
  cream:       '#FBF8F2',
  surface:     '#FFFFFF',
  textMid:     'rgba(42,29,18,0.65)',
  textDim:     'rgba(42,29,18,0.40)',
  border:      'rgba(42,29,18,0.12)',
};

function hexAlpha(hex: string, a: number): string {
  const r = parseInt(hex.slice(1, 3), 16);
  const g = parseInt(hex.slice(3, 5), 16);
  const b = parseInt(hex.slice(5, 7), 16);
  return `rgba(${r},${g},${b},${a})`;
}

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

type EntityKind = 'person' | 'company' | 'location' | 'concept' | 'default';

interface GNode {
  id: string;
  kind: EntityKind;
  x: number; y: number;
  vx: number; vy: number;
  r: number;
  edges: number;
  props: number;
  pulseUntil: number;
}

interface GEdge {
  id: string;
  source: string;
  target: string;
  label: string;
}

// ── Entity classification (same logic as before) ──────────────────────────────

function classifyEntity(name: string, facts: WasmFact[]): EntityKind {
  if (name.startsWith('@')) return 'company';
  for (const f of facts) {
    if (f.subject !== name) continue;
    const p = f.predicate;
    if (p === 'works_at' || p === 'job_title' || p === 'role' || p === 'knows' || p === 'age' || p === 'born_in') return 'person';
    if (p === 'industry' || p === 'founded' || p === 'hq' || p === 'sector') return 'company';
    if (p === 'country' || p === 'region' || p === 'population' || p === 'timezone') return 'location';
  }
  for (const f of facts) {
    const target = f.object.type === 'Entity' ? String(f.object.value) : f.object.type === 'Text' ? String(f.object.value) : null;
    if (target !== name) continue;
    if (f.predicate === 'lives_in' || f.predicate === 'born_in' || f.predicate === 'located_in') return 'location';
    if (f.predicate === 'works_at' || f.predicate === 'employed_by') return 'company';
    if (f.predicate === 'knows') return 'person';
  }
  return 'default';
}

const KIND_COLOURS: Record<EntityKind, { fill: string; stroke: string }> = {
  person:   { fill: C.violetFill, stroke: C.violet },
  company:  { fill: C.copperFill, stroke: C.copper },
  location: { fill: C.aquaFill,   stroke: C.aqua },
  concept:  { fill: C.limeFill,   stroke: C.lime },
  default:  { fill: 'rgba(42,29,18,0.12)', stroke: C.textMid },
};

// ── Physics constants (tuned from hero-graph.ts for 2D) ───────────────────────

const K_REPEL   = 18000;
const K_SPRING  = 0.025;
const REST_LEN  = 130;
const DAMPING   = 0.82;
const GRAVITY   = 0.012;
const WALL_K    = 0.3;
const WALL_PAD  = 40;

// ── State ─────────────────────────────────────────────────────────────────────

let nodes: GNode[] = [];
let edges: GEdge[] = [];
let nodeIdx = new Map<string, number>();
let previousNodeIds = new Set<string>();

let cnv: HTMLCanvasElement | null = null;
let ctx: CanvasRenderingContext2D | null = null;
let W = 0, H = 0;
let dpr = 1;
let animFrame = 0;
let settled = false;
let settleCount = 0;

let hovered: GNode | null = null;
let selected: GNode | null = null;
let dragging: GNode | null = null;
let dragOffX = 0, dragOffY = 0;

interface Particle { edgeIdx: number; startTs: number; }
interface FloatingLabel { text: string; x: number; y: number; startTs: number; life: number; }
let particle: Particle | null = null;
let floatingLabel: FloatingLabel | null = null;
let lastParticle = 0;
const PARTICLE_INTERVAL = 6000;
const PARTICLE_TRAVEL = 1400;

// ── Build nodes/edges from facts ──────────────────────────────────────────────

function buildGraph(facts: WasmFact[]): { newNodes: GNode[]; newEdges: GEdge[] } {
  const active = facts.filter(f => f.expired_at === null);
  const names = new Set<string>();
  const edgeCount = new Map<string, number>();
  const newEdges: GEdge[] = [];

  for (const f of active) {
    names.add(f.subject);
    if (f.object.type === 'Entity') {
      const target = String(f.object.value);
      names.add(target);
      edgeCount.set(f.subject, (edgeCount.get(f.subject) || 0) + 1);
      edgeCount.set(target, (edgeCount.get(target) || 0) + 1);
      newEdges.push({ id: f.id, source: f.subject, target, label: f.predicate });
    }
  }

  const newNodes: GNode[] = [];
  for (const name of names) {
    const kind = classifyEntity(name, active);
    const ec = edgeCount.get(name) || 0;
    const props = active.filter(f => f.subject === name && f.object.type !== 'Entity').length;
    const existing = nodes.find(n => n.id === name);
    const baseR = Math.min(28, 16 + ec * 3);

    newNodes.push({
      id: name, kind,
      x: existing?.x ?? (W / 2 + (Math.random() - 0.5) * W * 0.5),
      y: existing?.y ?? (H / 2 + (Math.random() - 0.5) * H * 0.5),
      vx: 0, vy: 0,
      r: baseR, edges: ec, props,
      pulseUntil: 0,
    });
  }

  return { newNodes, newEdges };
}

// ── Physics step (2D Coulomb + Hooke + Verlet) ────────────────────────────────

function physicsStep() {
  const n = nodes.length;
  if (n === 0) return;

  const fx = new Float32Array(n);
  const fy = new Float32Array(n);

  // Coulomb repulsion
  for (let i = 0; i < n; i++) {
    for (let j = i + 1; j < n; j++) {
      const dx = nodes[i].x - nodes[j].x;
      const dy = nodes[i].y - nodes[j].y;
      const d2 = Math.max(dx * dx + dy * dy, 50 * 50);
      const d = Math.sqrt(d2);
      const f = K_REPEL / d2;
      const ux = dx / d, uy = dy / d;
      fx[i] += f * ux; fy[i] += f * uy;
      fx[j] -= f * ux; fy[j] -= f * uy;
    }
  }

  // Hooke spring
  for (const e of edges) {
    const i = nodeIdx.get(e.source);
    const j = nodeIdx.get(e.target);
    if (i === undefined || j === undefined) continue;
    const dx = nodes[j].x - nodes[i].x;
    const dy = nodes[j].y - nodes[i].y;
    const d = Math.sqrt(dx * dx + dy * dy) + 1e-4;
    const f = K_SPRING * (d - REST_LEN);
    const ux = dx / d, uy = dy / d;
    fx[i] += f * ux; fy[i] += f * uy;
    fx[j] -= f * ux; fy[j] -= f * uy;
  }

  // Centre gravity
  const cx = W / 2, cy = H / 2;
  for (let i = 0; i < n; i++) {
    fx[i] -= GRAVITY * (nodes[i].x - cx);
    fy[i] -= GRAVITY * (nodes[i].y - cy);
  }

  // Wall repulsion
  for (let i = 0; i < n; i++) {
    const nd = nodes[i];
    if (nd.x < WALL_PAD) fx[i] += WALL_K * (WALL_PAD - nd.x);
    if (nd.x > W - WALL_PAD) fx[i] -= WALL_K * (nd.x - (W - WALL_PAD));
    if (nd.y < WALL_PAD) fy[i] += WALL_K * (WALL_PAD - nd.y);
    if (nd.y > H - WALL_PAD) fy[i] -= WALL_K * (nd.y - (H - WALL_PAD));
  }

  // Verlet integration
  let totalV = 0;
  for (let i = 0; i < n; i++) {
    if (nodes[i] === dragging) continue;
    nodes[i].vx = (nodes[i].vx + fx[i]) * DAMPING;
    nodes[i].vy = (nodes[i].vy + fy[i]) * DAMPING;
    nodes[i].x += nodes[i].vx;
    nodes[i].y += nodes[i].vy;
    totalV += Math.abs(nodes[i].vx) + Math.abs(nodes[i].vy);
  }

  if (totalV < 0.5 * n) { settleCount++; if (settleCount > 30) settled = true; }
  else { settleCount = 0; settled = false; }
}

// ── Draw shapes by entity kind ────────────────────────────────────────────────

function drawNodeShape(x: number, y: number, r: number, kind: EntityKind) {
  if (!ctx) return;
  if (kind === 'company') {
    const s = r * 1.6, cr = r * 0.3;
    ctx.beginPath();
    ctx.moveTo(x - s / 2 + cr, y - s / 2);
    ctx.arcTo(x + s / 2, y - s / 2, x + s / 2, y + s / 2, cr);
    ctx.arcTo(x + s / 2, y + s / 2, x - s / 2, y + s / 2, cr);
    ctx.arcTo(x - s / 2, y + s / 2, x - s / 2, y - s / 2, cr);
    ctx.arcTo(x - s / 2, y - s / 2, x + s / 2, y - s / 2, cr);
    ctx.closePath();
  } else if (kind === 'location') {
    const s = r * 1.2;
    ctx.beginPath();
    ctx.moveTo(x, y - s); ctx.lineTo(x + s, y); ctx.lineTo(x, y + s); ctx.lineTo(x - s, y);
    ctx.closePath();
  } else {
    ctx.beginPath(); ctx.arc(x, y, r, 0, Math.PI * 2);
  }
}

// ── Render ─────────────────────────────────────────────────────────────────────

function render(ts: number) {
  if (!ctx || !cnv) return;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);

  ctx.fillStyle = C.surface;
  ctx.fillRect(0, 0, W, H);

  // Subtle dot grid
  ctx.fillStyle = 'rgba(124,92,252,0.045)';
  for (let dx = 14; dx < W; dx += 28) {
    for (let dy = 14; dy < H; dy += 28) {
      ctx.beginPath(); ctx.arc(dx, dy, 0.8, 0, Math.PI * 2); ctx.fill();
    }
  }

  const isHL = selected !== null;
  const conn = new Set<string>();
  if (selected) {
    conn.add(selected.id);
    for (const e of edges) {
      if (e.source === selected.id) conn.add(e.target);
      if (e.target === selected.id) conn.add(e.source);
    }
  }

  // ── Edges ──
  for (const e of edges) {
    const ni = nodeIdx.get(e.source), nj = nodeIdx.get(e.target);
    if (ni === undefined || nj === undefined) continue;
    const a = nodes[ni], b = nodes[nj];
    const dim = isHL && !conn.has(a.id) && !conn.has(b.id);
    const hl = isHL && conn.has(a.id) && conn.has(b.id);

    const dx = b.x - a.x, dy = b.y - a.y;
    const dist = Math.hypot(dx, dy) + 1e-4;
    const ux = dx / dist, uy = dy / dist;
    const x1 = a.x + ux * a.r, y1 = a.y + uy * a.r;
    const x2 = b.x - ux * b.r, y2 = b.y - uy * b.r;

    ctx.save();
    ctx.strokeStyle = hl ? C.violet : dim ? hexAlpha(C.copper, 0.15) : C.copper;
    ctx.lineWidth = hl ? 2.5 : 1.8;
    ctx.globalAlpha = dim ? 0.3 : 1;
    ctx.beginPath(); ctx.moveTo(x1, y1); ctx.lineTo(x2, y2); ctx.stroke();

    // Arrow
    const al = 8, aa = Math.atan2(y2 - y1, x2 - x1);
    ctx.fillStyle = ctx.strokeStyle;
    ctx.beginPath(); ctx.moveTo(x2, y2);
    ctx.lineTo(x2 - al * Math.cos(aa - 0.35), y2 - al * Math.sin(aa - 0.35));
    ctx.lineTo(x2 - al * Math.cos(aa + 0.35), y2 - al * Math.sin(aa + 0.35));
    ctx.closePath(); ctx.fill();

    // Label
    if (!dim || hl) {
      const mx = (x1 + x2) / 2, my = (y1 + y2) / 2;
      const el = Math.hypot(x2 - x1, y2 - y1) || 1;
      const ox = -(y2 - y1) / el * 12, oy = (x2 - x1) / el * 12;
      ctx.globalAlpha = dim ? 0.25 : 0.75;
      ctx.font = '500 9px "JetBrains Mono", monospace';
      ctx.fillStyle = hl ? C.violet : C.copper;
      ctx.textAlign = 'center'; ctx.textBaseline = 'middle';
      ctx.fillText(e.label, mx + ox, my + oy);
    }
    ctx.restore();
  }

  // ── Travelling particle ──
  if (particle && edges.length > 0) {
    const t = Math.min((ts - particle.startTs) / PARTICLE_TRAVEL, 1);
    const e = edges[particle.edgeIdx];
    if (e) {
      const ni = nodeIdx.get(e.source), nj = nodeIdx.get(e.target);
      if (ni !== undefined && nj !== undefined) {
        const a = nodes[ni], b = nodes[nj];
        const px = a.x + (b.x - a.x) * t, py = a.y + (b.y - a.y) * t;
        const fade = t < 0.12 ? t / 0.12 : t > 0.88 ? (1 - t) / 0.12 : 1;
        ctx.save(); ctx.globalAlpha = fade * 0.9; ctx.fillStyle = C.copper;
        ctx.beginPath(); ctx.arc(px, py, 3, 0, Math.PI * 2); ctx.fill(); ctx.restore();
        if (t >= 1) {
          floatingLabel = { text: `recorded_at: ${new Date().toISOString().slice(0, 19)}`,
            x: b.x, y: b.y - b.r - 14, startTs: ts, life: 2200 };
          particle = null;
        }
      }
    }
  }

  // ── Floating label ──
  if (floatingLabel) {
    const t = Math.min((ts - floatingLabel.startTs) / floatingLabel.life, 1);
    if (t >= 1) { floatingLabel = null; }
    else {
      const fa = t < 0.15 ? t / 0.15 : t > 0.65 ? 1 - (t - 0.65) / 0.35 : 1;
      ctx.save(); ctx.globalAlpha = fa * 0.85;
      ctx.font = '500 8px "JetBrains Mono", monospace'; ctx.fillStyle = C.copper;
      ctx.textAlign = 'center'; ctx.textBaseline = 'middle';
      ctx.fillText(floatingLabel.text, floatingLabel.x, floatingLabel.y - t * 20);
      ctx.restore();
    }
  }

  // ── Nodes ──
  for (const nd of nodes) {
    const kc = KIND_COLOURS[nd.kind];
    const dim = isHL && !conn.has(nd.id);
    const isSel = selected === nd, isHov = hovered === nd;

    ctx.save();
    ctx.globalAlpha = dim ? 0.2 : 1;

    ctx.fillStyle = kc.fill;
    drawNodeShape(nd.x, nd.y, nd.r, nd.kind); ctx.fill();

    ctx.strokeStyle = kc.stroke;
    ctx.lineWidth = isSel ? 3 : isHov ? 2.5 : 2;
    drawNodeShape(nd.x, nd.y, nd.r, nd.kind); ctx.stroke();

    if ((isSel || isHov) && !dim) {
      ctx.strokeStyle = hexAlpha(kc.stroke, 0.3); ctx.lineWidth = 1;
      drawNodeShape(nd.x, nd.y, nd.r + 8, nd.kind); ctx.stroke();
    }

    // Pulse
    if (nd.pulseUntil > ts) {
      const cyc = ((ts % 800) / 800);
      ctx.strokeStyle = hexAlpha(C.limeLight, (1 - cyc) * 0.45); ctx.lineWidth = 2;
      ctx.beginPath(); ctx.arc(nd.x, nd.y, nd.r + 6 + cyc * 16, 0, Math.PI * 2); ctx.stroke();
    }

    // Label
    ctx.globalAlpha = dim ? 0.2 : 0.9;
    ctx.font = '600 11px "Quicksand", system-ui, sans-serif';
    ctx.fillStyle = C.espresso; ctx.textAlign = 'center'; ctx.textBaseline = 'top';
    ctx.strokeStyle = C.surface; ctx.lineWidth = 3; ctx.lineJoin = 'round';
    ctx.strokeText(nd.id, nd.x, nd.y + nd.r + 5);
    ctx.fillText(nd.id, nd.x, nd.y + nd.r + 5);

    // Property count badge
    if (nd.props > 0 && !dim) {
      const bx = nd.x + nd.r * 0.7, by = nd.y - nd.r * 0.7;
      ctx.globalAlpha = 0.85; ctx.fillStyle = C.espresso;
      ctx.beginPath(); ctx.arc(bx, by, 8, 0, Math.PI * 2); ctx.fill();
      ctx.fillStyle = '#fff'; ctx.font = '600 8px "JetBrains Mono", monospace';
      ctx.textAlign = 'center'; ctx.textBaseline = 'middle';
      ctx.fillText(String(nd.props), bx, by);
    }
    ctx.restore();
  }

  // Empty state
  if (nodes.length === 0) {
    ctx.save(); ctx.fillStyle = C.textDim;
    ctx.font = '500 13px "Quicksand", system-ui, sans-serif';
    ctx.textAlign = 'center'; ctx.textBaseline = 'middle';
    ctx.fillText('Assert a fact to see the graph', W / 2, H / 2);
    ctx.restore();
  }
}

// ── Hit testing ───────────────────────────────────────────────────────────────

function hitTest(mx: number, my: number): GNode | null {
  for (let i = nodes.length - 1; i >= 0; i--) {
    const nd = nodes[i];
    const dx = mx - nd.x, dy = my - nd.y;
    if (dx * dx + dy * dy < (nd.r + 6) * (nd.r + 6)) return nd;
  }
  return null;
}

function canvasCoords(e: MouseEvent | Touch): { x: number; y: number } {
  if (!cnv) return { x: 0, y: 0 };
  const rect = cnv.getBoundingClientRect();
  return { x: e.clientX - rect.left, y: e.clientY - rect.top };
}

// ── Interaction ───────────────────────────────────────────────────────────────

function setupInteraction() {
  if (!cnv) return;

  cnv.addEventListener('mousedown', (e) => {
    const { x, y } = canvasCoords(e);
    const hit = hitTest(x, y);
    if (hit) {
      dragging = hit; dragOffX = hit.x - x; dragOffY = hit.y - y;
      cnv!.style.cursor = 'grabbing'; settled = false;
    }
  });

  cnv.addEventListener('mousemove', (e) => {
    const { x, y } = canvasCoords(e);
    if (dragging) {
      dragging.x = x + dragOffX; dragging.y = y + dragOffY;
      dragging.vx = 0; dragging.vy = 0; settled = false;
    } else {
      hovered = hitTest(x, y);
      cnv!.style.cursor = hovered ? 'pointer' : 'default';
    }
  });

  cnv.addEventListener('mouseup', () => {
    if (dragging) { dragging.vx = 0; dragging.vy = 0; dragging = null; cnv!.style.cursor = hovered ? 'pointer' : 'default'; }
  });

  cnv.addEventListener('click', (e) => {
    if (dragging) return;
    const { x, y } = canvasCoords(e);
    const hit = hitTest(x, y);
    selected = hit === selected ? null : hit;
  });

  cnv.addEventListener('touchstart', (e) => {
    e.preventDefault();
    const { x, y } = canvasCoords(e.touches[0]);
    const hit = hitTest(x, y);
    if (hit) { dragging = hit; dragOffX = hit.x - x; dragOffY = hit.y - y; }
    else { selected = null; }
  }, { passive: false });

  cnv.addEventListener('touchmove', (e) => {
    e.preventDefault();
    if (dragging) {
      const { x, y } = canvasCoords(e.touches[0]);
      dragging.x = x + dragOffX; dragging.y = y + dragOffY;
      dragging.vx = 0; dragging.vy = 0; settled = false;
    }
  }, { passive: false });

  cnv.addEventListener('touchend', () => {
    if (dragging) { const d = dragging; dragging = null; selected = d === selected ? null : d; }
  });
}

// ── Resize ────────────────────────────────────────────────────────────────────

function resize() {
  if (!cnv) return;
  const rect = cnv.getBoundingClientRect();
  if (rect.width > 0) { W = rect.width; cnv.width = Math.round(W * dpr); }
  if (rect.height > 0) { H = rect.height; cnv.height = Math.round(H * dpr); }
  settled = false;
}

// ── Animation loop ────────────────────────────────────────────────────────────

function loop(ts: number) {
  if (!settled || dragging) physicsStep();
  if (!particle && !floatingLabel && edges.length > 0 && ts - lastParticle > PARTICLE_INTERVAL) {
    particle = { edgeIdx: Math.floor(Math.random() * edges.length), startTs: ts };
    lastParticle = ts;
  }
  if (W > 0 && H > 0) render(ts);
  animFrame = requestAnimationFrame(loop);
}

// ── Public API (same signatures as before — drop-in replacement) ──────────────

export function initPlaygroundGraph(containerId: string): void {
  const container = document.getElementById(containerId);
  if (!container) throw new Error(`Graph container #${containerId} not found`);

  cnv = document.createElement('canvas');
  cnv.style.width = '100%';
  cnv.style.height = '100%';
  cnv.style.display = 'block';
  container.appendChild(cnv);

  ctx = cnv.getContext('2d')!;
  dpr = Math.min(window.devicePixelRatio || 1, 2);

  resize();
  new ResizeObserver(resize).observe(cnv);
  setupInteraction();
  animFrame = requestAnimationFrame(loop);
}

export function updatePlaygroundGraph(facts: WasmFact[]): void {
  const { newNodes, newEdges } = buildGraph(facts);
  const newNodeIds = new Set(newNodes.map(n => n.id));
  const now = performance.now();

  for (const nd of newNodes) {
    if (!previousNodeIds.has(nd.id) && previousNodeIds.size > 0) nd.pulseUntil = now + 1200;
  }
  previousNodeIds = newNodeIds;

  nodes = newNodes;
  edges = newEdges;
  nodeIdx = new Map(nodes.map((n, i) => [n.id, i]));
  settled = false; settleCount = 0;
  for (let i = 0; i < 80; i++) physicsStep();
}

export function highlightNode(entityName: string): void {
  const nd = nodes.find(n => n.id === entityName);
  if (nd) selected = nd;
}

export function highlightFact(subjectName: string): void {
  const nd = nodes.find(n => n.id === subjectName);
  if (nd) nd.pulseUntil = performance.now() + 800;
}

export function clearPlaygroundGraph(): void {
  nodes = []; edges = []; nodeIdx = new Map(); previousNodeIds = new Set();
  selected = null; hovered = null; dragging = null; particle = null; floatingLabel = null;
  settled = false;
}

export function destroyPlaygroundGraph(): void {
  if (animFrame) cancelAnimationFrame(animFrame);
  if (cnv) { cnv.remove(); cnv = null; ctx = null; }
  clearPlaygroundGraph();
}
