import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { runInNewContext } from 'node:vm';

const source = readFileSync(new URL('../public/language.js', import.meta.url), 'utf8');
function requestedLanguage(languages, { saved = null, storageBlocked = false, language = '' } = {}) {
  const requests = [];
  runInNewContext(source, {
    window: {}, navigator: { languages, language }, Intl, AbortSignal,
    location: { pathname: '/guide/' },
    document: {
      documentElement: { lang: 'zh-CN' },
      addEventListener() {}, querySelector() { return null; },
    },
    localStorage: {
      getItem() { if (storageBlocked) throw new Error('blocked'); return saved; },
      setItem() { throw new Error('Automatic selection must not save a preference'); },
    },
    fetch(url) {
      requests.push(url);
      // Initial selection is all this test needs; leave loading pending.
      return new Promise(() => {});
    },
  });
  return requests.length ? 'en' : 'zh';
}

for (const tag of ['zh-CN', 'zh-SG', 'zh-Hans', 'zh-Hans-HK']) {
  test(`${tag} defaults to simplified Chinese`, () => assert.equal(requestedLanguage([tag]), 'zh'));
}
for (const tag of ['en-US', 'ja-JP', 'de-DE', 'zh-TW', 'zh-HK', 'zh-MO', 'zh-Hant-CN', 'zh', 'invalid_tag', '']) {
  test(`${tag || 'missing language'} defaults to English`, () => assert.equal(requestedLanguage([tag]), 'en'));
}
test('secondary Chinese language does not override primary English', () => {
  assert.equal(requestedLanguage(['en-US', 'zh-CN']), 'en');
});
test('explicit preference wins in both directions', () => {
  assert.equal(requestedLanguage(['en-US'], { saved: 'zh' }), 'zh');
  assert.equal(requestedLanguage(['zh-CN'], { saved: 'en' }), 'en');
});
test('blocked or invalid storage falls back to system language', () => {
  assert.equal(requestedLanguage(['zh-CN'], { storageBlocked: true }), 'zh');
  assert.equal(requestedLanguage(['en-US'], { storageBlocked: true }), 'en');
  assert.equal(requestedLanguage(['zh-CN'], { saved: 'invalid' }), 'zh');
});
test('navigator.language is used when languages is unavailable', () => {
  assert.equal(requestedLanguage(undefined, { language: 'zh-CN' }), 'zh');
});
