// Swap translated content in place. The document, URL, theme and reading position stay put.
(() => {
  if (window.__luciddeskLanguage) return;
  window.__luciddeskLanguage = true;
  let navigation = 0;
  const storageKey = 'luciddesk-language';
  const cache = new Map();
  let busy = false;
  const current = () => document.documentElement.lang === 'en' ? 'en' : 'zh';
  const payloadUrl = (lang, pathname) => {
    const path = pathname.endsWith('/') ? `${pathname}index` : pathname.replace(/\.html$/, '');
    return `/_languages/${lang}${path}.json`;
  };
  async function load(lang, pathname = location.pathname) {
    const url = payloadUrl(lang, pathname);
    if (cache.has(url)) return cache.get(url);
    const response = await fetch(url, {signal: AbortSignal.timeout(10000)});
    if (!response.ok) throw new Error('Language content unavailable');
    const payload = await response.json();
    if (payload.lang !== lang || typeof payload.body !== 'string' || typeof payload.head !== 'string') throw new Error('Invalid language content');
    cache.set(url, payload);
    return payload;
  }
  function status(message) {
    let region = document.getElementById('language-status');
    if (!region) {
      region = document.createElement('p');
      region.id = 'language-status';
      region.setAttribute('role', 'status');
      region.className = 'fixed bottom-4 left-4 right-4 z-50 rounded-xl border border-line bg-page p-4 text-sm text-default shadow-lg';
      document.body.append(region);
    }
    region.textContent = message;
  }
  async function switchLanguage(lang, manual = true) {
    if (busy || lang === current() || !['zh', 'en'].includes(lang)) return;
    busy = true;
    const button = document.querySelector('[data-language-switch]');
    button?.setAttribute('aria-busy', 'true');
    button?.setAttribute('disabled', '');
    try {
      const generation = navigation;
      const payload = await load(lang);
      if (generation !== navigation) return;
      const y = window.scrollY;
      const section = [...document.querySelectorAll('main section[id], main [data-slot="accordion-item"][id]')].reverse().find(el => el.getBoundingClientRect().top <= 160);
      const anchor = section ? {id: section.id, offset: section.getBoundingClientRect().top} : null;
      const states = new Map([...document.querySelectorAll('astro-island')].map(island => [island.getAttribute('component-url'), [...island.querySelectorAll('[data-slot="accordion-trigger"]')].map(trigger => trigger.getAttribute('aria-expanded') === 'true')]));
      const restorePosition = () => {
        const target = anchor && document.getElementById(anchor.id);
        window.scrollTo({top: target ? window.scrollY + target.getBoundingClientRect().top - anchor.offset : y, behavior:'instant'});
      };
      // New Astro islands hydrate normally; restore any open FAQ/version sections afterward.
      const onHydrate = event => {
        const island = event.target;
        const open = states.get(island.getAttribute?.('component-url'));
        if (open?.length) {
          island.querySelectorAll('[data-slot="accordion-trigger"]').forEach((trigger, index) => {
            if ((trigger.getAttribute('aria-expanded') === 'true') !== open[index]) trigger.click();
          });
          requestAnimationFrame(restorePosition);
        }
      };
      document.addEventListener('astro:hydrate', onHydrate, true);
      setTimeout(() => document.removeEventListener('astro:hydrate', onHydrate, true), 3000);
      const parsed = new DOMParser().parseFromString(`<head>${payload.head}</head><body>${payload.body}</body>`, 'text/html');
      document.title = parsed.title;
      for (const selector of ['meta[name="description"]', 'meta[property="og:title"]', 'meta[property="og:description"]']) {
        const content = parsed.querySelector(selector)?.getAttribute('content');
        if (content) document.querySelector(selector)?.setAttribute('content', content);
      }
      document.documentElement.lang = lang === 'en' ? 'en' : 'zh-CN';
      // Replacing translated markup is not a new navigation. Do not replay
      // first-paint CSS animation on content the visitor has already seen.
      document.body.setAttribute('data-motion-suppressed', '');
      document.body.replaceChildren(...parsed.body.childNodes);
      document.dispatchEvent(new Event('luciddesk:language-swapped'));
      document.dispatchEvent(new Event('luciddesk:theme-updated'));
      if (manual) {
        try { localStorage.setItem(storageKey, lang); } catch { /* This page can still switch. */ }
      }
      restorePosition();
      if (manual) document.querySelector('[data-language-switch]')?.focus({preventScroll:true});
    } catch {
      status(current() === 'en' ? 'Could not switch language. Please try again.' : '语言切换暂时失败，请重试。');
    } finally {
      busy = false;
      document.querySelector('[data-language-switch]')?.removeAttribute('aria-busy');
      document.querySelector('[data-language-switch]')?.removeAttribute('disabled');
    }
  }
  // Prepare translated content before Astro swaps documents, avoiding a Chinese flash.
  document.addEventListener('astro:before-preparation', event => {
    navigation++;
    const lang = current();
    const loader = event.loader;
    event.loader = async () => {
      await loader();
      if (event.signal.aborted || lang !== 'en' || !event.newDocument) return;
      try {
        const payload = await load(lang, event.to.pathname);
        if (event.signal.aborted) return;
        const parsed = new DOMParser().parseFromString(`<head>${payload.head}</head><body>${payload.body}</body>`, 'text/html');
        event.newDocument.body.replaceChildren(...parsed.body.childNodes);
        event.newDocument.documentElement.lang = 'en';
        event.newDocument.title = parsed.title;
        for (const selector of ['meta[name="description"]', 'meta[property="og:title"]', 'meta[property="og:description"]']) {
          const content = parsed.querySelector(selector)?.getAttribute('content');
          if (content) event.newDocument.querySelector(selector)?.setAttribute('content', content);
        }
      } catch {
        // Let the ordinary document remain navigable if language data is unavailable.
      }
    };
  });
  document.addEventListener('click', event => {
    const button = event.target.closest?.('[data-language-switch]');
    if (button) switchLanguage(button.dataset.languageTarget);
  });
  function preferredLanguage() {
    try {
      const saved = localStorage.getItem(storageKey);
      if (saved === 'en' || saved === 'zh') return saved;
    } catch { /* System language also works without browser storage. */ }
    // Use the primary browser/system language, not a secondary Chinese language.
    // An explicit script takes precedence over the region (e.g. zh-Hant-CN).
    const tag = navigator.languages?.[0] || navigator.language || '';
    try {
      const locale = new Intl.Locale(tag);
      if (locale.language === 'zh' &&
          (locale.script === 'Hans' || (!locale.script && ['CN', 'SG'].includes(locale.region)))) return 'zh';
    } catch { /* Unknown language tags fall back to English. */ }
    return 'en';
  }
  switchLanguage(preferredLanguage(), false);
})();
