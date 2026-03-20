// ── Kronroe visual animations ────────────────────────────────────────────────
// All animation/visual-effect code lives here so it's bundled by Vite and
// served as 'self' — keeping inline-script CSP off the table entirely.

// ── Progressive-enhancement animations ───────────────────────────────────────
// Add the sentinel class so the CSS knows JS is available, then set up all
// scroll-triggered reveals. Elements are fully visible without this class.
document.documentElement.classList.add('animations-ready');

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
})();

// ── 3D tilt on use-case cards ─────────────────────────────────────────────────
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
          if (countEl && target) animateCount(countEl, target, ringFill, circumference);
          io.unobserve(item);
        }
      });
    },
    { threshold: 0.4 },
  );

  document.querySelectorAll<HTMLElement>('.stat-item[data-target]').forEach((el) => io.observe(el));
})();

// ── Tracing beam — scroll-following glow on lifecycle steps ──────────────────
(function () {
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
