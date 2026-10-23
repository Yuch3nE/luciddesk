(() => {
  if (window.__luciddeskNavigation) return;
  window.__luciddeskNavigation = true;
  function sync() {
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
  for (const event of ['hashchange', 'popstate']) {
    window.addEventListener(event, sync);
  }
  // Language swap events are dispatched on document without bubbling.
  document.addEventListener('luciddesk:language-swapped', sync);
  document.addEventListener('astro:page-load', sync);
  sync();
})();
