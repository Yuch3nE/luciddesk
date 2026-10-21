import { writeFile, readFile } from 'node:fs/promises';
const destination = new URL('../src/data/repository.json', import.meta.url);
const base = 'https://api.github.com/repos/Yuch3nE/luciddesk';
try {
  const responses = await Promise.all([base, `${base}/releases/latest`].map(url => fetch(url, {
    headers: { Accept: 'application/vnd.github+json', 'User-Agent': 'LucidDesk-Website' },
    signal: AbortSignal.timeout(10000),
  })));
  for (const response of responses) if (!response.ok) throw new Error(`GitHub HTTP ${response.status}`);
  const [repo, release] = await Promise.all(responses.map(response => response.json()));
  if (!Number.isInteger(repo.stargazers_count) || typeof release.tag_name !== 'string') throw new Error('Invalid GitHub response');
  await writeFile(destination, JSON.stringify({name: repo.full_name, stars: repo.stargazers_count, version: release.tag_name, updatedAt: new Date().toISOString()}, null, 2) + '\n');
  console.log(`GitHub: ${release.tag_name}, ${repo.stargazers_count} stars`);
} catch (error) {
  const cached = JSON.parse(await readFile(destination, 'utf8'));
  console.warn(`GitHub refresh unavailable (${error.message}); using snapshot from ${cached.updatedAt}.`);
}
