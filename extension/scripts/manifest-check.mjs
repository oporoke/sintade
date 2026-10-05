// Rules a *store* build of the manifest must meet. Used by `npm run package` and its test.

export const STORE_PERMISSIONS = ['activeTab', 'offscreen', 'scripting', 'storage', 'tabCapture'];
export const ICON_SIZES = ['16', '32', '48', '128'];

/**
 * @param {Record<string, unknown>} manifest the built dist/manifest.json
 * @param {(path: string) => boolean} exists whether a path is in the build
 * @returns {string[]} what is wrong (empty: fine to upload)
 */
export function checkManifest(manifest, exists) {
  const problems = [];
  const m = /** @type {any} */ (manifest);
  if (m.manifest_version !== 3) problems.push('manifest_version must be 3');
  if (m.name !== 'Sintade') problems.push('name must be "Sintade"');
  if (
    typeof m.description !== 'string' ||
    m.description.length === 0 ||
    m.description.length > 132
  ) {
    problems.push('description must be 1-132 characters (the stores cut it)');
  }
  if (!/^\d+(\.\d+){0,3}$/.test(String(m.version))) {
    problems.push(`version "${m.version}" is not 1-4 dot-separated numbers`);
  }
  if ('key' in m) problems.push('"key" is for the e2e build only; stores assign their own');

  const permissions = [...(m.permissions ?? [])].sort();
  if (JSON.stringify(permissions) !== JSON.stringify([...STORE_PERMISSIONS].sort())) {
    problems.push(
      `permissions must be exactly ${STORE_PERMISSIONS.join(', ')} (got ${permissions.join(', ')})`,
    );
  }

  const hosts = m.host_permissions ?? [];
  if (hosts.length !== 1) {
    problems.push(`host_permissions must be the one Sintade origin (got ${hosts.length})`);
  } else {
    const host = String(hosts[0]);
    const match = /^(https?):\/\/([^/]+)\/\*$/.exec(host);
    if (!match || match[1] !== 'https') {
      problems.push(`host permission ${host} must be https://<origin>/*`);
    } else if (/localhost|127\.0\.0\.1|\*|\.example$|\.test$/.test(match[2] ?? '')) {
      // The reserved test domains pass `package --dry-run` in CI; a real submission uses neither.
      if (
        !process.env['SINTADE_ALLOW_TEST_ORIGIN'] ||
        /\*|localhost|127\.0\.0\.1/.test(match[2] ?? '')
      ) {
        problems.push(`host permission ${host} is a development or test origin`);
      }
    }
  }
  for (const key of ['content_scripts', 'externally_connectable', 'web_accessible_resources']) {
    if (key in m) problems.push(`${key} is not used and must not be declared`);
  }
  if (m.content_security_policy)
    problems.push('no custom content_security_policy (none is needed)');

  for (const size of ICON_SIZES) {
    const path = m.icons?.[size];
    if (!path || !exists(path)) problems.push(`icon ${size} is missing`);
  }
  const worker = m.background?.service_worker;
  if (!worker || !exists(worker)) problems.push('the service worker file is missing');
  const popup = m.action?.default_popup;
  if (!popup || !exists(popup)) problems.push('the popup page is missing');
  return problems;
}
