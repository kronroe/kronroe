function activateTabset(tabset, nextIndex) {
  const tabs = Array.from(tabset.querySelectorAll('[role="tab"]'));
  const panels = Array.from(tabset.querySelectorAll('[role="tabpanel"]'));

  tabs.forEach((tab, index) => {
    const selected = index === nextIndex;
    tab.setAttribute('aria-selected', String(selected));
    tab.tabIndex = selected ? 0 : -1;
  });

  panels.forEach((panel, index) => {
    panel.hidden = index !== nextIndex;
  });
}

function initTabset(tabset) {
  const tabs = Array.from(tabset.querySelectorAll('[role="tab"]'));
  if (!tabs.length) return;

  tabs.forEach((tab, index) => {
    tab.addEventListener('click', () => activateTabset(tabset, index));
    tab.addEventListener('keydown', (event) => {
      const currentIndex = tabs.indexOf(tab);
      if (event.key === 'ArrowRight' || event.key === 'ArrowLeft') {
        event.preventDefault();
        const direction = event.key === 'ArrowRight' ? 1 : -1;
        const nextIndex = (currentIndex + direction + tabs.length) % tabs.length;
        tabs[nextIndex].focus();
        activateTabset(tabset, nextIndex);
      }
    });
  });

  activateTabset(tabset, 0);
}

document.querySelectorAll('[data-docs-tabs]').forEach(initTabset);
