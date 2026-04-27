/**
 * Kronroe docs interactions
 * ─────────────────────────────────────────────────────────────
 * Five interactions, all vanilla JS, no framework:
 *   1. Dark/light theme toggle (persists in localStorage)
 *   2. Mobile sidebar burger menu
 *   3. Code-block copy buttons
 *   4. Active-section highlighting in the on-page TOC
 *   5. CMD+K search dialog with keyword matching
 *
 * Depends on /docs/_assets/search.json (built by build-docs.py).
 *
 * Future (Phase 3): semantic search via /api/docs/recall instead of
 * client-side keyword matching.
 */
(function () {
  'use strict';

  // ─── Theme toggle ──────────────────────────────────────────

  const THEME_KEY = 'kronroe-docs-theme';
  function applyTheme(theme) {
    if (theme === 'dark') document.documentElement.setAttribute('data-theme', 'dark');
    else document.documentElement.removeAttribute('data-theme');
  }
  function initTheme() {
    const stored = localStorage.getItem(THEME_KEY);
    const prefersDark = window.matchMedia && window.matchMedia('(prefers-color-scheme: dark)').matches;
    applyTheme(stored || (prefersDark ? 'dark' : 'light'));
    document.querySelectorAll('.kr-docs-theme-toggle').forEach((btn) => {
      btn.addEventListener('click', () => {
        const next = document.documentElement.getAttribute('data-theme') === 'dark' ? 'light' : 'dark';
        localStorage.setItem(THEME_KEY, next);
        applyTheme(next);
      });
    });
  }

  // ─── Mobile sidebar burger ─────────────────────────────────

  function initBurger() {
    document.querySelectorAll('.kr-docs-burger').forEach((btn) => {
      btn.addEventListener('click', () => {
        document.body.classList.toggle('kr-sidebar-open');
      });
    });
    // Close when clicking the backdrop or any sidebar link.
    document.addEventListener('click', (e) => {
      if (!document.body.classList.contains('kr-sidebar-open')) return;
      const target = e.target;
      if (target.closest('.kr-sidebar-link')) {
        document.body.classList.remove('kr-sidebar-open');
      } else if (!target.closest('.kr-sidebar') && !target.closest('.kr-docs-burger')) {
        document.body.classList.remove('kr-sidebar-open');
      }
    });
  }

  // ─── Copy buttons ──────────────────────────────────────────

  function initCopyButtons() {
    document.querySelectorAll('.kr-code-copy').forEach((btn) => {
      btn.addEventListener('click', async () => {
        const wrap = btn.closest('.kr-code-wrap');
        if (!wrap) return;
        const codeEl = wrap.querySelector('pre');
        const text = codeEl ? codeEl.innerText : '';
        try {
          await navigator.clipboard.writeText(text);
          btn.textContent = 'Copied';
          btn.classList.add('is-copied');
          setTimeout(() => {
            btn.textContent = 'Copy';
            btn.classList.remove('is-copied');
          }, 1500);
        } catch (_e) {
          btn.textContent = 'Failed';
          setTimeout(() => { btn.textContent = 'Copy'; }, 1500);
        }
      });
    });
  }

  // ─── Active TOC highlighting ───────────────────────────────

  function initTocHighlight() {
    const tocLinks = Array.from(document.querySelectorAll('.kr-toc-list a'));
    if (!tocLinks.length) return;

    const headingIds = tocLinks.map((a) => a.getAttribute('href').slice(1));
    const headingEls = headingIds
      .map((id) => document.getElementById(id))
      .filter(Boolean);

    if (!headingEls.length) return;

    const observer = new IntersectionObserver(
      (entries) => {
        entries.forEach((entry) => {
          const id = entry.target.id;
          const link = tocLinks.find((a) => a.getAttribute('href') === '#' + id);
          if (!link) return;
          if (entry.isIntersecting) {
            tocLinks.forEach((a) => a.classList.remove('is-active'));
            link.classList.add('is-active');
          }
        });
      },
      { rootMargin: '-20% 0px -75% 0px', threshold: 0 }
    );
    headingEls.forEach((h) => observer.observe(h));
  }

  // ─── Search dialog ─────────────────────────────────────────

  let searchIndex = null;
  async function loadSearchIndex() {
    if (searchIndex) return searchIndex;
    try {
      const res = await fetch('/docs/_assets/search.json');
      const json = await res.json();
      searchIndex = json.docs || [];
    } catch (_e) {
      searchIndex = [];
    }
    return searchIndex;
  }

  function escapeHtml(s) {
    return s
      .replace(/&/g, '&amp;')
      .replace(/</g, '&lt;')
      .replace(/>/g, '&gt;')
      .replace(/"/g, '&quot;');
  }

  function highlightMatch(text, query) {
    if (!query) return escapeHtml(text);
    const escaped = escapeHtml(text);
    const safeQuery = query.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
    return escaped.replace(new RegExp('(' + safeQuery + ')', 'ig'), '<mark>$1</mark>');
  }

  function snippet(body, query, length = 140) {
    if (!query) return body.slice(0, length) + (body.length > length ? '...' : '');
    const lower = body.toLowerCase();
    const idx = lower.indexOf(query.toLowerCase());
    if (idx < 0) return body.slice(0, length) + (body.length > length ? '...' : '');
    const start = Math.max(0, idx - 40);
    const excerpt = body.slice(start, start + length);
    return (start > 0 ? '...' : '') + excerpt + (start + length < body.length ? '...' : '');
  }

  function searchDocs(query, index) {
    if (!query) return [];
    const q = query.toLowerCase();
    const scored = index
      .map((doc) => {
        let score = 0;
        if (doc.title.toLowerCase().includes(q)) score += 10;
        if (doc.headings.some((h) => h.toLowerCase().includes(q))) score += 5;
        if (doc.description.toLowerCase().includes(q)) score += 2;
        if (doc.body.toLowerCase().includes(q)) score += 1;
        return { doc, score };
      })
      .filter((r) => r.score > 0)
      .sort((a, b) => b.score - a.score)
      .slice(0, 8);
    return scored.map((r) => r.doc);
  }

  function renderResults(query, results) {
    if (!query) {
      return '<div class="kr-docs-search-empty">Start typing to search the docs.</div>';
    }
    if (!results.length) {
      return `<div class="kr-docs-search-empty">No results for "${escapeHtml(query)}".</div>`;
    }
    return results
      .map((doc, i) => `
        <a class="kr-docs-search-result${i === 0 ? ' is-active' : ''}" href="${doc.url}">
          <div class="kr-docs-search-result-cat">${escapeHtml(doc.category || 'Docs')}</div>
          <div class="kr-docs-search-result-title">${highlightMatch(doc.title, query)}</div>
          <div class="kr-docs-search-result-snippet">${highlightMatch(snippet(doc.body, query), query)}</div>
        </a>
      `)
      .join('');
  }

  function initSearch() {
    const dialog = document.querySelector('.kr-docs-search-dialog');
    const trigger = document.querySelector('.kr-docs-search-trigger');
    const input = document.querySelector('.kr-docs-search-input');
    const resultsEl = document.querySelector('.kr-docs-search-results');
    const closeBtn = document.querySelector('.kr-docs-search-close');
    if (!dialog || !input || !resultsEl) return;

    function open() {
      loadSearchIndex().then(() => {
        if (!dialog.open) dialog.showModal();
        setTimeout(() => input.focus(), 50);
        resultsEl.innerHTML = renderResults('', []);
      });
    }
    function close() {
      if (dialog.open) dialog.close();
      input.value = '';
    }

    if (trigger) trigger.addEventListener('click', open);
    if (closeBtn) closeBtn.addEventListener('click', close);

    document.addEventListener('keydown', (e) => {
      // CMD+K / CTRL+K to open
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'k') {
        e.preventDefault();
        open();
      }
      // Slash to open (when not focused on an input)
      if (e.key === '/' && !['INPUT', 'TEXTAREA'].includes(document.activeElement.tagName)) {
        e.preventDefault();
        open();
      }
    });

    input.addEventListener('input', () => {
      const q = input.value.trim();
      const results = searchDocs(q, searchIndex || []);
      resultsEl.innerHTML = renderResults(q, results);
    });

    // Arrow-key + enter navigation in the results list.
    input.addEventListener('keydown', (e) => {
      const links = Array.from(resultsEl.querySelectorAll('a.kr-docs-search-result'));
      if (!links.length) return;
      const activeIndex = links.findIndex((l) => l.classList.contains('is-active'));
      if (e.key === 'ArrowDown') {
        e.preventDefault();
        const next = (activeIndex + 1) % links.length;
        links.forEach((l) => l.classList.remove('is-active'));
        links[next].classList.add('is-active');
        links[next].scrollIntoView({ block: 'nearest' });
      } else if (e.key === 'ArrowUp') {
        e.preventDefault();
        const next = (activeIndex - 1 + links.length) % links.length;
        links.forEach((l) => l.classList.remove('is-active'));
        links[next].classList.add('is-active');
        links[next].scrollIntoView({ block: 'nearest' });
      } else if (e.key === 'Enter') {
        e.preventDefault();
        const target = links[activeIndex >= 0 ? activeIndex : 0];
        if (target) window.location.href = target.getAttribute('href');
      }
    });
  }

  // ─── Init on DOM ready ─────────────────────────────────────

  function init() {
    initTheme();
    initBurger();
    initCopyButtons();
    initTocHighlight();
    initSearch();
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', init);
  } else {
    init();
  }
})();
