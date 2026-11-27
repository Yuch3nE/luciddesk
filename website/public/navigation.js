(() => {
  if (window.__luciddeskNavigation) return;
  window.__luciddeskNavigation = true;
  let tocObserver = null;

  function syncHeader() {
    const header = document.querySelector('.site-header');
    if (header) {
      header.toggleAttribute('data-scrolled', window.scrollY > 12);
    }
  }

  function syncNav() {
    const path = location.pathname.replace(/^\/en(?=\/|$)/, '') || '/';
    document.querySelectorAll('.site-nav a').forEach(link => {
      const url = new URL(link.href, location.href);
      const page = link.dataset.page;
      const isPage = page && (path === page || path.startsWith(page + '/'));
      const isSection = path === '/' && url.hash && url.hash === location.hash;
      if (isPage || isSection) link.setAttribute('aria-current', isSection ? 'location' : 'page');
      else link.removeAttribute('aria-current');
    });
  }

  function setActiveToc(id) {
    document.querySelectorAll('.help-toc a[href^="#"]').forEach(link => {
      if (link.getAttribute('href') === `#${id}`) link.setAttribute('aria-current', 'location');
      else link.removeAttribute('aria-current');
    });
  }

  function observeToc() {
    if (tocObserver) {
      tocObserver.disconnect();
      tocObserver = null;
    }
    const sections = [...document.querySelectorAll('.help-section[id]')];
    if (!sections.length) return;
    const hashId = decodeURIComponent(location.hash.replace(/^#/, ''));
    const initial = sections.find(section => section.id === hashId) || sections[0];
    if (initial) setActiveToc(initial.id);
    if (!('IntersectionObserver' in window)) return;
    const visible = new Set();
    tocObserver = new IntersectionObserver(entries => {
      for (const entry of entries) {
        if (entry.isIntersecting) visible.add(entry.target);
        else visible.delete(entry.target);
      }
      const active = sections.find(section => visible.has(section));
      if (active) setActiveToc(active.id);
    }, { rootMargin: '-18% 0px -65% 0px', threshold: 0 });
    sections.forEach(section => tocObserver.observe(section));
  }

  function sync() {
    syncNav();
    syncHeader();
    observeToc();
  }

  for (const event of ['hashchange', 'popstate']) {
    window.addEventListener(event, sync);
  }
  window.addEventListener('scroll', syncHeader, { passive: true });
  document.addEventListener('astro:before-swap', () => {
    if (tocObserver) {
      tocObserver.disconnect();
      tocObserver = null;
    }
  });
  // Language swap events are dispatched on document without bubbling.
  document.addEventListener('luciddesk:language-swapped', sync);
  document.addEventListener('astro:page-load', sync);
  sync();
})();
