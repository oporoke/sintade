// Builds the extension for the stores and zips it: `SINTADE_ORIGIN=https://… npm run package`.
// A store build differs from the dev build: minified, no source maps, no test key, no test host
// permissions, and the manifest must pass the store rules (scripts/manifest-check.mjs).
import { spawnSync } from 'node:child_process';
import { mkdir, readFile, readdir, stat, writeFile } from 'node:fs/promises';
import { dirname, join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';

import { checkManifest } from './manifest-check.mjs';
import { zip } from './zip.mjs';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
// Built apart from dist/, which the browser tests use (with their dev key and test hosts).
const dist = join(root, 'dist-store');
const origin = process.env.SINTADE_ORIGIN;

if (!origin || !origin.startsWith('https://')) {
  console.error(
    'Set SINTADE_ORIGIN to the production origin, e.g. SINTADE_ORIGIN=https://app.sintade.example\n' +
      '(the production address is not decided yet: TODO: Verify)',
  );
  process.exit(2);
}

const built = spawnSync(process.execPath, [join(root, 'scripts/build.mjs')], {
  cwd: root,
  stdio: 'inherit',
  env: { ...process.env, NODE_ENV: 'production', SINTADE_DEV_KEY: '', SINTADE_DIST: 'dist-store' },
});
if (built.status !== 0) process.exit(built.status ?? 1);

async function walk(dir) {
  const out = [];
  for (const name of (await readdir(dir)).sort()) {
    const path = join(dir, name);
    if ((await stat(path)).isDirectory()) out.push(...(await walk(path)));
    else if (!name.endsWith('.map')) out.push(path);
  }
  return out;
}

const files = await walk(dist);
const names = new Set(files.map((f) => relative(dist, f).split('\\').join('/')));
const manifest = JSON.parse(await readFile(join(dist, 'manifest.json'), 'utf8'));
const problems = checkManifest(manifest, (path) => names.has(path));
if (problems.length > 0) {
  console.error(`The manifest is not fit for a store:\n- ${problems.join('\n- ')}`);
  process.exit(1);
}

const entries = [];
for (const file of files) {
  entries.push({
    name: relative(dist, file).split('\\').join('/'),
    data: await readFile(file),
  });
}
// manifest.json first, as the stores' validators expect to find it quickly.
entries.sort((a, b) =>
  a.name === 'manifest.json' ? -1 : b.name === 'manifest.json' ? 1 : a.name.localeCompare(b.name),
);
const out = join(root, 'artifacts');
await mkdir(out, { recursive: true });
const archive = join(out, `sintade-extension-${manifest.version}.zip`);
await writeFile(archive, zip(entries));
console.log(
  `${relative(root, archive)}: ${entries.length} files, ${(await stat(archive)).size} bytes, ` +
    `permissions ${manifest.permissions.join(', ')}, host ${manifest.host_permissions.join(', ')}`,
);
