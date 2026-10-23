import { readFile } from 'node:fs/promises';
import assert from 'node:assert/strict';
const expected = JSON.parse(await readFile('public/release.json', 'utf8'));
const origin = 'https://luciddesk.bbkingdom365.com';
let lastError;
for (let attempt = 0; attempt < 5; attempt++) {
  try {
    const response = await fetch(`${origin}/release.json?verify=${Date.now()}`, {signal:AbortSignal.timeout(15000)});
    assert.ok(response.ok);
    assert.deepEqual(await response.json(), expected);
    for (const path of ['/guide/', '/agent/', `/changelog/${expected.tag.replace(/^v/, '')}/`, '/_languages/en/guide/index.json']) {
      const page = await fetch(origin + path, {signal:AbortSignal.timeout(15000)});
      assert.ok(page.ok, `${path}: ${page.status}`);
    }
    console.log(`Verified live ${expected.tag} and bilingual documentation.`);
    process.exit(0);
  } catch (error) { lastError = error; }
  if (attempt < 4) await new Promise(resolve => setTimeout(resolve, 5000));
}
throw lastError;
