// Run before styles paint so a saved theme never flashes the opposite palette.
(() => {
  if (window.__luciddeskTheme) return;
  window.__luciddeskTheme = true;
  const system = window.matchMedia('(prefers-color-scheme: dark)');
  const valid = ['system', 'light', 'dark'];
  let preference = 'system';
  try {
    const saved = localStorage.getItem('luciddesk-theme');
    if (valid.includes(saved)) preference = saved;
  } catch { /* The system theme also works when browser storage is unavailable. */ }

  function apply() {
    const theme = preference === 'system' ? (system.matches ? 'dark' : 'light') : preference;
    document.documentElement.classList.toggle('dark', theme === 'dark');
    document.documentElement.dataset.theme = theme;
    document.querySelector('meta[name="theme-color"]')?.setAttribute('content', theme === 'dark' ? '#141c28' : '#f8fafc');
    document.documentElement.dataset.themePreference = preference;
    document.dispatchEvent(new Event('luciddesk:theme-updated'));
  }

  document.addEventListener('astro:before-swap', event => {
    const root = event.newDocument.documentElement;
    root.classList.toggle('dark', document.documentElement.classList.contains('dark'));
    root.dataset.theme = document.documentElement.dataset.theme;
    root.dataset.themePreference = preference;
  });
  document.addEventListener('astro:after-swap', apply);
  apply();
  system.addEventListener('change', apply);
  document.addEventListener('luciddesk:theme-change', (event) => {
    if (!valid.includes(event.detail)) return;
    preference = event.detail;
    try { localStorage.setItem('luciddesk-theme', preference); } catch { /* Keep this tab usable. */ }
    apply();
  });
  window.addEventListener('storage', (event) => {
    if (event.key === 'luciddesk-theme' || event.key === null) {
      preference = valid.includes(event.newValue) ? event.newValue : 'system';
      apply();
    }
  });
})();
