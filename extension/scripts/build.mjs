// Bundles the extension into dist/: one file per entry point (the service worker, the popup and,
// later, the offscreen and content scripts), plus the manifest, HTML/CSS and generated icons.
import { cp, mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { build, context } from 'esbuild';

import { ICON_SIZES, iconPng } from './icons.mjs';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const dist = join(root, 'dist');
const watch = process.argv.includes('--watch');

const entries = {
  'service-worker': 'src/background/service-worker.ts',
  popup: 'src/popup/popup.ts',
};

const options = {
  absWorkingDir: root,
  entryPoints: entries,
  outdir: dist,
  bundle: true,
  format: 'esm',
  target: 'chrome116',
  sourcemap: 'linked',
  logLevel: 'info',
  legalComments: 'none',
};

async function copyStatic() {
  const pkg = JSON.parse(await readFile(join(root, 'package.json'), 'utf8'));
  const manifest = JSON.parse(await readFile(join(root, 'manifest.json'), 'utf8'));
  manifest.version = pkg.version;
  await writeFile(join(dist, 'manifest.json'), `${JSON.stringify(manifest, null, 2)}\n`);
  await cp(join(root, 'src/popup/popup.html'), join(dist, 'popup.html'));
  await cp(join(root, 'src/popup/popup.css'), join(dist, 'popup.css'));
  await mkdir(join(dist, 'icons'), { recursive: true });
  for (const size of ICON_SIZES) {
    await writeFile(join(dist, 'icons', `icon-${size}.png`), iconPng(size));
  }
}

await rm(dist, { recursive: true, force: true });
await mkdir(dist, { recursive: true });
await copyStatic();
if (watch) {
  const ctx = await context(options);
  await ctx.watch();
  console.log('watching…');
} else {
  await build(options);
}
