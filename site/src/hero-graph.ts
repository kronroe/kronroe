// ── Kronroe hero — 3D force-directed knowledge graph ─────────────────────────
// Visual language matches the logo + playground pill colours:
//   • Violet  #7C5CFC — subject nodes  (alice, bob, carol)  → subject pill
//   • Lime    #5A8A00 — object nodes   (@TechCorp, London, @Acme) → object pill
//   • Orange  #C04800 — predicate edges (works_at, lives_in, knows) → predicate pill
//   • Background #1A1510 — warm dark echoing the cream
// Node style: flat fill + stroke ring, exactly like the logo nodes. No shaded spheres.
// Physics: Coulomb repulsion + Hooke spring + Verlet + soft canvas-wall repulsion.
// Zero external libraries.

(function () {
  const cnv = document.getElementById('hero-3d-graph') as HTMLCanvasElement | null;
  if (!cnv) return;

  const ctx = cnv.getContext('2d')!;
  const dpr = Math.min(window.devicePixelRatio || 1, 2);

  // ── Sizing ───────────────────────────────────────────────────────────────────
  // Default W/H to half the window width — guarantees the sphere bounding force
  // is non-trivially active during the synchronous pre-settle even if
  // getBoundingClientRect hasn't fired yet (which can happen before first layout).
  let W = window.innerWidth * 0.5;
  let H = window.innerHeight * 0.75;

  function resize() {
    const rect = cnv!.getBoundingClientRect();
    if (rect.width > 0)  { W = rect.width;  cnv!.width  = Math.round(W * dpr); }
    if (rect.height > 0) { H = rect.height; cnv!.height = Math.round(H * dpr); }
  }
  new ResizeObserver(resize).observe(cnv);
  resize();

  // ── Colour language ───────────────────────────────────────────────────────────
  // Each constant maps directly to the playground pill colour for that role.
  const C_SUBJECT_FILL   = 'rgba(124,92,252,0.28)'; // violet tint fill
  const C_SUBJECT_STROKE = '#7C5CFC';               // violet ring
  const C_OBJECT_FILL    = 'rgba(90,138,0,0.28)';   // lime tint fill
  const C_OBJECT_STROKE  = '#5A8A00';               // lime ring
  const C_EDGE           = '#C04800';               // orange predicate
  const C_BG             = '#2A1D12';               // warm dark (echoes cream #F7F3EA)

  // ── Graph data ───────────────────────────────────────────────────────────────
  type NodeRole = 'subject' | 'object';

  interface Node3D {
    id: string; label: string;
    x: number; y: number; z: number;
    vx: number; vy: number; vz: number;
    r: number; role: NodeRole;
    expired?: boolean;
  }
  interface Edge3D {
    a: string; b: string;
    label: string;
    validFrom?: string; validTo?: string;
    dashed?: boolean; width: number;
  }

  // Subject = people (violet pill). Object = orgs/places (lime pill).
  const nodes: Node3D[] = [
    { id:'alice',    label:'alice',     x:  -20, y:   0, z:  45, vx:0,vy:0,vz:0, r:16, role:'subject' },
    { id:'techcorp', label:'@TechCorp', x:  230, y:-120, z:  70, vx:0,vy:0,vz:0, r:12, role:'object'  },
    { id:'acme',     label:'@Acme',     x: -230, y:  30, z: -90, vx:0,vy:0,vz:0, r:12, role:'object', expired:true },
    { id:'london',   label:'London',    x:   25, y: 220, z: -30, vx:0,vy:0,vz:0, r:12, role:'object'  },
    { id:'bob',      label:'bob',       x:  195, y: 125, z:-135, vx:0,vy:0,vz:0, r:14, role:'subject' },
    { id:'carol',    label:'carol',     x: -195, y:-135, z: 105, vx:0,vy:0,vz:0, r:14, role:'subject' },
  ];

  // Edges: all orange (predicate colour). Expired = dashed at 30% opacity.
  const edges: Edge3D[] = [
    { a:'alice', b:'techcorp', label:'works_at', validFrom:'2023-01',                   width:1.5 },
    { a:'alice', b:'acme',     label:'works_at', validFrom:'2020-03', validTo:'2022-06', width:1.5, dashed:true },
    { a:'alice', b:'london',   label:'lives_in', validFrom:'2021-09',                   width:1.5 },
    { a:'alice', b:'carol',    label:'knows',    validFrom:'2019-04',                   width:1.5 },
    { a:'bob',   b:'techcorp', label:'works_at', validFrom:'2022-11',                   width:1.5 },
    { a:'carol', b:'london',   label:'lives_in', validFrom:'2020-07',                   width:1.5 },
  ];

  const nodeIdx = new Map<string, number>(nodes.map((n, i) => [n.id, i]));
  const activeEdgeIndices = [0, 2, 3, 4, 5]; // non-expired, used for particle spawning

  // ── Interaction state (declared early — physicsStep references dragging) ─────
  let dragging = false;

  // ── Physics ──────────────────────────────────────────────────────────────────
  const K_REPEL    = 28000;
  const K_SPRING   = 0.022;
  const REST_LEN   = 135;
  const DAMPING    = 0.83;
  const GRAVITY    = 0.013;
  // World-space sphere bounding — rotation-invariant containment.
  // 3D distance from origin is independent of camera angle, so this works
  // correctly at any rotY/rotX without the screen-space projection mismatch.
  // At FOV=620, CAM_DIST=360, a world radius of 130 projects to ≤224px from
  // screen centre at any angle, fitting comfortably in the ~600px-wide panel.
  const WORLD_BOUND = 130;
  const WORLD_K     = 0.28;

  const fx = new Float32Array(nodes.length);
  const fy = new Float32Array(nodes.length);
  const fz = new Float32Array(nodes.length);

  // ── Projection ───────────────────────────────────────────────────────────────
  // Defined before physicsStep because wall forces need project() at each step.
  const CAM_DIST = 360;
  const FOV      = 620;
  let rotY = 0.30;
  let rotX = 0.15;

  interface Proj { sx: number; sy: number; sz: number; scale: number; nd: Node3D; }

  function project(nd: Node3D): Proj {
    const cosY = Math.cos(rotY), sinY = Math.sin(rotY);
    const x1 =  cosY*nd.x + sinY*nd.z;
    const y1 =  nd.y;
    const z1 = -sinY*nd.x + cosY*nd.z;
    const cosX = Math.cos(rotX), sinX = Math.sin(rotX);
    const x2 = x1;
    const y2 =  cosX*y1 - sinX*z1;
    const z2 =  sinX*y1 + cosX*z1;
    const scale = FOV / (z2 + CAM_DIST);
    return { sx: W/2 + x2*scale, sy: H/2 + y2*scale, sz: z2, scale, nd };
  }

  function physicsStep() {
    const n = nodes.length;
    fx.fill(0); fy.fill(0); fz.fill(0);

    // Coulomb repulsion — MIN_D2 floor prevents numerical explosion at close range
    for (let i = 0; i < n; i++) {
      for (let j = i + 1; j < n; j++) {
        const dx = nodes[i].x - nodes[j].x;
        const dy = nodes[i].y - nodes[j].y;
        const dz = nodes[i].z - nodes[j].z;
        const d2 = Math.max(dx*dx + dy*dy + dz*dz, 60*60);
        const d  = Math.sqrt(d2);
        const f  = K_REPEL / d2;
        const ux = dx/d, uy = dy/d, uz = dz/d;
        fx[i] += f*ux; fy[i] += f*uy; fz[i] += f*uz;
        fx[j] -= f*ux; fy[j] -= f*uy; fz[j] -= f*uz;
      }
    }

    // Hooke spring along edges
    for (const e of edges) {
      const i = nodeIdx.get(e.a)!;
      const j = nodeIdx.get(e.b)!;
      const dx = nodes[j].x - nodes[i].x;
      const dy = nodes[j].y - nodes[i].y;
      const dz = nodes[j].z - nodes[i].z;
      const d  = Math.sqrt(dx*dx + dy*dy + dz*dz) + 1e-4;
      const f  = K_SPRING * (d - REST_LEN);
      const ux = dx/d, uy = dy/d, uz = dz/d;
      fx[i] += f*ux; fy[i] += f*uy; fz[i] += f*uz;
      fx[j] -= f*ux; fy[j] -= f*uy; fz[j] -= f*uz;
    }

    // Center gravity — slight upward bias (+20 y) so graph fills the dark panel vertically
    for (let i = 0; i < n; i++) {
      fx[i] -= GRAVITY * nodes[i].x;
      fy[i] -= GRAVITY * (nodes[i].y + 20);
      fz[i] -= GRAVITY * nodes[i].z;
    }

    // World-space sphere bounding — rotation-invariant.
    // Pushes any node outside WORLD_BOUND radius back toward the origin.
    // Works correctly at any camera angle, unlike the old screen-space projection.
    for (let i = 0; i < n; i++) {
      const r2 = nodes[i].x**2 + nodes[i].y**2 + nodes[i].z**2;
      if (r2 > WORLD_BOUND * WORLD_BOUND) {
        const r = Math.sqrt(r2);
        const over = r - WORLD_BOUND;
        fx[i] -= WORLD_K * over * nodes[i].x / r;
        fy[i] -= WORLD_K * over * nodes[i].y / r;
        fz[i] -= WORLD_K * over * nodes[i].z / r;
      }
    }

    // Verlet integration
    for (let i = 0; i < n; i++) {
      nodes[i].vx = (nodes[i].vx + fx[i]) * DAMPING;
      nodes[i].vy = (nodes[i].vy + fy[i]) * DAMPING;
      nodes[i].vz = (nodes[i].vz + fz[i]) * DAMPING;
      nodes[i].x += nodes[i].vx;
      nodes[i].y += nodes[i].vy;
      nodes[i].z += nodes[i].vz;
    }
  }

  // Pre-settle: run physics synchronously before first frame.
  // W/H are initialised to window.innerWidth*0.5 / innerHeight*0.75 above,
  // so the sphere bounding force is active throughout these steps.
  for (let s = 0; s < 700; s++) physicsStep();
  nodes.forEach(n => { n.vx = 0; n.vy = 0; n.vz = 0; });

  // ── Temporal particle system ──────────────────────────────────────────────────
  // A small orange dot travels along an active edge every PARTICLE_INTERVAL ms,
  // then a "recorded_at" label floats up from the destination node.
  const PARTICLE_INTERVAL = 8000;
  const PARTICLE_TRAVEL   = 1800; // ms

  interface Particle { edgeIdx: number; startTs: number; }
  interface FloatingLabel { text: string; x: number; y: number; startTs: number; life: number; }

  let particle: Particle | null = null;
  let floatingLabel: FloatingLabel | null = null;
  let lastParticle = 0;

  // ── Helpers ───────────────────────────────────────────────────────────────────
  function hexAlpha(hex: string, a: number): string {
    const r = parseInt(hex.slice(1,3), 16);
    const g = parseInt(hex.slice(3,5), 16);
    const b = parseInt(hex.slice(5,7), 16);
    return `rgba(${r},${g},${b},${a})`;
  }

  // ── Render ────────────────────────────────────────────────────────────────────
  let pulsePhase = 0;

  function render(ts: number) {
    pulsePhase = ts / 1000;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);

    // ── Background — warm dark
    ctx.fillStyle = C_BG;
    ctx.fillRect(0, 0, W, H);

    // Faint dot grid — matches the cream side's radial dot pattern for visual cohesion.
    // Very low opacity so it reads as texture, not noise.
    const dotSpacing = 28;
    ctx.fillStyle = 'rgba(124,92,252,0.055)';
    for (let dx = dotSpacing / 2; dx < W; dx += dotSpacing) {
      for (let dy = dotSpacing / 2; dy < H; dy += dotSpacing) {
        ctx.beginPath();
        ctx.arc(dx, dy, 1.2, 0, Math.PI * 2);
        ctx.fill();
      }
    }

    // Central violet bloom
    const bloom = ctx.createRadialGradient(W/2, H/2, 0, W/2, H/2, Math.max(W, H) * 0.65);
    bloom.addColorStop(0, 'rgba(124,92,252,0.055)');
    bloom.addColorStop(1, 'rgba(0,0,0,0)');
    ctx.fillStyle = bloom;
    ctx.fillRect(0, 0, W, H);

    // Left-edge bridge gradient — softens the seam between cream and dark panels
    const bridge = ctx.createLinearGradient(0, 0, W * 0.12, 0);
    bridge.addColorStop(0, 'rgba(247,243,234,0.048)');
    bridge.addColorStop(1, 'rgba(247,243,234,0)');
    ctx.fillStyle = bridge;
    ctx.fillRect(0, 0, W * 0.12, H);

    const proj: Proj[] = nodes.map(project);
    const byZ = [...proj].sort((a, b) => a.sz - b.sz);

    // ── Edges ──
    for (let ei = 0; ei < edges.length; ei++) {
      const e  = edges[ei];
      const pi = proj[nodeIdx.get(e.a)!];
      const pj = proj[nodeIdx.get(e.b)!];
      const avgZ   = (pi.sz + pj.sz) / 2;
      const depthT = Math.max(0, Math.min(1, (avgZ + 300) / 550));

      // Expired edges render at 30% opacity and dashed — same language as the playground
      const lineAlpha  = e.dashed ? 0.28 : (0.38 + depthT * 0.42);
      const labelAlpha = e.dashed ? 0.38 : (0.55 + depthT * 0.35);

      ctx.save();
      ctx.strokeStyle = hexAlpha(C_EDGE, lineAlpha);
      ctx.lineWidth   = e.width;
      if (e.dashed) ctx.setLineDash([5, 5]);

      // Clip edge endpoints to node circumferences — prevents lines from
      // piercing through transparent node fills creating a "pie slice" artifact.
      const ri   = pi.nd.r * pi.scale;
      const rj   = pj.nd.r * pj.scale;
      const edge_dx = pj.sx - pi.sx, edge_dy = pj.sy - pi.sy;
      const edist   = Math.hypot(edge_dx, edge_dy) + 1e-4;
      ctx.beginPath();
      ctx.moveTo(pi.sx + edge_dx/edist * ri, pi.sy + edge_dy/edist * ri);
      ctx.lineTo(pj.sx - edge_dx/edist * rj, pj.sy - edge_dy/edist * rj);
      ctx.stroke();
      ctx.setLineDash([]);

      // Predicate label mid-edge (orange, perpendicular offset)
      const mx  = (pi.sx + pj.sx) / 2;
      const my  = (pi.sy + pj.sy) / 2;
      const edx = pj.sx - pi.sx, edy = pj.sy - pi.sy;
      const el  = Math.hypot(edx, edy) || 1;
      const ox  = -edy / el * 13;
      const oy  =  edx / el * 13;

      ctx.textAlign    = 'center';
      ctx.textBaseline = 'middle';

      ctx.globalAlpha = labelAlpha;
      ctx.font        = '500 9px "JetBrains Mono", monospace';
      ctx.fillStyle   = C_EDGE;
      ctx.fillText(e.label, mx + ox, my + oy);

      // Temporal sub-label — format exactly mirrors the playground fact row timestamps
      const tLabel = e.validTo
        ? `${e.validFrom} → ${e.validTo}`
        : e.validFrom ? `${e.validFrom} →` : '';
      if (tLabel) {
        ctx.globalAlpha = labelAlpha * 0.68;
        ctx.font        = '400 7px "JetBrains Mono", monospace';
        ctx.fillText(tLabel, mx + ox, my + oy + 11);
      }

      ctx.restore();
    }

    // ── Travelling particle (temporal event — orange dot along edge) ──
    if (particle) {
      const elapsed = ts - particle.startTs;
      const t = Math.min(elapsed / PARTICLE_TRAVEL, 1);
      const e  = edges[particle.edgeIdx];
      const pi = proj[nodeIdx.get(e.a)!];
      const pj = proj[nodeIdx.get(e.b)!];
      const px = pi.sx + (pj.sx - pi.sx) * t;
      const py = pi.sy + (pj.sy - pi.sy) * t;
      const fade = t < 0.12 ? t / 0.12 : t > 0.88 ? (1 - t) / 0.12 : 1;

      ctx.save();
      ctx.globalAlpha = fade * 0.88;
      ctx.fillStyle   = C_EDGE;
      ctx.beginPath();
      ctx.arc(px, py, 2.8, 0, Math.PI * 2);
      ctx.fill();
      ctx.restore();

      if (t >= 1) {
        const now = new Date().toISOString().slice(0, 7);
        floatingLabel = {
          text: `recorded_at: ${now}`,
          x: pj.sx, y: pj.sy - pj.nd.r * pj.scale - 10,
          startTs: ts, life: 2400,
        };
        particle = null;
      }
    }

    // ── Floating "recorded_at" label ──
    if (floatingLabel) {
      const t = Math.min((ts - floatingLabel.startTs) / floatingLabel.life, 1);
      if (t >= 1) {
        floatingLabel = null;
      } else {
        const fadeA = t < 0.15 ? t / 0.15 : t > 0.68 ? 1 - (t - 0.68) / 0.32 : 1;
        ctx.save();
        ctx.globalAlpha  = fadeA * 0.80;
        ctx.font         = '500 8px "JetBrains Mono", monospace';
        ctx.fillStyle    = C_EDGE;
        ctx.textAlign    = 'center';
        ctx.textBaseline = 'middle';
        ctx.fillText(floatingLabel.text, floatingLabel.x, floatingLabel.y - t * 18);
        ctx.restore();
      }
    }

    // ── Nodes (back → front, painter's algorithm) ──
    for (const p of byZ) {
      const nd     = p.nd;
      const depthT = Math.max(0, Math.min(1, (p.sz + 200) / 400));
      const alpha  = nd.expired ? 0.48 : (0.62 + depthT * 0.38);
      const r      = nd.r * p.scale;

      const fillColor   = nd.role === 'subject' ? C_SUBJECT_FILL   : C_OBJECT_FILL;
      const strokeColor = nd.role === 'subject' ? C_SUBJECT_STROKE : C_OBJECT_STROKE;

      ctx.save();
      ctx.globalAlpha = alpha;

      // Flat fill — no radial gradient, no specular, no glow. Matches logo node style.
      ctx.fillStyle = fillColor;
      ctx.beginPath();
      ctx.arc(p.sx, p.sy, r, 0, Math.PI * 2);
      ctx.fill();

      // Stroke ring — solid for active, dashed for expired.
      // This is the direct equivalent of the logo's circle + ring visual.
      ctx.strokeStyle = strokeColor;
      ctx.lineWidth   = nd.expired ? 1.2 : 1.8;
      if (nd.expired) {
        ctx.setLineDash([4, 3]);
        ctx.globalAlpha = alpha * 0.70;
      }
      ctx.beginPath();
      ctx.arc(p.sx, p.sy, r, 0, Math.PI * 2);
      ctx.stroke();
      ctx.setLineDash([]);
      ctx.globalAlpha = alpha;

      // Subject nodes: outer ring — logo's "active entity" indicator.
      // Fixed 10px screen-space gap so the ring stays tight regardless of depth/scale.
      if (nd.role === 'subject' && !nd.expired) {
        ctx.strokeStyle = hexAlpha(strokeColor, 0.30);
        ctx.lineWidth   = 1.0;
        ctx.beginPath();
        ctx.arc(p.sx, p.sy, r + 10, 0, Math.PI * 2);
        ctx.stroke();
      }

      // Alice (hub): slow expanding pulse ring — flat, no glow, fixed pixel expansion
      if (nd.id === 'alice') {
        const cycle = (pulsePhase % 2.8) / 2.8;
        const pr    = r + 10 + cycle * 22;
        const pa    = (1 - cycle) * 0.38;
        ctx.strokeStyle = hexAlpha(strokeColor, pa);
        ctx.lineWidth   = 1.0;
        ctx.beginPath();
        ctx.arc(p.sx, p.sy, pr, 0, Math.PI * 2);
        ctx.stroke();
      }

      // Node label — white, handwriting font, below node
      ctx.setLineDash([]);
      ctx.globalAlpha  = alpha * 0.90;
      const fontSize   = Math.max(8, Math.round(9.5 * p.scale));
      ctx.font         = `600 ${fontSize}px "Quicksand", system-ui, sans-serif`;
      ctx.fillStyle    = nd.expired ? 'rgba(255,255,255,0.52)' : 'rgba(255,255,255,0.88)';
      ctx.textAlign    = 'center';
      ctx.textBaseline = 'top';
      ctx.fillText(nd.label, p.sx, p.sy + r + 3 * p.scale);
      ctx.restore();
    }
  }

  // ── Interaction ───────────────────────────────────────────────────────────────
  let autoRotate   = true;
  // dragging declared early (above physicsStep) to avoid temporal dead zone
  let lastInteract = 0;
  const RESUME_MS  = 2000;
  let mx0 = 0, my0 = 0;
  let hovered: Node3D | null = null;
  const HIT_SLOP = 9;

  cnv.addEventListener('mousedown', e => {
    dragging = true; autoRotate = false;
    mx0 = e.clientX; my0 = e.clientY;
    cnv.style.cursor = 'grabbing';
    lastInteract = performance.now();
  });

  window.addEventListener('mousemove', e => {
    if (dragging) {
      rotY += (e.clientX - mx0) * 0.006;
      rotX += (e.clientY - my0) * 0.006;
      rotX  = Math.max(-1.1, Math.min(1.1, rotX));
      mx0 = e.clientX; my0 = e.clientY;
      lastInteract = performance.now();
    } else if (W > 0 && H > 0) {
      const rect = cnv.getBoundingClientRect();
      const cx = e.clientX - rect.left;
      const cy = e.clientY - rect.top;
      hovered = null;
      for (const nd of nodes) {
        const p = project(nd);
        const r = nd.r * p.scale + HIT_SLOP;
        if ((cx - p.sx)**2 + (cy - p.sy)**2 < r*r) { hovered = nd; break; }
      }
      cnv.style.cursor = hovered ? 'pointer' : 'grab';
    }
  });

  window.addEventListener('mouseup', () => {
    dragging = false;
    // Kill accumulated velocity so nodes settle smoothly after drag rather than flying off.
    nodes.forEach(n => { n.vx = 0; n.vy = 0; n.vz = 0; });
    cnv.style.cursor = hovered ? 'pointer' : 'grab';
    lastInteract = performance.now();
  });

  let tx0 = 0, ty0 = 0;
  cnv.addEventListener('touchstart', e => {
    e.preventDefault();
    dragging = true; autoRotate = false;
    tx0 = e.touches[0].clientX; ty0 = e.touches[0].clientY;
  }, { passive: false });

  cnv.addEventListener('touchmove', e => {
    e.preventDefault();
    rotY += (e.touches[0].clientX - tx0) * 0.006;
    rotX += (e.touches[0].clientY - ty0) * 0.006;
    rotX  = Math.max(-1.1, Math.min(1.1, rotX));
    tx0 = e.touches[0].clientX; ty0 = e.touches[0].clientY;
    lastInteract = performance.now();
  }, { passive: false });

  cnv.addEventListener('touchend', () => {
    dragging = false;
    nodes.forEach(n => { n.vx = 0; n.vy = 0; n.vz = 0; });
    lastInteract = performance.now();
  });

  // ── Animation loop ────────────────────────────────────────────────────────────
  function loop(ts: number) {
    if (!dragging && !autoRotate && performance.now() - lastInteract > RESUME_MS) {
      autoRotate = true;
    }
    if (autoRotate) rotY += 0.003;
    physicsStep();

    if (!particle && !floatingLabel && ts - lastParticle > PARTICLE_INTERVAL) {
      const ei = activeEdgeIndices[Math.floor(Math.random() * activeEdgeIndices.length)];
      particle     = { edgeIdx: ei, startTs: ts };
      lastParticle = ts;
    }

    if (W > 0 && H > 0) render(ts);
    requestAnimationFrame(loop);
  }

  cnv.style.cursor = 'grab';
  requestAnimationFrame(loop);
})();
