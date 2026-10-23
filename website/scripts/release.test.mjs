import test from 'node:test';
import assert from 'node:assert/strict';
import { validateRelease } from './update-repository.mjs';
import { syncReleaseDocs, documents } from './sync-release-docs.mjs';
import { mkdtemp, mkdir, writeFile, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, dirname } from 'node:path';
import { execFileSync } from 'node:child_process';
const repo = {full_name:'Yuch3nE/luciddesk', stargazers_count:10};
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
  } finally { await rm(root, {recursive:true, force:true}); }
});
