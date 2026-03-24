// ── Kronroe visual animations ────────────────────────────────────────────────
// All animation/visual-effect code lives here so it's bundled by Vite and
// served as 'self' — keeping inline-script CSP off the table entirely.

// ── Progressive-enhancement animations ───────────────────────────────────────
// Add the sentinel class so the CSS knows JS is available, then set up all
// scroll-triggered reveals. Elements are fully visible without this class.
document.documentElement.classList.add('animations-ready');

const prefersReducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches;

// ── Sticky header — scroll-aware background transition ─────────────────────
(function () {
  const header = document.querySelector('header');
  if (!header) return;
  const SCROLL_THRESHOLD = 32;

  function onScroll() {
    header!.classList.toggle('header-scrolled', window.scrollY > SCROLL_THRESHOLD);
  }
  window.addEventListener('scroll', onScroll, { passive: true });
  onScroll();
})();

// ── Hero entrance — fact assertion sequence ──────────────────────────────────
// Each element appears in order, mimicking facts being asserted into the database.
// Plays once on load. Reduced-motion: everything visible immediately.
(function () {
  if (prefersReducedMotion) {
    // Show everything immediately
    document.querySelectorAll<HTMLElement>('.hero-word').forEach(el => el.classList.add('hero-word--visible'));
    document.querySelectorAll<HTMLElement>('.hero-fact').forEach(el => el.classList.add('hero-fact--visible'));
    document.querySelector('.hero-sweep')?.classList.add('hero-sweep--visible');
    document.querySelector('.hero-install')?.classList.add('hero-el--visible');
    document.querySelector('.hero-cta')?.classList.add('hero-el--visible');
    const accent = document.querySelector('.temporal-accent');
    if (accent) accent.classList.add('underline-drawn');
    return;
  }

  const words = document.querySelectorAll<HTMLElement>('.hero-word');
  const facts = document.querySelectorAll<HTMLElement>('.hero-fact');
  const accent = document.querySelector('.temporal-accent');
  const sweep = document.querySelector('.hero-sweep');
  const install = document.querySelector('.hero-install');
  const cta = document.querySelector('.hero-cta');

  // Sequence timings (ms from page load)
  // Bi- (0) → temporal (300) → underline draws (400) → AI memory. (800)
  // → No cloud. (1100) → No server. (1300) → No data risk. (1500)
  // → sweep (1700) → install (1900) → cta (2050)

  const schedule: [Element | null, string, number][] = [
    [words[0] ?? null, 'hero-word--visible', 0],       // "Bi-"
    [words[1] ?? null, 'hero-word--visible', 300],      // "temporal"
    [accent, 'underline-drawn', 400],                   // underline draws
    [words[2] ?? null, 'hero-word--visible', 800],      // "AI memory."
    [facts[0] ?? null, 'hero-fact--visible', 1100],     // "No cloud."
    [facts[1] ?? null, 'hero-fact--visible', 1300],     // "No server."
    [facts[2] ?? null, 'hero-fact--visible', 1500],     // "No data risk."
    [sweep, 'hero-sweep--visible', 1700],               // sweep paragraph
    [install, 'hero-el--visible', 1900],                // install commands
    [cta, 'hero-el--visible', 2050],                    // CTA buttons
  ];

  for (const [el, cls, delay] of schedule) {
    if (!el) continue;
    setTimeout(() => el.classList.add(cls), delay);
  }
})();

// ── Cursor-reactive graph particles ──────────────────────────────────────────
// Ambient SVG nodes drift subtly away from the cursor, like the database
// humming in the background. Spring easing for smooth return to rest.
(function () {
  if (prefersReducedMotion) return;
  const hero = document.querySelector<HTMLElement>('.hero');
  const svg = hero?.querySelector<SVGSVGElement>('.graph-bg svg');
  if (!hero || !svg) return;

  const nodes = svg.querySelectorAll<SVGGElement>('.graph-node');
  if (!nodes.length) return;

  // Pre-compute rest positions from data attributes
  const state = Array.from(nodes).map(g => ({
    el: g,
    cx: parseFloat(g.dataset.cx || '0'),
    cy: parseFloat(g.dataset.cy || '0'),
    dx: 0, dy: 0,          // current displacement
    vx: 0, vy: 0,          // velocity for spring
  }));

  const MAX_DRIFT = 12;    // max px displacement in SVG coords
  const REPEL_RADIUS = 250; // influence radius in SVG coords
  const SPRING = 0.06;     // spring constant (return to rest)
  const DAMPING = 0.75;    // velocity damping

  let mouseX = -9999, mouseY = -9999;
  let animating = false;

  hero.addEventListener('mousemove', (e: MouseEvent) => {
    // Convert page coords to SVG viewBox coords
    const rect = svg.getBoundingClientRect();
    const scaleX = 1280 / rect.width;
    const scaleY = 900 / rect.height;
    mouseX = (e.clientX - rect.left) * scaleX;
    mouseY = (e.clientY - rect.top) * scaleY;
    if (!animating) { animating = true; tick(); }
  });

  hero.addEventListener('mouseleave', () => {
    mouseX = -9999; mouseY = -9999;
  });

  function tick() {
    let moving = false;
    for (const n of state) {
      // Repulsion from cursor
      const ddx = n.cx - mouseX;
      const ddy = n.cy - mouseY;
      const dist = Math.sqrt(ddx * ddx + ddy * ddy) + 1;

      let fx = 0, fy = 0;
      if (dist < REPEL_RADIUS) {
        const strength = (1 - dist / REPEL_RADIUS) * MAX_DRIFT * 0.3;
        fx = (ddx / dist) * strength;
        fy = (ddy / dist) * strength;
      }

      // Spring back to rest
      fx -= SPRING * n.dx;
      fy -= SPRING * n.dy;

      n.vx = (n.vx + fx) * DAMPING;
      n.vy = (n.vy + fy) * DAMPING;
      n.dx += n.vx;
      n.dy += n.vy;

      // Clamp
      const mag = Math.sqrt(n.dx * n.dx + n.dy * n.dy);
      if (mag > MAX_DRIFT) {
        n.dx = (n.dx / mag) * MAX_DRIFT;
        n.dy = (n.dy / mag) * MAX_DRIFT;
      }

      n.el.setAttribute('transform', `translate(${n.dx.toFixed(1)},${n.dy.toFixed(1)})`);

      if (Math.abs(n.vx) > 0.01 || Math.abs(n.vy) > 0.01 || Math.abs(n.dx) > 0.1 || Math.abs(n.dy) > 0.1) {
        moving = true;
      }
    }

    if (moving) {
      requestAnimationFrame(tick);
    } else {
      animating = false;
      // Snap to rest
      for (const n of state) {
        n.dx = 0; n.dy = 0; n.vx = 0; n.vy = 0;
        n.el.removeAttribute('transform');
      }
    }
  }
})();

// ── Scroll reveals ────────────────────────────────────────────────────────────
(function () {
  const items = document.querySelectorAll<HTMLElement>('.reveal-on-scroll');
  if (!items.length) return;

  const io = new IntersectionObserver(
    (entries) => {
      entries.forEach((entry) => {
        if (entry.isIntersecting) {
          (entry.target as HTMLElement).classList.add('revealed');
          io.unobserve(entry.target);
        }
      });
    },
    { threshold: 0.08, rootMargin: '0px 0px -32px 0px' },
  );

  // Group by parent section so stagger resets per section
  const groups = new Map<Element, HTMLElement[]>();
  items.forEach((el) => {
    const section = el.closest('section') ?? document.body;
    if (!groups.has(section)) groups.set(section, []);
    groups.get(section)!.push(el);
  });
  groups.forEach((group) => {
    group.forEach((el, i) => {
      el.style.transitionDelay = `${i * 0.07}s`;
      io.observe(el);
    });
  });

  // Fallback: if IntersectionObserver never fires (AI browser tools, headless
  // crawlers, some extensions), reveal all content after 2 seconds so the page
  // is never blank. Real users will have scrolled by then and the normal
  // observer-driven reveals take precedence.
  setTimeout(() => {
    const anyRevealed = document.querySelector('.reveal-on-scroll.revealed');
    if (!anyRevealed) {
      items.forEach((el) => el.classList.add('revealed'));
    }
  }, 2000);
})();

// ── 3D tilt on use-case cards ─────────────────────────────────────────────────
if (!prefersReducedMotion) {
  (function () {
    document.querySelectorAll<HTMLElement>('.use-case-card').forEach((card) => {
      card.addEventListener('mousemove', (e) => {
        const rect = card.getBoundingClientRect();
        const x = (e.clientX - rect.left) / rect.width - 0.5;
        const y = (e.clientY - rect.top) / rect.height - 0.5;
        card.style.transform = `perspective(700px) rotateY(${x * 10}deg) rotateX(${-y * 10}deg) translateZ(6px)`;
      });
      card.addEventListener('mouseleave', () => {
        card.style.transform = '';
      });
    });
  })();
}

// ── NumberTicker for stats ────────────────────────────────────────────────────
(function () {
  const DURATION = 1400;

  function animateCount(
    el: HTMLElement,
    target: number,
    ringFill: SVGCircleElement | null,
    ringCircumference: number,
  ) {
    const start = performance.now();
    function step(now: number) {
      const elapsed = now - start;
      const progress = Math.min(elapsed / DURATION, 1);
      const eased = 1 - Math.pow(1 - progress, 3);
      el.textContent = String(Math.round(eased * target));
      // Sync the SVG progress ring if present
      if (ringFill) {
        const fill = eased * (target / 100); // normalise to 0-1
        ringFill.style.strokeDashoffset = String(
          ringCircumference * (1 - Math.min(fill, 1)),
        );
      }
      if (progress < 1) requestAnimationFrame(step);
    }
    requestAnimationFrame(step);
  }

  const io = new IntersectionObserver(
    (entries) => {
      entries.forEach((entry) => {
        if (entry.isIntersecting) {
          const item = entry.target as HTMLElement;
          const target = parseInt(item.dataset.target ?? '0', 10);
          const countEl = item.querySelector<HTMLElement>('.stat-count');
          const ringFill = item.querySelector<SVGCircleElement>('.stat-ring-fill');
          const circumference = ringFill
            ? 2 * Math.PI * (ringFill.r?.baseVal?.value ?? 54)
            : 0;
          if (countEl && target) {
            if (prefersReducedMotion) {
              countEl.textContent = String(target);
              if (ringFill) ringFill.style.strokeDashoffset = String(circumference * (1 - Math.min(target / 100, 1)));
            } else {
              animateCount(countEl, target, ringFill, circumference);
            }
          }
          io.unobserve(item);
        }
      });
    },
    { threshold: 0.4 },
  );

  const statItems = document.querySelectorAll<HTMLElement>('.stat-item[data-target]');
  statItems.forEach((el) => {
    // HTML has real values for a11y/no-JS; reset to 0 visually when JS runs
    const countEl = el.querySelector<HTMLElement>('.stat-count');
    if (countEl) countEl.textContent = '0';
    io.observe(el);
  });

  // Fallback: if IntersectionObserver never fires (AI browser tools, headless
  // crawlers), show the final stat values after 2 seconds.
  setTimeout(() => {
    statItems.forEach((el) => {
      const target = parseInt(el.dataset.target ?? '0', 10);
      const countEl = el.querySelector<HTMLElement>('.stat-count');
      if (countEl && countEl.textContent === '0' && target) {
        countEl.textContent = String(target);
        const ringFill = el.querySelector<SVGCircleElement>('.stat-ring-fill');
        if (ringFill) {
          const circumference = 2 * Math.PI * (ringFill.r?.baseVal?.value ?? 54);
          ringFill.style.strokeDashoffset = String(circumference * (1 - Math.min(target / 100, 1)));
        }
      }
    });
  }, 2000);
})();

// ── Tracing beam — scroll-following glow on lifecycle steps ──────────────────
(function () {
  if (prefersReducedMotion) return;
  const container = document.querySelector<HTMLElement>('.temporal-steps');
  const fill = document.getElementById('tracing-beam-fill');
  if (!container || !fill) return;

  const badges = container.querySelectorAll<HTMLElement>('.step-badge');

  function update() {
    const cRect = container!.getBoundingClientRect();
    const viewH = window.innerHeight;
    // Trigger: beam fills from 0→100% as the user scrolls through the section.
    // Use viewport 35% line so each badge lights up as its step enters the
    // upper third of the screen — the user sees the activation while reading,
    // not after they've already scrolled past.
    const scrollInto = viewH * 0.35 - cRect.top;
    const totalH = cRect.height;
    const pct = Math.max(0, Math.min(100, (scrollInto / totalH) * 100));
    fill!.style.height = `${pct}%`;

    // Light up step circles as the beam passes them
    const beamY = cRect.top + (pct / 100) * totalH;
    badges.forEach((badge) => {
      const bRect = badge.getBoundingClientRect();
      const badgeMid = bRect.top + bRect.height / 2;
      badge.classList.toggle('beam-active', beamY >= badgeMid);
    });
  }

  window.addEventListener('scroll', update, { passive: true });
  update();
})();

// ── Focus cards — blur siblings on hover ─────────────────────────────────────
(function () {
  const grid = document.querySelector<HTMLElement>('.use-case-grid');
  if (!grid) return;
  const cards = grid.querySelectorAll<HTMLElement>('.use-case-card');

  grid.addEventListener('mouseenter', () => {
    grid.classList.add('has-hover');
  });
  grid.addEventListener('mouseleave', () => {
    grid.classList.remove('has-hover');
    cards.forEach((c) => c.classList.remove('focused'));
  });
  cards.forEach((card) => {
    card.addEventListener('mouseenter', () => {
      cards.forEach((c) => c.classList.remove('focused'));
      card.classList.add('focused');
    });
  });
})();
