import { readFile, writeFile, mkdir } from 'node:fs/promises';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { resolve, dirname } from 'node:path';
import { pathToFileURL } from 'node:url';

export const documents = ['CHANGELOG.md', 'CHANGELOG.en.md', 'PRIVACY.md', 'PRIVACY.en.md',
  'docs/usage.md', 'docs/usage.en.md', 'docs/cli.md', 'docs/installer.md', 'docs/portable.md', 'docs/package.md'];
export function validateManualPair(manuals, version) {
  const versions = manuals.map(text => text.match(/^<!-- manual-version: (\d+\.\d+\.\d+) -->/m)?.[1]);
  if (versions.every(value => !value)) return false;
  if (!versions.every(value => value === version)) throw new Error('Both manual versions must match the release');
  if (manuals.some(text => !/^# .+/m.test(text) || !/^## .+/m.test(text))) throw new Error('Manual content is incomplete');
  return true;
}
// Only the matching release section may be corrected; future releases and older
// history from main must never leak into a version-matched deployment.
export function correctReleaseNotes(original, corrected, version) {
  const section = text => {
    const headings = [...text.matchAll(/^##[ \t]+([^\r\n]+)\r?$/gm)];
    const matches = headings.flatMap((heading, index) => {
      if (heading[1].trim().split(/\s/)[0] !== version) return [];
      const end = headings[index + 1]?.index ?? text.length;
      return [{start: heading.index, end, body: text.slice(heading.index + heading[0].length, end).trim()}];
    });
    if (matches.length !== 1 || !matches[0].body) throw new Error(`Expected one nonempty changelog section for ${version}`);
    return matches[0];
  };
  const from = section(original), to = section(corrected);
  return original.slice(0, from.start) + corrected.slice(to.start, to.end).trimEnd() + '\n\n' + original.slice(from.end);
}

export async function syncReleaseDocs(source, targetRoot, tag, expectedCommit = '', correctionsRoot = '') {
  if (!/^v?\d+\.\d+\.\d+$/.test(tag ?? '')) throw new Error('Invalid release tag');
  const sha = execFileSync('git', ['-C', source, 'rev-parse', 'HEAD'], {encoding:'utf8'}).trim();
  if (expectedCommit && sha !== expectedCommit) throw new Error('Successful build does not match latest release; refusing stale deployment');
  const cargo = await readFile(resolve(source, 'Cargo.toml'), 'utf8');
  const version = tag.replace(/^v/, '');
  const workspace = cargo.split('[workspace.package]')[1]?.split(/\n\[/)[0];
  if (!workspace?.includes(`version = "${version}"`)) throw new Error('Release tag and source version differ');
  // Validate every source before replacing any build input.
  const correctionsCommit = correctionsRoot
    ? execFileSync('git', ['-C', correctionsRoot, 'rev-parse', 'HEAD'], {encoding:'utf8'}).trim() : undefined;
  const manualFiles = ['docs/usage.md', 'docs/usage.en.md'];
  const manuals = correctionsRoot ? await Promise.all(manualFiles.map(file => readFile(resolve(correctionsRoot, file), 'utf8'))) : [];
  const correctedManuals = correctionsRoot && validateManualPair(manuals, version);
  const entries = await Promise.all(documents.map(async file => {
    let bytes = await readFile(resolve(source, file));
    if (file.startsWith('CHANGELOG') && !bytes.toString().includes(`## ${version} ·`)) throw new Error(`Missing release notes: ${file}`);
    if (correctionsRoot && file.startsWith('CHANGELOG')) {
      bytes = Buffer.from(correctReleaseNotes(bytes.toString(), await readFile(resolve(correctionsRoot, file), 'utf8'), version));
    }
    if (correctedManuals && manualFiles.includes(file)) bytes = Buffer.from(manuals[manualFiles.indexOf(file)]);
    return [file, bytes];
  }));
  const hashes = {};
  for (const [file, bytes] of entries) {
    const target = resolve(targetRoot, file);
    await mkdir(dirname(target), {recursive:true});
    await writeFile(target, bytes);
    hashes[file] = createHash('sha256').update(bytes).digest('hex');
  }
  const provenance = resolve(targetRoot, 'website/public/release.json');
  await mkdir(dirname(provenance), {recursive:true});
  await writeFile(provenance, JSON.stringify({tag, commit:sha, changelogCorrectionsCommit:correctionsCommit, manualCorrectionsCommit:correctedManuals ? correctionsCommit : undefined, documents:hashes}, null, 2) + '\n');
  console.log(`Imported documentation from ${tag} (${sha}) without modifying the release source.`);
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  await syncReleaseDocs(resolve('../release-source'), resolve('..'), process.env.RELEASE_TAG, process.env.SOURCE_SHA, resolve('..'));
}
