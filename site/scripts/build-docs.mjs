import fs from 'node:fs/promises';
import path from 'node:path';
import MarkdownIt from 'markdown-it';

const rootDir = process.cwd();
const docsDir = path.join(rootDir, 'docs');

const md = new MarkdownIt({
  html: false,
  linkify: true,
  typographer: true,
});

const docsEntries = [
  { source: 'getting-started/quick-start-mcp.md', title: 'Quick Start: MCP', label: 'Getting Started' },
  { source: 'getting-started/quick-start-python.md', title: 'Quick Start: Python', label: 'Getting Started' },
  { source: 'getting-started/quick-start-rust.md', title: 'Quick Start: Rust', label: 'Getting Started' },
  { source: 'getting-started/what-is-kronroe.md', title: 'What is Kronroe?', label: 'Getting Started' },
  { source: 'concepts/bi-temporal-model.md', title: 'Bi-Temporal Model', label: 'Core Concepts' },
  { source: 'concepts/facts-and-entities.md', title: 'Facts and Entities', label: 'Core Concepts' },
  { source: 'api/agent-memory.md', title: 'AgentMemory', label: 'API Reference' },
  { source: 'api/core.md', title: 'TemporalGraph (Core)', label: 'API Reference' },
  { source: 'api/mcp-tools.md', title: 'MCP Tools', label: 'API Reference' },
];

const sidebarSections = [
  {
    title: 'Getting Started',
    items: [
      ['What is Kronroe?', '/docs/getting-started/what-is-kronroe/'],
      ['Quick Start: Python', '/docs/getting-started/quick-start-python/'],
      ['Quick Start: Rust', '/docs/getting-started/quick-start-rust/'],
      ['Quick Start: MCP', '/docs/getting-started/quick-start-mcp/'],
    ],
  },
  {
    title: 'Core Concepts',
    items: [
      ['Bi-Temporal Model', '/docs/concepts/bi-temporal-model/'],
      ['Facts and Entities', '/docs/concepts/facts-and-entities/'],
    ],
  },
  {
    title: 'API Reference',
    items: [
      ['TemporalGraph (Core)', '/docs/api/core/'],
      ['AgentMemory', '/docs/api/agent-memory/'],
      ['MCP Tools', '/docs/api/mcp-tools/'],
    ],
  },
];

const docsEntryBySource = new Map(docsEntries.map((entry, index) => [entry.source, { ...entry, index }]));

function stripFrontMatter(source) {
  if (!source.startsWith('---\n')) return source;
  const end = source.indexOf('\n---\n', 4);
  if (end === -1) return source;
  return source.slice(end + 5);
}

function extractTitle(source, fallback) {
  const match = source.match(/^#\s+(.+)$/m);
  if (match) return match[1].trim();
  return fallback;
}

function extractIntro(source) {
  const withoutTitle = source.replace(/^#\s+.+\n+/, '');
  const match = withoutTitle.match(/^\s*([^\n].+?)\n(?:\n|$)/s);
  if (!match) return '';
  const firstLine = match[1].trim();
  if (firstLine.startsWith('#') || firstLine.startsWith('<')) return '';
  const intro = match[1]
    .replace(/\[(.+?)\]\((.+?)\)/g, '$1')
    .replace(/`([^`]+)`/g, '$1')
    .replace(/\s+/g, ' ')
    .trim();
  return intro.startsWith('#') || intro.startsWith('<') ? '' : intro;
}

function extractDocsTabs(source) {
  const blocks = [];
  let output = '';
  let cursor = 0;

  while (cursor < source.length) {
    const start = source.indexOf('<div class="docs-tabs" data-docs-tabs>', cursor);
    if (start === -1) break;

    output += source.slice(cursor, start);
    let pos = start;
    let depth = 1;
    pos += '<div'.length;

    while (pos < source.length) {
      const nextOpen = source.indexOf('<div', pos);
      const nextClose = source.indexOf('</div>', pos);
      if (nextClose === -1) break;

      if (nextOpen !== -1 && nextOpen < nextClose) {
        depth += 1;
        pos = nextOpen + '<div'.length;
        continue;
      }

      depth -= 1;
      pos = nextClose + '</div>'.length;
      if (depth === 0) break;
    }

    const block = source.slice(start, pos);
    const token = `@@DOCS_TABS_${blocks.length}@@`;
    blocks.push({ block, token });
    output += token;
    cursor = pos;
  }

  output += source.slice(cursor);
  return { source: output, blocks };
}

function markdownToPlainText(source) {
  return source
    .replace(/^---\n[\s\S]*?\n---\n/, '')
    .replace(/^#\s+.+\n+/, '')
    .replace(/```[\s\S]*?```/g, ' ')
    .replace(/@@DOCS_TABS_\d+@@/g, ' ')
    .replace(/<[^>]+>/g, ' ')
    .replace(/`([^`]+)`/g, '$1')
    .replace(/!\[[^\]]*\]\([^)]+\)/g, ' ')
    .replace(/\[([^\]]+)\]\([^)]+\)/g, '$1')
    .replace(/^\s*>\s?/gm, '')
    .replace(/^\s*[-*+]\s+/gm, ' ')
    .replace(/^\s*\d+\.\s+/gm, ' ')
    .replace(/\|/g, ' ')
    .replace(/[#_*~]/g, ' ')
    .replace(/\s+/g, ' ')
    .trim();
}

function slugify(text) {
  return text
    .toLowerCase()
    .replace(/['"]/g, '')
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '');
}

function collectHeadings(source) {
  const tokens = md.parse(source, {});
  const headings = [];
  for (let i = 0; i < tokens.length - 1; i += 1) {
    const token = tokens[i];
    if (token.type !== 'heading_open' || (token.tag !== 'h2' && token.tag !== 'h3')) continue;
    const inline = tokens[i + 1];
    if (!inline || inline.type !== 'inline') continue;
    const text = inline.content.trim();
    if (!text) continue;
    headings.push({
      level: Number(token.tag.slice(1)),
      text,
      slug: slugify(text),
    });
  }
  return headings;
}

function addHeadingIds(html, headings) {
  let index = 0;
  return html.replace(/<h([23])>(.*?)<\/h\1>/g, (match, level, inner) => {
    const heading = headings[index];
    if (!heading || heading.level !== Number(level)) return match;
    index += 1;
    return `<h${level} id="${heading.slug}">${inner}</h${level}>`;
  });
}

function rewriteLinks(html) {
  return html
    .replaceAll('href="/getting-started/', 'href="/docs/getting-started/')
    .replaceAll('href="/concepts/', 'href="/docs/concepts/')
    .replaceAll('href="/api/', 'href="/docs/api/')
    .replaceAll('href="https://kronroe.dev#playground"', 'href="/#playground"');
}

function logoSvg() {
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 420 120" height="50" style="display:block;width:auto;" aria-label="Kronroe">
  <circle cx="48" cy="38" r="16" fill="#3EC9C9"/>
  <line x1="65" y1="38" x2="119" y2="38" stroke="#3EC9C9" stroke-width="3.5" stroke-linecap="round"/>
  <circle cx="136" cy="38" r="14" fill="none" stroke="#8BBF20" stroke-width="3.5"/>
  <circle cx="136" cy="38" r="5" fill="#8BBF20"/>
  <circle cx="48" cy="82" r="16" fill="#E87D4A"/>
  <line x1="65" y1="82" x2="119" y2="82" stroke="#E87D4A" stroke-width="3" stroke-linecap="round" stroke-dasharray="9 5"/>
  <circle cx="136" cy="82" r="14" fill="none" stroke="#7C5CFC" stroke-width="3.5"/>
  <circle cx="136" cy="82" r="5" fill="#7C5CFC"/>
  <text x="168" y="75" font-family="'Quicksand',system-ui,sans-serif" font-size="52" font-weight="700" letter-spacing="-1" fill="#FFFFFF">Kron</text>
  <text x="282" y="75" font-family="'Quicksand',system-ui,sans-serif" font-size="52" font-weight="700" letter-spacing="-1" fill="#E87D4A">roe</text>
</svg>`;
}

function renderSidebar(currentHref) {
  return sidebarSections.map((section) => {
    const items = section.items.map(([label, href]) => {
      const active = href === currentHref ? ' active' : '';
      return `<a class="docs-link${active}" href="${href}">${label}</a>`;
    }).join('');
    return `<section class="docs-nav-group">
      <h2>${section.title}</h2>
      <nav>${items}</nav>
    </section>`;
  }).join('');
}

function searchButton() {
  return `<button class="site-search" type="button" data-docs-search-open>
    <span>Search</span>
    <kbd>⌘K</kbd>
  </button>`;
}

function searchDialog() {
  return `<div class="docs-search" hidden data-docs-search-dialog>
    <div class="docs-search-backdrop" data-docs-search-close></div>
    <section class="docs-search-panel" role="dialog" aria-modal="true" aria-labelledby="docs-search-title">
      <div class="docs-search-header">
        <div>
          <div class="docs-search-kicker">Docs search</div>
          <h2 id="docs-search-title">Find a guide, concept, or API page</h2>
        </div>
        <button class="docs-search-close" type="button" data-docs-search-close aria-label="Close search">Close</button>
      </div>
      <label class="docs-search-field">
        <span class="sr-only">Search docs</span>
        <input id="docs-search-input" name="docs-search" type="search" placeholder="Search Kronroe docs" autocomplete="off" spellcheck="false" data-docs-search-input />
      </label>
      <div class="docs-search-meta" data-docs-search-meta>Type to search the full docs set.</div>
      <div class="docs-search-results" data-docs-search-results></div>
    </section>
  </div>`;
}

function renderToc(headings) {
  if (!headings.length) return '';
  const items = headings.map((heading) => (
    `<a class="docs-toc-link docs-toc-level-${heading.level}" href="#${heading.slug}">${heading.text}</a>`
  )).join('');
  return `<aside class="docs-toc" aria-label="On this page">
    <div class="docs-toc-title">On this page</div>
    <nav>${items}</nav>
  </aside>`;
}

function renderNextUp(currentSource) {
  const current = docsEntryBySource.get(currentSource);
  if (!current) return '';
  const next = docsEntries[current.index + 1];
  const prev = docsEntries[current.index - 1];
  const links = [];
  if (prev) {
    links.push(`<a class="docs-next-link docs-next-link-prev" href="/docs/${prev.source.replace(/\.md$/, '/')}" aria-label="Previous page">
      <span>Previous</span>
      <strong>${prev.title}</strong>
    </a>`);
  }
  if (next) {
    links.push(`<a class="docs-next-link docs-next-link-next" href="/docs/${next.source.replace(/\.md$/, '/')}" aria-label="Next page">
      <span>Next</span>
      <strong>${next.title}</strong>
    </a>`);
  }
  if (!links.length) return '';
  return `<section class="docs-next-up">
    <div class="docs-next-up-label">Continue reading</div>
    <div class="docs-next-up-grid">${links.join('')}</div>
  </section>`;
}

function wrapHtml({ title, intro, body, currentHref, sectionLabel, toc, nextUp }) {
  return `<!DOCTYPE html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <meta name="description" content="${intro || title}" />
    <link rel="canonical" href="https://kronroe.dev${currentHref}" />
    <link rel="icon" type="image/svg+xml" href="/favicon.svg" />
    <link rel="icon" type="image/x-icon" href="/favicon/favicon.ico" />
    <link rel="stylesheet" href="/page-shell.css" />
    <link rel="stylesheet" href="/docs-shell.css" />
    <title>${title} — Kronroe Docs</title>
  </head>
  <body class="docs-page">
    <header class="site-header docs-header">
      <a class="site-brand" href="/" aria-label="Kronroe home">${logoSvg()}</a>
      <span class="site-badge">Docs</span>
      <nav class="site-nav" aria-label="Docs navigation">
        <a href="/docs/">Home</a>
        <a href="/docs/getting-started/what-is-kronroe/">Guide</a>
        <a href="/docs/api/core/">API</a>
        <a href="/">kronroe.dev</a>
        <a href="https://github.com/kronroe/kronroe" target="_blank" rel="noopener noreferrer">GitHub</a>
      </nav>
      ${searchButton()}
      <a class="site-cta" href="/#playground">Try playground</a>
    </header>
    <main class="docs-layout">
      <aside class="docs-sidebar">
        <a class="docs-home-link" href="/docs/">Docs home</a>
        ${renderSidebar(currentHref)}
      </aside>
      <article class="docs-article">
        <div class="docs-kicker">${sectionLabel}</div>
        <h1>${title}</h1>
        ${intro ? `<p class="docs-intro">${intro}</p>` : ''}
        <div class="docs-content">${body}</div>
        ${nextUp}
      </article>
      ${toc}
    </main>
    <footer class="footer">
      <div class="footer-inner">
        <span>Kronroe · Docs</span>
        <div class="footer-links">
          <a href="https://github.com/kronroe/kronroe/releases" target="_blank" rel="noopener noreferrer">Releases</a>
          <a href="/community/">Community</a>
          <a href="/about/">About</a>
          <a href="/faq/">FAQ</a>
          <a href="/privacy/">Privacy</a>
          <a href="https://github.com/kronroe/kronroe" target="_blank" rel="noopener noreferrer">GitHub</a>
        </div>
      </div>
    </footer>
    ${searchDialog()}
    <script type="module" src="/docs-search.js"></script>
    <script type="module" src="/docs-tabs.js"></script>
  </body>
</html>`;
}

async function ensureDir(filePath) {
  await fs.mkdir(path.dirname(filePath), { recursive: true });
}

async function writePage(sourcePath) {
  const raw = await fs.readFile(sourcePath, 'utf8');
  const stripped = stripFrontMatter(raw);
  const { source: withoutTabs, blocks: tabBlocks } = extractDocsTabs(stripped);
  const title = extractTitle(withoutTabs, path.basename(sourcePath, '.md'));
  const intro = extractIntro(withoutTabs);
  const headings = collectHeadings(withoutTabs);
  const bodySource = withoutTabs.replace(/^#\s+.+\n+/, '');
  let rendered = rewriteLinks(md.render(bodySource));
  rendered = addHeadingIds(rendered, headings);
  tabBlocks.forEach(({ token, block }) => {
    rendered = rendered.replace(new RegExp(`<p>${token}<\\/p>`, 'g'), block);
    rendered = rendered.replaceAll(token, block);
  });
  const relative = path.relative(docsDir, sourcePath).replace(/\\/g, '/').replace(/\.md$/, '');
  if (relative === 'index') return;
  const outputPath = path.join(docsDir, relative, 'index.html');
  const sectionLabel = relative.startsWith('getting-started/')
    ? 'Getting Started'
    : relative.startsWith('concepts/')
      ? 'Core Concepts'
      : 'API Reference';
  const currentSource = `${relative}.md`;
  await ensureDir(outputPath);
  await fs.writeFile(outputPath, wrapHtml({
    title,
    intro,
    body: rendered,
    currentHref: `/docs/${relative}/`,
    sectionLabel,
    toc: renderToc(headings),
    nextUp: renderNextUp(currentSource),
  }), 'utf8');
}

async function writeSearchIndex() {
  const entries = await Promise.all(docsEntries.map(async (entry) => {
    const sourcePath = path.join(docsDir, entry.source);
    const raw = await fs.readFile(sourcePath, 'utf8');
    const stripped = stripFrontMatter(raw);
    const { source: withoutTabs } = extractDocsTabs(stripped);
    const title = extractTitle(withoutTabs, entry.title);
    const intro = extractIntro(withoutTabs);
    const headings = collectHeadings(withoutTabs).map((heading) => heading.text);
    const bodyText = markdownToPlainText(withoutTabs);
    return {
      href: `/docs/${entry.source.replace(/\.md$/, '/')}`,
      title,
      section: entry.label,
      intro,
      headings,
      excerpt: bodyText.slice(0, 260),
      bodyText,
    };
  }));
  const outputPath = path.join(rootDir, 'public', 'docs-search.json');
  await fs.writeFile(outputPath, JSON.stringify(entries, null, 2), 'utf8');
}

async function clearGeneratedPages() {
  const targets = docsEntries.map((entry) => {
    const relative = entry.source.replace(/\.md$/, '');
    return path.join(docsDir, relative, 'index.html');
  });
  await Promise.all(targets.map(async (file) => {
    try {
      await fs.unlink(file);
    } catch {}
  }));
}

async function main() {
  await clearGeneratedPages();
  await writeSearchIndex();
  const files = docsEntries.map((entry) => path.join(docsDir, entry.source));
  for (const file of files) {
    await writePage(file);
  }
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
