// Pairing state: server URL, device token, device id. Kept in localStorage on the web build;
// the Capacitor shell can swap `storage` for the Keychain bridge without touching callers.
// Nothing here ever clears the token on a failed request: only `unpair()` does.
import { create } from 'zustand';

const K = { server: 'aos.server', token: 'aos.token', device: 'aos.device', safe: 'aos.safe' } as const;

const storage = {
  get(k: string): string | null {
    try {
      return localStorage.getItem(k);
    } catch {
      return null;
    }
  },
  set(k: string, v: string | null) {
    try {
      if (v === null) localStorage.removeItem(k);
      else localStorage.setItem(k, v);
    } catch {
      /* private mode: session-only */
    }
  },
};

/** Where the app was served from, when that is the server (not capacitor:// or file://). */
export function pageOrigin(): string | null {
  const o = location.origin;
  return /^https?:/.test(o) ? o : null;
}

export const isDemo = new URLSearchParams(location.search).has('demo');

interface Session {
  server: string;
  token: string | null;
  deviceId: string | null;
  /** A request came back 401: the pairing is kept, the UI offers to re-pair. */
  needsRepair: boolean;
  /** The last request could not reach the server. */
  offline: boolean;
  setPaired(server: string, token: string, deviceId: string): void;
  setServer(server: string): void;
  unpair(): void;
  flagRepair(v: boolean): void;
  flagOffline(v: boolean): void;
}

export const useSession = create<Session>((set) => ({
  server: storage.get(K.server) || pageOrigin() || '',
  token: isDemo ? 'demo' : storage.get(K.token),
  deviceId: storage.get(K.device),
  needsRepair: false,
  offline: false,
  setPaired(server, token, deviceId) {
    storage.set(K.server, server);
    storage.set(K.token, token);
    storage.set(K.device, deviceId);
    set({ server, token, deviceId, needsRepair: false });
  },
  setServer(server) {
    storage.set(K.server, server);
    set({ server });
  },
  unpair() {
    storage.set(K.token, null);
    storage.set(K.device, null);
    set({ token: null, deviceId: null, needsRepair: false });
  },
  flagRepair(v) {
    set((s) => (s.needsRepair === v ? s : { needsRepair: v }));
  },
  flagOffline(v) {
    set((s) => (s.offline === v ? s : { offline: v }));
  },
}));

export const safeStore = {
  get: () => storage.get(K.safe) === '1',
  set: (v: boolean) => storage.set(K.safe, v ? '1' : '0'),
};

export function cacheGet<T>(key: string): T | undefined {
  const raw = storage.get('aos.cache.' + key);
  if (!raw) return undefined;
  try {
    return JSON.parse(raw) as T;
  } catch {
    return undefined;
  }
}

export function cacheSet(key: string, value: unknown) {
  try {
    storage.set('aos.cache.' + key, JSON.stringify(value));
  } catch {
    /* quota: skip */
  }
}
