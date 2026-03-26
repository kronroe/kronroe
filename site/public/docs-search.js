const normalize = (text) =>
  text
    .toLowerCase()
    .replace(/[^a-z0-9\s]+/g, ' ')
    .replace(/\s+/g, ' ')
    .trim();

const escapeHtml = (text) =>
  text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');

const highlight = (text, tokens) => {
  if (!tokens.length) return escapeHtml(text);
  let output = escapeHtml(text);
  for (const token of tokens.slice().sort((a, b) => b.length - a.length)) {
    if (!token) continue;
    const pattern = new RegExp(`(${token.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')})`, 'ig');
    output = output.replace(pattern, '<mark>$1</mark>');
  }
  return output;
};

const state = {
  entries: [],
  query: '',
  lastFocused: null,
};

function tokenize(query) {
  return normalize(query).split(' ').filter(Boolean);
}

function scoreEntry(entry, query, tokens) {
  const text = normalize(`${entry.title} ${entry.section} ${entry.intro} ${entry.headings.join(' ')} ${entry.bodyText}`);
  let score = 0;

  if (!query) {
    score += entry.section === 'Getting Started' ? 60 : 20;
    score += entry.title === 'What is Kronroe?' ? 50 : 0;
    score -= entry.index;
    return score;
  }

  if (normalize(entry.title).includes(query)) score += 120;
  if (normalize(entry.intro).includes(query)) score += 70;
  if (text.includes(query)) score += 40;

  for (const token of tokens) {
    if (!token) continue;
    const titleHits = normalize(entry.title).split(token).length - 1;
    const introHits = normalize(entry.intro).split(token).length - 1;
    const headingHits = normalize(entry.headings.join(' ')).split(token).length - 1;
    const bodyHits = text.split(token).length - 1;
    score += titleHits * 12;
    score += introHits * 6;
    score += headingHits * 4;
    score += bodyHits * 1.25;
  }

  return score;
}

function getResults(query) {
  const normalized = normalize(query);
  const tokens = tokenize(query);
  return state.entries
    .map((entry, index) => ({ ...entry, index, score: scoreEntry({ ...entry, index }, normalized, tokens) }))
    .filter((entry) => !query || entry.score > 0)
    .sort((a, b) => b.score - a.score || a.index - b.index)
    .slice(0, 8);
}

function resultMarkup(entry, tokens) {
  const keywords = entry.headings.slice(0, 3).join(' · ');
  return `
    <a class="docs-search-result" href="${entry.href}">
      <div class="docs-search-result-head">
        <div class="docs-search-result-title">${highlight(entry.title, tokens)}</div>
        <div class="docs-search-result-section">${escapeHtml(entry.section)}</div>
      </div>
      <div class="docs-search-result-excerpt">${highlight(entry.excerpt, tokens)}</div>
      ${keywords ? `<div class="docs-search-result-keywords">${escapeHtml(keywords)}</div>` : ''}
    </a>
  `;
}

function renderResults(query) {
  const resultsEl = document.querySelector('[data-docs-search-results]');
  const metaEl = document.querySelector('[data-docs-search-meta]');
  if (!resultsEl || !metaEl) return;

  const trimmed = query.trim();
  const tokens = tokenize(trimmed);
  const results = getResults(trimmed);

  metaEl.textContent = trimmed
    ? `${results.length} result${results.length === 1 ? '' : 's'} for "${trimmed}".`
    : 'Popular starting points and the most relevant pages.';

  if (!results.length) {
    resultsEl.innerHTML = '<div class="docs-search-empty">No docs pages matched that search.</div>';
    return;
  }

  resultsEl.innerHTML = results.map((entry) => resultMarkup(entry, tokens)).join('');
}

function openSearch() {
  const dialog = document.querySelector('[data-docs-search-dialog]');
  const input = document.querySelector('[data-docs-search-input]');
  if (!dialog || !input) return;
  state.lastFocused = document.activeElement;
  dialog.hidden = false;
  document.body.style.overflow = 'hidden';
  input.value = state.query;
  renderResults(state.query);
  window.requestAnimationFrame(() => input.focus());
}

function closeSearch() {
  const dialog = document.querySelector('[data-docs-search-dialog]');
  if (!dialog) return;
  dialog.hidden = true;
  document.body.style.overflow = '';
  if (state.lastFocused && typeof state.lastFocused.focus === 'function') {
    state.lastFocused.focus();
  }
}

async function loadIndex() {
  const response = await fetch('/docs-search.json', { cache: 'no-store' });
  if (!response.ok) throw new Error(`Search index request failed: ${response.status}`);
  state.entries = await response.json();
}

function bindEvents() {
  document.querySelectorAll('[data-docs-search-open]').forEach((button) => {
    button.addEventListener('click', openSearch);
  });

  document.querySelectorAll('[data-docs-search-close]').forEach((button) => {
    button.addEventListener('click', closeSearch);
  });

  const dialog = document.querySelector('[data-docs-search-dialog]');
  const input = document.querySelector('[data-docs-search-input]');
  if (input) {
    input.addEventListener('input', (event) => {
      state.query = event.target.value;
      renderResults(state.query);
    });
  }

  if (dialog) {
    dialog.addEventListener('click', (event) => {
      if (event.target === dialog || event.target.matches('.docs-search-backdrop')) {
        closeSearch();
      }
    });
  }

  document.addEventListener('keydown', (event) => {
    const activeTag = document.activeElement?.tagName?.toLowerCase();
    const typing = activeTag === 'input' || activeTag === 'textarea' || document.activeElement?.isContentEditable;

    if (event.key === 'Escape' && dialog && !dialog.hidden) {
      event.preventDefault();
      closeSearch();
      return;
    }

    if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'k') {
      event.preventDefault();
      openSearch();
      return;
    }

    if (!typing && event.key === '/') {
      event.preventDefault();
      openSearch();
    }
  });
}

async function init() {
  try {
    await loadIndex();
    bindEvents();
    renderResults('');
  } catch (error) {
    console.error(error);
    const metaEl = document.querySelector('[data-docs-search-meta]');
    const resultsEl = document.querySelector('[data-docs-search-results]');
    if (metaEl) metaEl.textContent = 'Search is unavailable right now.';
    if (resultsEl) resultsEl.innerHTML = '<div class="docs-search-empty">Unable to load the docs search index.</div>';
  }
}

init();
