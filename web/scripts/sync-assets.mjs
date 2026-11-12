// Copies the shared art from ../assets into public/ before dev and build:
//   img/      the 2.5D hero art and Nora's avatar
//   hero3d/   the three.js hero module (built from hero3d-src)
//   fonts/    Geist and Geist Mono
// His personal presets and GLBs are never bundled: the app fetches them from the paired server
// (GET /api/personal/...). The copies are gitignored; ../assets stays the one source.
import { cpSync, existsSync, mkdirSync, readdirSync, rmSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const web = join(dirname(fileURLToPath(import.meta.url)), '..');
const assets = join(web, '..', 'assets');
const pub = join(web, 'public');

function copyDir(from, to, filter = () => true) {
  if (!existsSync(from)) return 0;
  rmSync(to, { recursive: true, force: true });
  mkdirSync(to, { recursive: true });
  let n = 0;
  for (const name of readdirSync(from, { withFileTypes: true })) {
    if (!filter(name.name)) continue;
    cpSync(join(from, name.name), join(to, name.name), { recursive: true });
    n++;
  }
  return n;
}

const counts = {
  img: copyDir(join(assets, 'img'), join(pub, 'img')),
  hero3d: copyDir(join(assets, 'hero3d'), join(pub, 'hero3d'), (n) => n.endsWith('.js')),
  fonts: copyDir(join(assets, 'fonts'), join(pub, 'fonts'), (n) => n.endsWith('.woff2')),
};
// An older build step copied personal files into public/: make sure none ship.
rmSync(join(pub, 'personal'), { recursive: true, force: true });
console.log('assets:', Object.entries(counts).map(([k, v]) => `${k} ${v}`).join(', '));
