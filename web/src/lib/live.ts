// Live updates. `/api/events` (SSE) says a row changed somewhere (CLI, Nora, another device);
// we refetch the affected screens, debounced. EventSource reconnects by itself; `hello` after a
// reconnect triggers one full refresh so nothing missed stays stale.
import { authUrl } from './api';
import { qc } from './queries';
import { isDemo, useSession } from './session';
import type { NoraEvent } from './types';

const ENTITY_KEYS: Record<string, string[]> = {
  task: ['today', 'loose', 'xp'],
  habit: ['today', 'xp'],
  habit_log: ['today', 'xp'],
  mood: ['today', 'orb'],
  journal: ['today', 'journal'],
  journal_entry: ['today', 'journal'],
  xp_event: ['today', 'xp'],
  goal: ['today', 'profile'],
  area: ['areas', 'today'],
  alarm: ['today'],
  inbox_item: ['inbox'],
};

let es: EventSource | null = null;
let pending = new Set<string>();
let timer: ReturnType<typeof setTimeout> | null = null;

function schedule(keys: string[]) {
  keys.forEach((k) => pending.add(k));
  if (timer) return;
  timer = setTimeout(() => {
    const keys = pending;
    pending = new Set();
    timer = null;
    keys.forEach((k) => void qc.invalidateQueries({ queryKey: [k] }));
  }, 350);
}

export function startLive() {
  if (isDemo) return;
  const connect = () => {
    es?.close();
    es = null;
    const { token, server } = useSession.getState();
    if (!token || !server) return;
    es = new EventSource(authUrl('/api/events'));
    let first = true;
    es.addEventListener('hello', () => {
      if (!first) void qc.invalidateQueries();
      first = false;
    });
    es.addEventListener('change', (e) => {
      try {
        const ev = JSON.parse((e as MessageEvent).data) as { entity: string };
        schedule(ENTITY_KEYS[ev.entity] || ['today']);
      } catch {
        schedule(['today']);
      }
    });
    es.addEventListener('resync', () => void qc.invalidateQueries());
  };
  connect();
  useSession.subscribe((s, prev) => {
    if (s.token !== prev.token || s.server !== prev.server) connect();
  });
}

/** Nora's own stream (one per app): messages, streaming deltas, tool steps, orb, call events. */
type NoraListener = (e: NoraEvent) => void;
const noraListeners = new Set<NoraListener>();
let noraSrc: EventSource | null = null;

function connectNora() {
  noraSrc?.close();
  noraSrc = null;
  const { token, server } = useSession.getState();
  if (isDemo || !token || !server) return;
  noraSrc = new EventSource(authUrl('/api/nora/events'));
  noraSrc.onmessage = (e) => {
    let ev: NoraEvent;
    try {
      ev = JSON.parse(e.data) as NoraEvent;
    } catch {
      return;
    }
    noraListeners.forEach((fn) => fn(ev));
  };
}

export function startNoraLive() {
  connectNora();
  useSession.subscribe((s, prev) => {
    if (s.token !== prev.token || s.server !== prev.server) connectNora();
  });
}

export function noraStream(onEvent: NoraListener): () => void {
  noraListeners.add(onEvent);
  return () => noraListeners.delete(onEvent);
}
