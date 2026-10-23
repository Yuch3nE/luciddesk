import { writeFile, readFile } from 'node:fs/promises';
import { pathToFileURL } from 'node:url';

export function validateRelease(repo, release, requested = '') {
  if (repo.full_name !== 'Yuch3nE/luciddesk' || !Number.isInteger(repo.stargazers_count)) throw new Error('Invalid repository');
  if (!/^v?\d+\.\d+\.\d+$/.test(release.tag_name) || release.draft || release.prerelease || !release.published_at) throw new Error('Expected a published stable release');
  if (requested && release.tag_name !== requested) throw new Error('Requested release is not the latest stable release; refusing to deploy older content');
  const version = release.tag_name.replace(/^v/, '');
  const assets = release.assets ?? [];
  const names = new Set(assets.filter(a => a.state === 'uploaded' && a.size > 0).map(a => a.name));
  const required = [`LucidDesk-${version}-windows-x64-setup.exe`, `LucidDesk-${version}-windows-x64-portable.zip`];
  const ordinary = [...names].filter(n => new RegExp(`^LucidDesk-${version.replaceAll('.', '\\.')}-[0-9a-f]{7,40}-windows-x64-[0-9-]+\\.zip$`).test(n));
  if (ordinary.length !== 1) throw new Error('Expected one ordinary ZIP');
  for (const name of [...required, ...ordinary]) {
    if (!names.has(name) || !names.has(`${name}.sha256`)) throw new Error(`Missing release asset or checksum: ${name}`);
  }
  return { name: repo.full_name, stars: repo.stargazers_count, version: release.tag_name, updatedAt: new Date().toISOString() };
}

async function main() {
  const destination = new URL('../src/data/repository.json', import.meta.url);
  const base = 'https://api.github.com/repos/Yuch3nE/luciddesk';
  const strict = process.env.RELEASE_DEPLOY === '1';
  try {
    const headers = { Accept: 'application/vnd.github+json', 'User-Agent': 'LucidDesk-Website' };
    if (process.env.GH_TOKEN) headers.Authorization = `Bearer ${process.env.GH_TOKEN}`;
    const responses = await Promise.all([base, `${base}/releases/latest`].map(url => fetch(url, {headers, signal: AbortSignal.timeout(15000)})));
    for (const response of responses) if (!response.ok) throw new Error(`GitHub HTTP ${response.status}`);
    const [repo, release] = await Promise.all(responses.map(response => response.json()));
    const metadata = validateRelease(repo, release, process.env.RELEASE_TAG ?? '');
    await writeFile(destination, JSON.stringify(metadata, null, 2) + '\n');
    if (process.env.GITHUB_OUTPUT) {
      const { appendFile } = await import('node:fs/promises');
      await appendFile(process.env.GITHUB_OUTPUT, `tag=${release.tag_name}\n`);
    }
    console.log(`GitHub: ${release.tag_name}, ${repo.stargazers_count} stars`);
  } catch (error) {
    if (strict) throw error;
    const cached = JSON.parse(await readFile(destination, 'utf8'));
    console.warn(`GitHub refresh unavailable (${error.message}); using snapshot from ${cached.updatedAt}.`);
  }
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) await main();
