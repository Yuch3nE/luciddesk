import assert from 'node:assert/strict';
import { readFile, access } from 'node:fs/promises';
import { resolve } from 'node:path';

const legacy = ['first-panel', 'panels', 'organize', 'search', 'preview', 'shortcuts', 'backup', 'troubleshooting'];
for (const [language, source] of [['zh', '../docs/usage.md'], ['en', '../docs/usage.en.md']]) {
  const markdown = await readFile(source, 'utf8');
  const payload = JSON.parse(await readFile(`dist/_languages/${language}/guide/index.json`, 'utf8'));
  const html = payload.body;
  const ids = [...html.matchAll(/\bid="([^"]+)"/g)].map(match => match[1]);
  assert.equal(new Set(ids).size, ids.length, `${language}: duplicate anchors`);
  for (const id of legacy) assert.ok(ids.includes(id), `${language}: missing legacy anchor ${id}`);
  const headings = [...markdown.matchAll(/^## (.+)$/gm)];
  assert.equal([...html.matchAll(/<section\b/g)].length, headings.length, `${language}: missing chapters`);
  assert.ok(html.includes('0.22.2') && html.includes(language === 'zh' ? '未发布' : 'Unreleased'));
  for (const [, target] of html.matchAll(/\bhref="#([^"]+)"/g)) {
    assert.ok(ids.includes(decodeURIComponent(target)), `${language}: broken chapter link ${target}`);
  }
  for (const [, target] of html.matchAll(/\b(?:href|src)="(\/[^"#?]*)[^"]*"/g)) {
    if (target.startsWith('//')) continue;
    const local = target.endsWith('/') ? `${target}index.html` : target;
    await access(resolve('dist', '.' + local));
  }
  assert.ok(!/href="(?:\.\.\/|[^/"#:]+\.md)/.test(html), `${language}: unresolved Markdown link`);
  console.log(`Manual ${language}: ${headings.length} chapters, legacy anchors, local links and release labels verified.`);
}
