// His personal hero files (presets JSON and the GLB models) come from the paired server at
// GET /api/personal/<path>, never from the app bundle, so a public build carries none of them.
// They are kept on the device in IndexedDB (bytes + ETag), so the next launch mounts the 3D hero
// from the cache at once and revalidates in the background (304s when nothing changed).
// Unpaired, demo, or offline on first run: null, and the hero stays on the 2D art.
import { clear, createStore, del, entries, set } from 'idb-keyval';
import { isDemo, useSession } from '@/lib/session';

export const PERSONAL_DB = 'aos-personal';
const store = createStore(PERSONAL_DB, 'files');

interface Entry {
  etag: string | null;
  type: string;
  // ArrayBuffer, not Blob: older WebKit builds were unreliable with Blobs in IndexedDB.
  bytes: ArrayBuffer;
}
type Files = Record<string, Entry>;

const PRESETS = 'hero3d.json';
const DEFAULT_STAGES = [1, 5, 10, 15, 20, 30];
const stageFile = (n: number) => `character/stage-${String(n).padStart(2, '0')}.glb`;
const SAFE = /^[A-Za-z0-9_-][A-Za-z0-9._-]*(\/[A-Za-z0-9_-][A-Za-z0-9._-]*)*$/;

interface Presets {
  models?: { stages?: number[] };
  overrides?: Record<string, { url?: string } | { url?: string }[] | undefined>;
}

/** Every file the presets can ask for: the stage GLBs, the companion and any overrides. */
function wanted(p: Presets): string[] {
  const list = (p.models?.stages || DEFAULT_STAGES).map(stageFile);
  list.push('companion.glb');
  for (const v of Object.values(p.overrides || {})) {
    for (const o of Array.isArray(v) ? v : [v]) if (o?.url) list.push(o.url);
  }
  return [...new Set(list)].filter((f) => SAFE.test(f));
}

const isGlb = (b: ArrayBuffer) => {
  const u = new Uint8Array(b, 0, Math.min(4, b.byteLength));
  return u.length === 4 && u[0] === 0x67 && u[1] === 0x6c && u[2] === 0x54 && u[3] === 0x46;
};

class Unauthorized extends Error {}

/** One file from the server: a new Entry, 'same' (304), or null (the server has none). */
async function fetchOne(path: string, prev?: Entry): Promise<Entry | 'same' | null> {
  const { server, token } = useSession.getState();
  const headers: Record<string, string> = { Authorization: `Bearer ${token}` };
  if (prev?.etag) headers['If-None-Match'] = prev.etag;
  const res = await fetch(`${server.replace(/\/+$/, '')}/api/personal/${path}`, { headers, cache: 'no-store' });
  if (res.status === 304) return prev ? 'same' : null;
  if (res.status === 404) return null;
  if (res.status === 401) throw new Unauthorized();
  if (!res.ok) throw new Error(`personal ${path}: ${res.status}`);
  const bytes = await res.arrayBuffer();
  if (path.endsWith('.glb') && !isGlb(bytes)) return null;
  return { etag: res.headers.get('ETag'), type: res.headers.get('Content-Type') || '', bytes };
}

async function readCache(): Promise<Files> {
  try {
    return Object.fromEntries((await entries<string, Entry>(store)).filter(([, e]) => e && e.bytes)) as Files;
  } catch {
    return {};
  }
}

/** Bring the cache up to date with the server. Returns the new set (empty when the server has none). */
async function refresh(cached: Files): Promise<Files> {
  const out: Files = { ...cached };
  let p: Entry | 'same' | null;
  try {
    p = await fetchOne(PRESETS, cached[PRESETS]);
  } catch (e) {
    // A revoked device must not keep his personal files.
    if (e instanceof Unauthorized) await clear(store).catch(() => {});
    throw e;
  }
  if (p === null) {
    await clear(store).catch(() => {});
    return {};
  }
  if (p !== 'same') {
    out[PRESETS] = p;
    await set(PRESETS, p, store).catch(() => {});
  }
  let presets: Presets = {};
  try {
    presets = JSON.parse(new TextDecoder().decode(out[PRESETS].bytes));
  } catch {
    /* bad JSON: the module falls back to its placeholder */
  }
  const list = wanted(presets);
  // Files the presets no longer name.
  for (const k of Object.keys(out)) {
    if (k !== PRESETS && !list.includes(k)) {
      delete out[k];
      await del(k, store).catch(() => {});
    }
  }
  await Promise.all(
    list.map(async (path) => {
      try {
        const r = await fetchOne(path, cached[path]);
        if (r === 'same') return;
        if (r === null) {
          delete out[path];
          await del(path, store).catch(() => {});
          return;
        }
        out[path] = r;
        await set(path, r, store).catch(() => {});
      } catch {
        /* keep what we had */
      }
    }),
  );
  return out;
}

let urls: string[] = [];
function toUrls(files: Files): Record<string, string> {
  urls.forEach((u) => URL.revokeObjectURL(u));
  urls = [];
  const map: Record<string, string> = {};
  for (const [path, e] of Object.entries(files)) {
    const type = e.type || (path.endsWith('.json') ? 'application/json' : 'model/gltf-binary');
    const u = URL.createObjectURL(new Blob([e.bytes], { type }));
    urls.push(u);
    map[path] = u;
  }
  return map;
}

/**
 * URLs for the hero module's `personalFiles` option, or null when there is nothing to show.
 * Cached files return at once (and refresh in the background for the next launch).
 */
export async function personalFiles(): Promise<Record<string, string> | null> {
  const { server, token } = useSession.getState();
  if (isDemo || !token || !server) return null;
  const cached = await readCache();
  if (cached[PRESETS]) {
    refresh(cached).catch(() => {});
    return toUrls(cached);
  }
  try {
    const fresh = await refresh({});
    return fresh[PRESETS] ? toUrls(fresh) : null;
  } catch {
    return null;
  }
}

export function releasePersonalUrls() {
  urls.forEach((u) => URL.revokeObjectURL(u));
  urls = [];
}
