import test from 'node:test';
import assert from 'node:assert/strict';
import { validateRelease } from './update-repository.mjs';
import { syncReleaseDocs, documents, correctReleaseNotes, validateManualPair, validateReleaseDoc } from './sync-release-docs.mjs';
import { createHash } from 'node:crypto';
import { mkdtemp, mkdir, writeFile, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, dirname } from 'node:path';
import { execFileSync } from 'node:child_process';
const repo = {full_name:'Yuch3nE/luciddesk', stargazers_count:10};
test('other public document corrections reject wrong versions and incomplete content', () => {
  const valid = '<!-- release-doc-version: 0.23.0 -->\n# CLI\n## Usage\nInstructions';
  assert.equal(validateReleaseDoc(valid, '0.23.0'), true);
  assert.equal(validateReleaseDoc('Legacy document', '0.23.0'), false);
  assert.throws(() => validateReleaseDoc(valid, '0.24.0'), /version/);
  assert.throws(() => validateReleaseDoc('<!-- release-doc-version: 0.23.0 -->', '0.23.0'), /incomplete/);
});
test('manual corrections require matching bilingual release versions and content', () => {
  const valid = '<!-- manual-version: 0.23.0 -->\n# Manual\n## Start\nInstructions';
  assert.equal(validateManualPair([valid, valid], '0.23.0'), true);
  assert.equal(validateManualPair(['Legacy manual', 'Legacy manual'], '0.23.0'), false);
  for (const pair of [[valid, 'Legacy manual'], [valid, valid.replace('0.23.0', '0.24.0')], [valid, '<!-- manual-version: 0.23.0 -->']]) {
    assert.throws(() => validateManualPair(pair, '0.23.0'));
  }
});
const fixture = () => ({tag_name:'v0.23.0', draft:false, prerelease:false, published_at:'2026-10-24',
  assets:['LucidDesk-0.23.0-windows-x64-setup.exe','LucidDesk-0.23.0-windows-x64-portable.zip','LucidDesk-0.23.0-3f8928e-windows-x64-20261024-010000.zip']
    .flatMap(name => [name, name+'.sha256']).map(name => ({name, size:123, state:'uploaded'}))});
test('accept complete latest stable release', () => assert.equal(validateRelease(repo, fixture(), 'v0.23.0').version, 'v0.23.0'));
test('reject old deployment and malformed tags', () => {
  assert.throws(() => validateRelease(repo, fixture(), 'v0.22.2'));
  assert.throws(() => validateRelease(repo, {...fixture(), tag_name:'main'}));
});
test('reject draft, prerelease and unpublished release', () => {
  for (const patch of [{draft:true}, {prerelease:true}, {published_at:null}]) assert.throws(() => validateRelease(repo, {...fixture(), ...patch}));
});
test('reject incomplete, empty and uploading assets', () => {
  const missing = fixture(); missing.assets.pop();
  assert.throws(() => validateRelease(repo, missing));
  for (const patch of [{size:0}, {state:'uploading'}]) {
    const release = fixture(); Object.assign(release.assets[0], patch);
    assert.throws(() => validateRelease(repo, release));
  }
});

test('correct only the requested release, preserving tagged history and excluding future changes', () => {
  const tagged = '# Changelog\n\n## 0.23.0 · date\n\nOld notes\n\n## 0.22.2\n\nTagged history\n';
  const main = '# Changelog\n\n## 0.24.0\n\nFuture\n\n## 0.23.0 · date\n\nCorrected notes\n\n## 0.22.2\n\nChanged history\n';
  assert.equal(correctReleaseNotes(tagged, main, '0.23.0'), tagged.replace('Old notes', 'Corrected notes'));
  assert.equal(correctReleaseNotes(tagged, main.replaceAll('\n', '\r\n'), '0.23.0').replaceAll('\r\n', '\n'), tagged.replace('Old notes', 'Corrected notes'));
  for (const invalid of ['## 0.24.0\n\nOther', '## 0.23.0\n\n', main + '\n## 0.23.0\n\nDuplicate', main + '\n## 0.23.0\n']) {
    assert.throws(() => correctReleaseNotes(tagged, invalid, '0.23.0'), /one nonempty/);
  }
});

test('import exact release documents and reject mismatched version or commit', async () => {
  const root = await mkdtemp(join(tmpdir(), 'luciddesk-release-test-'));
  try {
    const source = join(root, 'source');
    const target = join(root, 'build');
    await mkdir(source);
    execFileSync('git', ['init', source], {stdio:'pipe'});
    await writeFile(join(source, 'Cargo.toml'), '[workspace.package]\nversion = "0.23.0"\n');
    for (const file of documents) {
      await mkdir(dirname(join(source, file)), {recursive:true});
      await writeFile(join(source, file), `## 0.23.0 · 2026-10-24\n\n${file}\n`);
    }
    execFileSync('git', ['-C', source, 'add', '.']);
    execFileSync('git', ['-C', source, '-c', 'user.name=Test', '-c', 'user.email=test@example.invalid', 'commit', '-m', 'fixture'], {stdio:'pipe'});
    await syncReleaseDocs(source, target, 'v0.23.0');
    const manifest = JSON.parse(await readFile(join(target, 'website/public/release.json'), 'utf8'));
    assert.equal(manifest.tag, 'v0.23.0');
    assert.equal(Object.keys(manifest.documents).length, documents.length);
    for (const file of documents) assert.deepEqual(await readFile(join(source, file)), await readFile(join(target, file)));
    await assert.rejects(syncReleaseDocs(source, target, 'v0.24.0'), /version differ/);
    await assert.rejects(syncReleaseDocs(source, target, 'v0.23.0', '0'.repeat(40)), /stale deployment/);

    execFileSync('git', ['init', target], {stdio:'pipe'});
    for (const file of ['CHANGELOG.md', 'CHANGELOG.en.md']) {
      await writeFile(join(target, file), `## 0.24.0\n\nFuture\n\n## 0.23.0 · 2026-10-24\n\nCorrected ${file}\n`);
    }
    execFileSync('git', ['-C', target, 'add', '.']);
    execFileSync('git', ['-C', target, '-c', 'user.name=Test', '-c', 'user.email=test@example.invalid', 'commit', '-m', 'corrections'], {stdio:'pipe'});
    await syncReleaseDocs(source, target, 'v0.23.0', '', target);
    const updated = JSON.parse(await readFile(join(target, 'website/public/release.json'), 'utf8'));
    assert.equal(updated.commit, manifest.commit);
    assert.equal(updated.changelogCorrectionsCommit, execFileSync('git', ['-C', target, 'rev-parse', 'HEAD'], {encoding:'utf8'}).trim());
    for (const file of documents) {
      const bytes = await readFile(join(target, file));
      assert.equal(updated.documents[file], createHash('sha256').update(bytes).digest('hex'));
      if (file.startsWith('CHANGELOG')) {
        assert.match(bytes.toString(), /Corrected/);
        assert.doesNotMatch(bytes.toString(), /Future/);
        assert.doesNotMatch(await readFile(join(source, file), 'utf8'), /Corrected/);
      } else assert.deepEqual(bytes, await readFile(join(source, file)));
    }
    for (const file of ['docs/usage.md', 'docs/usage.en.md']) {
      await writeFile(join(target, file), '<!-- manual-version: 0.23.0 -->\n# Manual\n## Start\nCorrected manual');
    }
    await syncReleaseDocs(source, target, 'v0.23.0', '', target);
    const manualManifest = JSON.parse(await readFile(join(target, 'website/public/release.json'), 'utf8'));
    assert.equal(manualManifest.manualCorrectionsCommit, updated.changelogCorrectionsCommit);
    for (const file of ['docs/usage.md', 'docs/usage.en.md']) {
      const bytes = await readFile(join(target, file));
      assert.match(bytes.toString(), /Corrected manual/);
      assert.equal(manualManifest.documents[file], createHash('sha256').update(bytes).digest('hex'));
      assert.doesNotMatch(await readFile(join(source, file), 'utf8'), /Corrected manual/);
    }
    await writeFile(join(target, 'docs/cli.md'), '<!-- release-doc-version: 0.23.0 -->\n# CLI\n## Usage\nCorrected CLI');
    await syncReleaseDocs(source, target, 'v0.23.0', '', target);
    const docManifest = JSON.parse(await readFile(join(target, 'website/public/release.json'), 'utf8'));
    assert.deepEqual(docManifest.documentCorrections, {commit:updated.changelogCorrectionsCommit, files:['docs/cli.md']});
    const cliBytes = await readFile(join(target, 'docs/cli.md'));
    assert.match(cliBytes.toString(), /Corrected CLI/);
    assert.equal(docManifest.documents['docs/cli.md'], createHash('sha256').update(cliBytes).digest('hex'));
    assert.doesNotMatch(await readFile(join(source, 'docs/cli.md'), 'utf8'), /Corrected CLI/);
    // An invalid translation must not partially overwrite build inputs.
    await writeFile(join(target, 'CHANGELOG.en.md'), '## 0.24.0\n\nWrong version\n');
    const before = await readFile(join(target, 'CHANGELOG.md'));
    await assert.rejects(syncReleaseDocs(source, target, 'v0.23.0', '', target), /one nonempty/);
    assert.deepEqual(await readFile(join(target, 'CHANGELOG.md')), before);
  } finally { await rm(root, {recursive:true, force:true}); }
});
