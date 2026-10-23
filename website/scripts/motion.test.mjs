import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { runInNewContext } from 'node:vm';

const source = readFileSync(new URL('../public/motion.js', import.meta.url), 'utf8');

function setup(reduced = false) {
  const listeners = new Map();
  const observed = new Set();
  const animations = [];
  let callback;
  let preferenceChanged;
  const preference = { matches: reduced, addEventListener(_, fn) { preferenceChanged = fn; } };
  const element = top => ({
    dataset: {}, isConnected: true,
    getBoundingClientRect: () => ({ top, bottom: top + 100 }),
    contains: () => false,
    animate() {
      const animation = { cancelled: false, cancel() { this.cancelled = true; }, finished: new Promise(() => {}) };
      animations.push(animation);
      return animation;
    },
  });
  const visible = element(100);
  const below = element(1000);
  const document = {
    hidden: false, activeElement: null,
    querySelectorAll: () => [visible, below],
    addEventListener(name, fn) { listeners.set(name, fn); },
  };
  class IntersectionObserver {
    constructor(fn) { callback = fn; }
    observe(target) { observed.add(target); }
    unobserve(target) { observed.delete(target); }
    disconnect() { observed.clear(); }
  }
  const context = {
    window: { innerHeight: 800, IntersectionObserver }, document,
    Element: { prototype: { animate() {} } },
    IntersectionObserver, matchMedia: () => preference,
  };
  runInNewContext(source, context);
  return { context, visible, below, observed, animations,
    enter: target => callback([{ target, isIntersecting: true }]),
    emit: (name, event) => listeners.get(name)(event),
    hide() { document.hidden = true; listeners.get('visibilitychange')(); },
    reduce() { preference.matches = true; preferenceChanged(); },
  };
}

test('late script initialization never hides content already in the viewport', () => {
  const state = setup();
  assert.equal(state.observed.has(state.visible), false);
  assert.equal(state.observed.has(state.below), true);
  state.enter(state.below);
  assert.equal(state.animations.length, 1);
  state.emit('astro:page-load');
  assert.equal(state.observed.size, 0);
});

test('language swaps cancel old animations and skip visible content', () => {
  const state = setup();
  state.enter(state.below);
  state.emit('luciddesk:language-swapped');
  assert.equal(state.animations[0].cancelled, true);
  assert.equal(state.observed.has(state.visible), false);
});

test('reduced motion prevents scroll animations', () => {
  const state = setup(true);
  state.enter(state.below);
  assert.equal(state.animations.length, 0);
});

test('navigation resets observation without animating visible content', () => {
  const state = setup();
  state.enter(state.below);
  state.emit('astro:before-swap');
  assert.equal(state.animations[0].cancelled, true);
  state.emit('astro:after-swap');
  assert.equal(state.observed.has(state.visible), false);
  assert.equal(state.observed.has(state.below), true);
});

test('repeated script execution does not create duplicate observers', () => {
  const state = setup();
  runInNewContext(source, state.context);
  state.enter(state.below);
  assert.equal(state.animations.length, 1);
});

test('switching to reduced motion cancels animations already running', () => {
  const state = setup();
  state.enter(state.below);
  state.reduce();
  assert.equal(state.animations[0].cancelled, true);
});

test('backgrounding the page cancels animation and prevents new motion', () => {
  const state = setup();
  state.enter(state.below);
  state.hide();
  assert.equal(state.animations[0].cancelled, true);
  state.enter(state.below);
  assert.equal(state.animations.length, 1);
});

test('keyboard focus immediately reveals animated content', () => {
  const state = setup();
  state.enter(state.below);
  state.below.contains = () => true;
  state.emit('focusin', { target: {} });
  assert.equal(state.animations[0].cancelled, true);
});
