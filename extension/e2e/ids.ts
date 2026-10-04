import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

/** The fixed id of the e2e build (`dev-key.json`, SINTADE_DEV_KEY=1). */
export const DEV_EXTENSION_ID = (
  JSON.parse(
    readFileSync(join(dirname(fileURLToPath(import.meta.url)), '..', 'dev-key.json'), 'utf8'),
  ) as { id: string }
).id;
