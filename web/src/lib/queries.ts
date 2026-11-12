// Server state through TanStack Query. Each read keeps its last good copy in localStorage so the
// app paints instantly (and offline) and refreshes behind it. Writes are optimistic: the cache
// changes first, the outbox carries the tool call, a flush refetches.
import { QueryClient, useQuery } from '@tanstack/react-query';
import { api, OfflineError } from './api';
import { cacheGet, cacheSet, useSession } from './session';
import { enqueue, newId, onFlushed } from './outbox';
import type {
  AreaList, Habit, HabitLogStatus, InboxItem, JournalEntry, MissionOverview, Mood, NoraMsg, OrbState, Profile, Task, TaskNode, Today,
  WorkProject, WorkThread, XpSummary,
} from './types';

export const qc = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 20_000,
      gcTime: 24 * 3600_000,
      retry: (n, e) => !(e instanceof OfflineError) && n < 2,
      refetchOnWindowFocus: true,
      networkMode: 'always',
    },
  },
});

onFlushed(() => {
  void qc.invalidateQueries();
});

function persisted<T>(key: string, fn: () => Promise<T>, opts: { enabled?: boolean; refetchInterval?: number } = {}) {
  const token = useSession((s) => s.token);
  return useQuery<T>({
    queryKey: [key],
    queryFn: async () => {
      const v = await fn();
      cacheSet(key, v);
      return v;
    },
    initialData: () => cacheGet<T>(key),
    initialDataUpdatedAt: 0,
    enabled: !!token && opts.enabled !== false,
    refetchInterval: opts.refetchInterval,
  });
}

export const useToday = () => persisted<Today>('today', () => api.tool('today'), { refetchInterval: 60_000 });
export const useProfile = () => persisted<Profile>('profile', () => api.tool('profile'));
export const useXp = () => persisted<XpSummary>('xp', () => api.tool('xp_summary'));
export const useAreas = () => persisted<AreaList>('areas', () => api.tool('area_list'));
export const useOrb = () => persisted<OrbState>('orb', () => api.tool('orb_state'), { refetchInterval: 120_000 });
export const useInbox = () =>
  persisted<InboxItem[]>('inbox', async () => (await api.tool<{ items: InboxItem[] }>('inbox_list', { limit: 60 })).items || []);
export const useLooseTasks = () =>
  persisted<Task[]>('loose', async () => (await api.tool<{ tasks: Task[] }>('task_list', { status: 'open', limit: 60 })).tasks || []);
export const useMission = () => persisted<MissionOverview>('mission', () => api.tool('mission_overview'));
export const useJournal = () =>
  persisted<JournalEntry[]>('journal', async () => (await api.tool<{ entries: JournalEntry[] }>('journal_read', { days: 14 })).entries || []);
export const useNoraThread = () => persisted<NoraMsg[]>('nora', () => api.get('/api/nora/thread?limit=60'));
export const useWorkProjects = () => persisted<WorkProject[]>('work-projects', () => api.get('/api/work/projects'));
export const useWorkThreads = () => persisted<WorkThread[]>('work-threads', () => api.get('/api/work/threads'), { refetchInterval: 60_000 });
/** T3 Code's own threads, read only (the server reads T3's database). */
export const useT3Threads = () => persisted<WorkThread[]>('work-t3', () => api.get('/api/work/t3/threads?limit=60'), { refetchInterval: 60_000 });

export interface Heartbeat {
  id: number;
  at: number;
  decision: string;
  reason: string;
  text: string;
}
export interface Proposal {
  id: string;
  status?: string;
  op?: string;
  target_kind?: string;
  quote?: string | null;
  reason?: string | null;
  payload?: Record<string, unknown>;
  created_at?: string | number;
}
export const useHeartbeats = () => persisted<Heartbeat[]>('heartbeats', () => api.get('/api/nora/heartbeats'), { refetchInterval: 300_000 });
export const useProposals = () =>
  persisted<Proposal[]>('proposals', async () => {
    const r = await api.tool<{ proposals?: Proposal[] } | Proposal[]>('mission_proposal_list', { status: 'pending' });
    return Array.isArray(r) ? r : r.proposals || [];
  });

/* ═══ optimistic writes ═══ */

function patchToday(fn: (t: Today) => void) {
  qc.setQueryData<Today>(['today'], (old) => {
    if (!old) return old;
    const next = structuredClone(old);
    fn(next);
    cacheSet('today', next);
    return next;
  });
}

function eachTask(t: Today, fn: (task: Task, parent: TaskNode | null) => void) {
  for (const a of t.areas) for (const n of a.tasks) {
    fn(n, null);
    n.subtasks?.forEach((s) => fn(s, n));
  }
}

function setStatus(id: string, status: Task['status'], extra: Partial<Task> = {}) {
  patchToday((t) => {
    eachTask(t, (task, parent) => {
      if (task.id !== id) return;
      const was = task.status;
      Object.assign(task, { status }, extra);
      if (parent?.rollup) {
        if (status === 'done' && was !== 'done') parent.rollup.done += 1;
        if (was === 'done' && status !== 'done') parent.rollup.done -= 1;
      }
    });
    const flat = t.areas.flatMap((a) => a.tasks);
    t.score.done = flat.filter((x) => x.status === 'done').length;
    t.score.skipped = flat.filter((x) => x.status === 'skipped').length;
    t.score.open = flat.filter((x) => x.status === 'open' || x.status === 'doing').length;
  });
}

function addXp(n: number) {
  patchToday((t) => {
    t.xp.today += n;
    t.xp.level.total_xp += n;
    t.xp.level.into_level += n;
    t.xp.level.to_next = Math.max(0, t.xp.level.to_next - n);
    t.xp.level.progress = Math.min(1, t.xp.level.into_level / t.xp.level.level_size);
  });
}

export const act = {
  tick(task: Task): number {
    const reopen = task.status === 'done';
    const xp = 10 * (task.xp_weight || 1);
    setStatus(task.id, reopen ? 'open' : 'done');
    addXp(reopen ? -xp : xp);
    void enqueue(reopen ? 'task_update' : 'task_done', reopen ? { id: task.id, status: 'open' } : { id: task.id });
    return reopen ? 0 : xp;
  },
  skip(task: Task, reason: string) {
    setStatus(task.id, 'skipped', { skip_reason: reason });
    // `genuine` is asked again at close of day; picking a reason here counts as the honest one.
    void enqueue('task_skip', { id: task.id, reason, genuine: true });
  },
  add(title: string, area: string | null, parent?: string, scheduled?: string) {
    const tmp: TaskNode = {
      id: 'tmp-' + newId().slice(0, 10), parent_id: parent || null, area_id: area, project_id: null, title, notes: null,
      status: 'open', due: null, scheduled: scheduled || null, xp_weight: 1, skip_reason: null, pending: true,
    };
    patchToday((t) => {
      if (parent) {
        eachTask(t, (task) => {
          const node = task as TaskNode;
          if (node.id === parent) {
            (node.subtasks ||= []).push(tmp);
            node.rollup = node.rollup ? { ...node.rollup, total: node.rollup.total + 1 } : { total: 1, done: 0, skipped: 0 };
          }
        });
        return;
      }
      let bucket = t.areas.find((a) => a.area_id === area);
      if (!bucket) {
        bucket = { area_id: area, area: null, pillar_id: null, tasks: [] };
        t.areas.push(bucket);
      }
      bucket.tasks.push(tmp);
    });
    const args: Record<string, unknown> = { title, due: 'today' };
    if (area) args.area = area;
    if (parent) args.parent = parent;
    if (scheduled) args.scheduled = scheduled;
    void enqueue('task_add', args);
  },
  habit(h: Habit, status: HabitLogStatus | null): number {
    const prev = h.today;
    patchToday((t) => {
      const x = t.habits.find((y) => y.id === h.id);
      if (!x) return;
      x.today = status;
      const counts = (s: HabitLogStatus | null) => s === 'done' || s === 'floor';
      if (counts(status) && !counts(prev)) x.streak.current += 1;
      if (!counts(status) && counts(prev)) x.streak.current = Math.max(0, x.streak.current - 1);
      if (counts(status)) x.streak.state = 'intact';
    });
    const xp = status === 'done' ? h.xp : status === 'floor' ? Math.ceil(h.xp / 2) : 0;
    const prevXp = prev === 'done' ? h.xp : prev === 'floor' ? Math.ceil(h.xp / 2) : 0;
    addXp(xp - prevXp);
    void enqueue('habit_log', { habit: h.id, status: status || 'excused' });
    return Math.max(0, xp - prevXp);
  },
  mood(valence: number, energy: number, reasons: string[], note?: string) {
    const m: Mood = {
      id: 'tmp-' + newId().slice(0, 8), date: '', at: new Date().toISOString(), valence, energy, labels: [], reasons, note: note || null,
    };
    patchToday((t) => {
      t.moods = [...t.moods, m];
    });
    const args: Record<string, unknown> = { valence: Math.round(valence * 100) / 100, energy, reasons };
    if (note) args.note = note;
    void enqueue('mood_log', args);
  },
  /** Give a task a time today (`HH:MM`). */
  schedule(task: Task, time: string) {
    const d = new Date();
    const at = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}T${time}`;
    patchToday((t) => {
      eachTask(t, (x) => {
        if (x.id === task.id) x.scheduled = at;
      });
    });
    void enqueue('task_update', { id: task.id, scheduled: at });
  },
  inboxHandled(item: InboxItem, handled = true) {
    qc.setQueryData<InboxItem[]>(['inbox'], (old) => old?.map((i) => (i.id === item.id ? { ...i, handled } : i)));
    void enqueue('inbox_mark', { id: item.id, handled });
  },
};
