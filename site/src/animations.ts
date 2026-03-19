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
  function animateCount(el: HTMLElement, target: number, duration: number) {
    const start = performance.now();
    function step(now: number) {
      const elapsed = now - start;
      const progress = Math.min(elapsed / duration, 1);
      const eased = 1 - Math.pow(1 - progress, 3);
      el.textContent = String(Math.round(eased * target));
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
          if (countEl && target) animateCount(countEl, target, 1400);
          io.unobserve(item);
        }
      });
    },
    { threshold: 0.4 },
  );

  document.querySelectorAll<HTMLElement>('.stat-item[data-target]').forEach((el) => io.observe(el));
})();
