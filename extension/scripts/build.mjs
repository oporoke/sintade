// Bundles the extension into dist/: one file per entry point (the service worker, the popup and,
// later, the offscreen and content scripts), plus the manifest, HTML/CSS and generated icons.
import { cp, mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { build, context } from 'esbuild';

import { ICON_SIZES, iconPng } from './icons.mjs';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const dist = join(root, process.env.SINTADE_DIST ?? 'dist');
const watch = process.argv.includes('--watch');
// The Sintade origin the extension talks to. Dev default: the HTTPS dev server (ADR-0006). Store
// builds set SINTADE_ORIGIN to the production origin (TODO: Verify, not decided yet).
const origin = (process.env.SINTADE_ORIGIN ?? 'https://localhost:4200').replace(/\/$/, '');

const entries = {
  'service-worker': 'src/background/service-worker.ts',
  popup: 'src/popup/popup.ts',
  offscreen: 'src/offscreen/offscreen.ts',
  mic: 'src/permission/mic.ts',
};

const options = {
  absWorkingDir: root,
  entryPoints: entries,
  outdir: dist,
  bundle: true,
  format: 'esm',
  target: 'chrome116',
  sourcemap: process.env.NODE_ENV === 'production' ? false : 'linked',
  minify: process.env.NODE_ENV === 'production',
  logLevel: 'info',
  legalComments: 'none',
  define: { __SINTADE_ORIGIN__: JSON.stringify(origin) },
};

// Injected with chrome.scripting.executeScript({ files }): a classic script, not a module.
const contentOptions = {
  ...options,
  entryPoints: { content: 'src/content/content.ts' },
  format: 'iife',
};

async function copyStatic() {
  const pkg = JSON.parse(await readFile(join(root, 'package.json'), 'utf8'));
  const manifest = JSON.parse(await readFile(join(root, 'manifest.json'), 'utf8'));
  manifest.version = pkg.version;
  manifest.host_permissions = [`${origin}/*`];
  if (process.env.SINTADE_DEV_KEY) {
    // A fixed public key gives the unpacked extension a fixed id, which the e2e run needs for
    // Chrome's --allowlisted-extension-id (tab capture without a toolbar click). Never in a store build.
    // activeTab is only granted by a real toolbar click, which automation can't make: the test
    // pages (*.example) get a host permission so the overlay script can be injected into them.
    manifest.host_permissions.push('https://*.example/*');
    manifest.key = JSON.parse(await readFile(join(root, 'dev-key.json'), 'utf8')).key;
  }
  await writeFile(join(dist, 'manifest.json'), `${JSON.stringify(manifest, null, 2)}\n`);
  await cp(join(root, 'src/popup/popup.html'), join(dist, 'popup.html'));
  await cp(join(root, 'src/popup/popup.css'), join(dist, 'popup.css'));
  await cp(join(root, 'src/offscreen/offscreen.html'), join(dist, 'offscreen.html'));
  await cp(join(root, 'src/permission/mic.html'), join(dist, 'mic.html'));
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
  await (await context(contentOptions)).watch();
  console.log('watching…');
} else {
  await build(options);
  await build(contentOptions);
}
