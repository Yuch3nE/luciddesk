import { readFile, writeFile, readdir, mkdir, unlink } from 'node:fs/promises';
import { resolve, relative, dirname, sep } from 'node:path';

// English routes are build inputs only. Ship language payloads, not /en/ pages.
const root = resolve('dist');
const englishRoot = resolve(root, 'en');
const redirects = ['/en / 301'];
let count = 0;
async function walk(directory) {
  for (const entry of await readdir(directory, {withFileTypes:true})) {
    const file = resolve(directory, entry.name);
    if (!file.startsWith(englishRoot + sep)) throw new Error('Unexpected locale output path');
    if (entry.isDirectory()) await walk(file);
    else if (entry.name.endsWith('.html')) {
      const path = relative(englishRoot, file).split(sep).join('/');
      for (const [lang, source] of [['en', file], ['zh', resolve(root, path)]]) {
        const html = await readFile(source, 'utf8');
        const body = html.match(/<body\b[^>]*>([\s\S]*?)<\/body>/i)?.[1];
        const head = html.match(/<head\b[^>]*>([\s\S]*?)<\/head>/i)?.[1];
        if (!body || !head) throw new Error(`Missing locale document: ${source}`);
        const destination = resolve(root, '_languages', lang, path.replace(/\.html$/, '.json'));
        await mkdir(dirname(destination), {recursive:true});
        await writeFile(destination, JSON.stringify({lang, head, body: body.replace(/<script\b[^>]*>[\s\S]*?<\/script>/gi, '')}));
      }
      const publicPath = '/' + path.replace(/index\.html$/, '');
      redirects.push(`/en${publicPath} ${publicPath} 301`);
      // Exact checked generated file only, never a recursive directory deletion.
      await unlink(file);
      count++;
    }
  }
}
await walk(englishRoot);
await writeFile(resolve(root, '_redirects'), redirects.join('\n') + '\n');
console.log(`Languages: ${count} shared URLs, bilingual payloads, legacy /en/ redirects.`);
