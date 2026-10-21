// Progressive enhancement: content stays visible if JavaScript or animation APIs fail.
(() => {
  const preference = matchMedia('(prefers-reduced-motion: reduce)');
  if (!('IntersectionObserver' in window) || !Element.prototype.animate) return;
  const running = new Map();
  const reveal = (element) => {
    if (preference.matches || element.contains(document.activeElement)) return;
    const animation = element.animate([
      { opacity: 0, transform: 'translateY(18px)' },
      { opacity: 1, transform: 'translateY(0)' },
    ], {
      duration: 520,
      delay: Math.min(Number(element.dataset.motionOrder) || 0, 3) * 70,
      easing: 'cubic-bezier(.22, 1, .36, 1)',
      fill: 'backwards',
    });
    running.set(element, animation);
    animation.finished.catch(() => {}).finally(() => running.delete(element));
  };
  const observer = new IntersectionObserver((entries) => {
    for (const entry of entries) {
      if (!entry.isIntersecting) continue;
      observer.unobserve(entry.target);
      reveal(entry.target);
    }
  }, { threshold: 0.08 });
  document.querySelectorAll('[data-motion]').forEach(element => observer.observe(element));
  preference.addEventListener('change', () => {
    if (preference.matches) running.forEach(animation => animation.cancel());
  });
  // Keyboard navigation must never land on temporarily transparent content.
  document.addEventListener('focusin', event => {
    running.forEach((animation, element) => {
      if (element.contains(event.target)) animation.cancel();
    });
  });
})();
