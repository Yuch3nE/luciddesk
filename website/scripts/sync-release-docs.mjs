import { readFile, writeFile, copyFile, mkdir } from 'node:fs/promises';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { resolve, dirname } from 'node:path';
import { pathToFileURL } from 'node:url';

export const documents = ['CHANGELOG.md', 'CHANGELOG.en.md', 'PRIVACY.md', 'PRIVACY.en.md',
  'docs/usage.md', 'docs/usage.en.md', 'docs/cli.md', 'docs/installer.md', 'docs/portable.md', 'docs/package.md'];
export async function syncReleaseDocs(source, targetRoot, tag, expectedCommit = '') {
  if (!/^v?\d+\.\d+\.\d+$/.test(tag ?? '')) throw new Error('Invalid release tag');
  const sha = execFileSync('git', ['-C', source, 'rev-parse', 'HEAD'], {encoding:'utf8'}).trim();
  if (expectedCommit && sha !== expectedCommit) throw new Error('Successful build does not match latest release; refusing stale deployment');
  const cargo = await readFile(resolve(source, 'Cargo.toml'), 'utf8');
  const version = tag.replace(/^v/, '');
  const workspace = cargo.split('[workspace.package]')[1]?.split(/\n\[/)[0];
  if (!workspace?.includes(`version = "${version}"`)) throw new Error('Release tag and source version differ');
  // Validate every source before replacing any build input.
  const entries = await Promise.all(documents.map(async file => {
    const bytes = await readFile(resolve(source, file));
    if (file.startsWith('CHANGELOG') && !bytes.toString().includes(`## ${version} ·`)) throw new Error(`Missing release notes: ${file}`);
    return [file, bytes];
  }));
  const hashes = {};
  for (const [file, bytes] of entries) {
    const target = resolve(targetRoot, file);
    await mkdir(dirname(target), {recursive:true});
    await copyFile(resolve(source, file), target);
    hashes[file] = createHash('sha256').update(bytes).digest('hex');
  }
  const provenance = resolve(targetRoot, 'website/public/release.json');
  await mkdir(dirname(provenance), {recursive:true});
  await writeFile(provenance, JSON.stringify({tag, commit:sha, documents:hashes}, null, 2) + '\n');
  console.log(`Imported documentation from ${tag} (${sha}) without modifying the release source.`);
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  await syncReleaseDocs(resolve('../release-source'), resolve('..'), process.env.RELEASE_TAG, process.env.SOURCE_SHA);
}
