// Makes sure ../web/dist has a page before `cap sync` copies it into the native projects.
// The React build (web/, `npm run build`) writes web/dist. Until that exists, the placeholder page is used,
// so the shell always builds. Pass --placeholder to force the placeholder.
import { existsSync, mkdirSync, copyFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const dist = join(here, '..', '..', 'web', 'dist');
const page = join(dist, 'index.html');
const force = process.argv.includes('--placeholder');

if (existsSync(page) && !force) {
  console.log(`web build found: ${page}`);
} else {
  mkdirSync(dist, { recursive: true });
  copyFileSync(join(here, '..', 'placeholder', 'index.html'), page);
  console.log(`no web build; placeholder copied to ${page}`);
}
