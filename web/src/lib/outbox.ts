// Offline outbox. Every write is a tool call queued in IndexedDB as {op_id, tool, args} and sent
// with POST /api/sync/push. The server applies each op once (by op_id), so resending after a
// dropped connection is safe. The UI has already applied the change; a flush only confirms it.
import { createStore, get, set } from 'idb-keyval';
import { create } from 'zustand';
import { api, ApiError, OfflineError } from './api';
import { isDemo } from './session';
import { demoCall } from './demo';

export interface Op {
  op_id: string;
  tool: string;
  args: Record<string, unknown>;
  at: number;
}

interface PushResult {
  results: { op_id: string; replayed: boolean; outcome: { ok: boolean; error?: { code: string; message: string } } }[];
  latest: number;
}

const db = createStore('aos', 'outbox');
const KEY = 'ops';

export const useOutbox = create<{ pending: number; lastError: string | null }>(() => ({ pending: 0, lastError: null }));

let memory: Op[] | null = null;
let flushing: Promise<void> | null = null;
const listeners = new Set<(ops: Op[]) => void>();

/** Called after a flush applied ops (to refetch). */
export function onFlushed(fn: (ops: Op[]) => void) {
  listeners.add(fn);
  return () => listeners.delete(fn);
}

export function newId(): string {
  const b = new Uint8Array(16);
  crypto.getRandomValues(b);
  return Array.from(b, (x) => x.toString(16).padStart(2, '0')).join('');
}

async function load(): Promise<Op[]> {
  if (memory) return memory;
  try {
    memory = ((await get(KEY, db)) as Op[] | undefined) || [];
  } catch {
    memory = [];
  }
  useOutbox.setState({ pending: memory.length });
  return memory;
}

async function save(ops: Op[]) {
  memory = ops;
  useOutbox.setState({ pending: ops.length });
  try {
    await set(KEY, ops, db);
  } catch {
    /* no IndexedDB (private mode): the queue lives in memory until the page closes */
  }
}

export async function enqueue(tool: string, args: Record<string, unknown>): Promise<Op> {
  const op: Op = { op_id: newId(), tool, args, at: Date.now() };
  const ops = await load();
  await save([...ops, op]);
  void flush();
  return op;
}

export function flush(): Promise<void> {
  if (flushing) return flushing;
  flushing = (async () => {
    const ops = await load();
    if (!ops.length) return;
    const batch = ops.slice(0, 50);
    try {
      if (isDemo) {
        for (const op of batch) await demoCall(op.tool, op.args);
      } else {
        const res = await api.post<PushResult>('/api/sync/push', { ops: batch.map(({ op_id, tool, args }) => ({ op_id, tool, args })) }, 30000);
        const failed = res.results.filter((r) => !r.outcome.ok);
        useOutbox.setState({ lastError: failed[0]?.outcome.error?.message || null });
      }
      const done = new Set(batch.map((o) => o.op_id));
      const rest = (await load()).filter((o) => !done.has(o.op_id));
      await save(rest);
      listeners.forEach((fn) => fn(batch));
    } catch (e) {
      // Offline or not paired right now: keep everything and try again later.
      if (!(e instanceof OfflineError) && !(e instanceof ApiError && e.status === 401)) {
        useOutbox.setState({ lastError: (e as Error).message });
      }
    }
  })().finally(() => {
    flushing = null;
  });
  return flushing;
}

export function startOutbox() {
  void load().then(() => flush());
  addEventListener('online', () => void flush());
  document.addEventListener('visibilitychange', () => {
    if (document.visibilityState === 'visible') void flush();
  });
  setInterval(() => {
    if ((memory?.length || 0) > 0) void flush();
  }, 15000);
}
