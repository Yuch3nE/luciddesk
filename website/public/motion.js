// Progressive enhancement: the base document is always visible and usable.
(() => {
  if (window.__luciddeskMotion) return;
  window.__luciddeskMotion = true;
  const preference = matchMedia('(prefers-reduced-motion: reduce)');
  if (!('IntersectionObserver' in window) || !Element.prototype.animate) return;
  const running = new Map();
  let seen = new WeakSet();
  const cancel = () => {
    running.forEach(animation => animation.cancel());
    running.clear();
  };
  const animate = (element, delay, illustration = false) => {
    if (preference.matches || document.hidden || element.contains(document.activeElement)) return;
    const animation = element.animate([
      { opacity: 0, transform: illustration ? 'translateY(12px) scale(.985)' : 'translateY(12px)' },
      { opacity: 1, transform: 'translateY(0) scale(1)' },
    ], {
      duration: illustration ? 700 : 480,
      delay,
      easing: 'cubic-bezier(.22, 1, .36, 1)',
      fill: 'backwards',
    });
    running.set(element, animation);
    animation.finished.catch(() => {}).finally(() => {
      if (running.get(element) === animation) running.delete(element);
    });
  };
  const observer = new IntersectionObserver((entries) => {
    for (const entry of entries) {
      if (!entry.isIntersecting) continue;
      const element = entry.target;
      observer.unobserve(element);
      if (element.dataset.motion === 'sequence') {
        element.querySelectorAll('[data-motion-stage]').forEach((stage, index) => {
          animate(stage, Math.min(index, 4) * 65);
        });
      } else {
        animate(element, Math.min(Number(element.dataset.motionOrder) || 0, 3) * 65,
          element.dataset.motion === 'illustration');
      }
    }
  }, { threshold: 0.08 });
  const observe = () => {
    // Language switching replaces the body without a full page load.
    running.forEach((animation, element) => {
      if (!element.isConnected) { animation.cancel(); running.delete(element); }
    });
    document.querySelectorAll('[data-motion]').forEach(element => {
      if (seen.has(element)) return;
      seen.add(element);
      // Never hide content that was already visible before this script or a
      // language payload arrived. Only animate later scroll entrances.
      const bounds = element.getBoundingClientRect();
      if (bounds.bottom > 0 && bounds.top < window.innerHeight) return;
      observer.observe(element);
    });
  };
  document.addEventListener('astro:before-swap', () => {
    observer.disconnect();
    cancel();
    seen = new WeakSet();
  });
  document.addEventListener('astro:after-swap', () => {
    observer.disconnect();
    seen = new WeakSet();
    observe();
  });
  document.addEventListener('luciddesk:language-swapped', () => {
    observer.disconnect();
    cancel();
    seen = new WeakSet();
    observe();
  });
  document.addEventListener('astro:page-load', observe);
  observe();
  preference.addEventListener('change', () => { if (preference.matches) cancel(); });
  document.addEventListener('visibilitychange', () => { if (document.hidden) cancel(); });
  // Keyboard focus must never land on temporarily transparent content.
  document.addEventListener('focusin', event => {
    running.forEach((animation, element) => {
      if (element.contains(event.target)) animation.cancel();
    });
  });
})();
