// `?demo=1`: a full, generic day so every screen can be seen at capacity without a server.
// Nothing here is anyone's real data. Writes change this in-memory copy only.
import type {
  AreaList, AreaTasks, Habit, InboxItem, Level, MissionOverview, Mood, NoraMsg, Profile, Task, TaskNode, Today, XpSummary,
} from './types';

const pad = (n: number) => String(n).padStart(2, '0');
const now = new Date();
const ymd = `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;
const at = (h: number, m = 0) => `${ymd}T${pad(h)}:${pad(m)}`;
/** A time relative to now, on a 5-minute grid, so the demo always has a block in progress. */
const rel = (min: number) => {
  const t = Math.max(0, Math.min(23 * 60 + 55, Math.round((now.getHours() * 60 + now.getMinutes() + min) / 5) * 5));
  return at(Math.floor(t / 60), t % 60);
};
let seq = 100;
const id = () => `demo${(seq++).toString(36)}`;

function task(title: string, area: string, o: Partial<Task> = {}): Task {
  return {
    id: id(), parent_id: null, area_id: area, project_id: null, title, notes: null, status: 'open',
    due: ymd, scheduled: null, xp_weight: 1, skip_reason: null, ...o,
  };
}

function node(t: Task, subs: Task[] = []): TaskNode {
  const n: TaskNode = { ...t };
  if (subs.length) {
    n.subtasks = subs.map((s) => ({ ...s, parent_id: t.id }));
    n.rollup = { total: subs.length, done: subs.filter((s) => s.status === 'done').length, skipped: 0 };
  }
  return n;
}

const areas: AreaTasks[] = [
  {
    area_id: 'fitness', area: 'Fitness', pillar_id: 'body',
    tasks: [
      node(task('Push day: chest, shoulders, triceps', 'fitness', { scheduled: at(6, 15), xp_weight: 2, status: 'done' }), [
        task('Bench 4 x 8', 'fitness', { status: 'done' }),
        task('Overhead press 3 x 10', 'fitness', { status: 'done' }),
        task('Dips to failure', 'fitness', { status: 'done' }),
      ]),
      node(task('10k steps', 'fitness')),
    ],
  },
  {
    area_id: 'health', area: 'Health and sleep', pillar_id: 'body',
    tasks: [node(task('Lights out by 21:00', 'health', { scheduled: at(21, 0) }))],
  },
  {
    area_id: 'work', area: 'Studio and clients', pillar_id: 'wealth',
    tasks: [
      node(task('Ship the onboarding flow', 'work', { scheduled: rel(-35), xp_weight: 3 }), [
        task('Fix the empty state copy', 'work', { status: 'done' }),
        task('Record a 30 s walkthrough', 'work'),
        task('Send the build for review', 'work'),
      ]),
      node(task('Reply to the client brief', 'work', { scheduled: rel(40) })),
    ],
  },
  {
    area_id: 'business', area: 'Online store', pillar_id: 'wealth',
    tasks: [node(task('Upload 12 new listings', 'business', { xp_weight: 2 }))],
  },
  {
    area_id: 'learning', area: 'Learning and college', pillar_id: 'mind',
    tasks: [
      node(task('Statistics assignment, part 2', 'learning', { scheduled: rel(150), xp_weight: 2 })),
      node(task('Revise chapter 4 notes', 'learning', { status: 'skipped', skip_reason: 'tired' })),
    ],
  },
  {
    area_id: 'reading', area: 'Reading', pillar_id: 'mind',
    tasks: [node(task('Read 20 pages', 'reading', { scheduled: at(20, 15) }))],
  },
];

function habit(id: string, name: string, area: string, o: Partial<Habit> = {}): Habit {
  return {
    id, name, area_id: area, anchor: false, cadence: 'daily', cue: null, floor: null, normal: null, xp: 10,
    due_today: true, today: null, streak: { best: 9, current: 4, last_done: ymd, state: 'intact' }, ...o,
  };
}

const habits: Habit[] = [
  habit('journal', 'Journal', 'mindset', { anchor: true, cue: 'after I get into bed', floor: 'one honest sentence', normal: 'close the day with Nora', xp: 15, streak: { best: 21, current: 12, last_done: ymd, state: 'intact' } }),
  habit('gym', 'Gym or outdoors', 'fitness', { today: 'done', cue: 'at 06:00', floor: '20 min walk', normal: 'the full split', xp: 15, streak: { best: 14, current: 6, last_done: ymd, state: 'intact' } }),
  habit('meditate', 'Meditate', 'mindset', { cue: 'after waking', floor: '2 min', normal: '10 min', streak: { best: 8, current: 3, last_done: null, state: 'bent' } }),
  habit('read', 'Read', 'reading', { cue: 'after dinner', floor: '1 page', normal: '10 pages', streak: { best: 30, current: 17, last_done: ymd, state: 'intact' } }),
  habit('shutdown', 'Shutdown routine', 'health', { cue: 'before bed', floor: 'brush teeth', normal: 'room, brush, face wash, phone docked', streak: { best: 5, current: 0, last_done: null, state: 'broken' } }),
];

function level(total: number): Level {
  let L = 1;
  while (50 * (L + 1) * L <= total) L++;
  const floor = 50 * L * (L - 1);
  const size = 50 * (L + 1) * L - floor;
  const into = total - floor;
  return {
    level: L, level_exact: L + into / size, into_level: into, level_size: size, progress: into / size, to_next: size - into,
    total_xp: total, stage: Math.min(20, Math.floor((L - 1) / 3) + 1), stage_progress: ((L - 1) % 3) / 3, evolution: Math.min(1, (L - 1) / 57),
  };
}

const state = {
  xp: 1760,
  xpToday: 85,
  moods: [] as Mood[],
};

const profile: Profile = {
  name: 'Arjun',
  avatar: 'default',
  companion: true,
  private_habits: [],
  hero: [
    { slot: 'physical', goal_id: 'bike', name: 'Retro twin motorcycle', variant: 'Matte black', about: 'Body, strength, sleep and how you carry yourself.', price: 320000, price_note: null, earned: 41000, progress: 41000 / 320000, specs: ['Engine: 650 cc parallel twin', 'Power: 47 hp', 'Gearbox: 6-speed', 'Kerb weight: 218 kg'], why: 'Riding to the hills on a Sunday morning, on something I paid for with my own work.' },
    { slot: 'money', goal_id: 'laptop', name: '16-inch laptop', variant: 'Space black, 48 GB, 1 TB', about: 'Work and money: clients, the store and every agent session.', price: 480000, price_note: 'You earn this one yourself.', earned: 132000, progress: 132000 / 480000, specs: ['Chip: 18-core CPU, 20-core GPU', 'Memory: 48 GB', 'Storage: 1 TB', 'Display: 16-inch XDR'], why: 'The machine that pays for everything else.' },
    { slot: 'mind', goal_id: 'companion', name: 'Companion', variant: 'Faint for now. She grows clearer as you level up.', about: 'Personality, how you speak, confidence, knowledge and peace of mind.', price: null, price_note: null, earned: 0, progress: 0.32, specs: ['Personality: Who you are when nobody is grading you', 'Speaking: How you talk and hold a room', 'Knowledge: What you read and learn', 'Peace: A calm mind, most days'], why: null },
  ],
};

function today(): Today {
  const flat = areas.flatMap((a) => a.tasks);
  const done = flat.filter((t) => t.status === 'done').length;
  const skipped = flat.filter((t) => t.status === 'skipped').length;
  const open = flat.filter((t) => t.status === 'open' || t.status === 'doing').length;
  return {
    date: ymd,
    score: { counted: done + skipped + open, done, open, skipped, rate: done / Math.max(1, done + skipped + open) },
    areas: structuredClone(areas),
    habits: structuredClone(habits),
    moods: state.moods,
    journal_written: false,
    alarms: [{ id: 'a1', at: new Date(now.getFullYear(), now.getMonth(), now.getDate(), 13, 30).toISOString(), label: 'Call the printer about the catalog', kind: 'reminder', status: 'pending', task_id: null }],
    xp: { level: level(state.xp), today: state.xpToday },
    goals: [],
  };
}

const areaList: AreaList = {
  pillars: [
    { id: 'body', name: 'Strongest', sort: 1, areas: [
      { id: 'fitness', name: 'Fitness', pillar_id: 'body', color: null, sort: 1, archived: false },
      { id: 'health', name: 'Health and sleep', pillar_id: 'body', color: null, sort: 2, archived: false },
      { id: 'care', name: 'Looks and care', pillar_id: 'body', color: null, sort: 3, archived: false },
    ] },
    { id: 'wealth', name: 'Richest', sort: 2, areas: [
      { id: 'work', name: 'Studio and clients', pillar_id: 'wealth', color: null, sort: 4, archived: false },
      { id: 'business', name: 'Online store', pillar_id: 'wealth', color: null, sort: 5, archived: false },
      { id: 'skills', name: 'Skills that earn', pillar_id: 'wealth', color: null, sort: 6, archived: false },
    ] },
    { id: 'mind', name: 'Smartest', sort: 3, areas: [
      { id: 'learning', name: 'Learning and college', pillar_id: 'mind', color: null, sort: 7, archived: false },
      { id: 'reading', name: 'Reading', pillar_id: 'mind', color: null, sort: 8, archived: false },
      { id: 'mindset', name: 'Mind and discipline', pillar_id: 'mind', color: null, sort: 9, archived: false },
    ] },
  ],
  unfiled: [],
};

function xpSummary(): XpSummary {
  const per: Record<string, number> = { fitness: 420, health: 160, care: 60, work: 510, business: 230, skills: 40, learning: 180, reading: 120, mindset: 40 };
  return {
    overall: level(state.xp),
    pillars: [
      { id: 'body', name: 'Strongest', level: 4, xp: 640 },
      { id: 'wealth', name: 'Richest', level: 4, xp: 780 },
      { id: 'mind', name: 'Smartest', level: 3, xp: 340 },
    ],
    areas: areaList.pillars.flatMap((p) => p.areas.map((a) => ({ id: a.id, name: a.name, pillar_id: p.id, level: 1 + Math.floor(per[a.id] / 150), xp: per[a.id] }))),
    today: state.xpToday,
    week: 410,
    unassigned: 0,
  };
}

const minsAgo = (m: number) => new Date(Date.now() - m * 60000).toISOString();
const inbox: InboxItem[] = [
  { id: 'i1', source: 'whatsapp', account: null, at: minsAgo(6), chat: 'c1', chat_name: 'Studio team', sender: null, sender_name: 'Rohan', subject: null, text: 'Client approved the second draft. Can we ship Friday?', from_me: false, handled: false, note: null, triage_yes: true, triage_score: 0.93 },
  { id: 'i2', source: 'gmail', account: 'main', at: minsAgo(48), chat: null, chat_name: null, sender: 'billing@example.com', sender_name: 'Hosting billing', subject: 'Your invoice for November', text: 'Your invoice is ready. Amount due in 7 days.', from_me: false, handled: false, note: null, triage_yes: true, triage_score: 0.71 },
  { id: 'i3', source: 'whatsapp', account: null, at: minsAgo(95), chat: 'c2', chat_name: 'Mom', sender: null, sender_name: 'Mom', subject: null, text: 'Dinner at 8. Bring curd on the way back.', from_me: false, handled: false, note: null, triage_yes: true, triage_score: 0.88 },
  { id: 'i4', source: 'gmail', account: 'main', at: minsAgo(300), chat: null, chat_name: null, sender: 'news@example.com', sender_name: 'Product Weekly', subject: 'Five launches worth your time', text: 'This week: a new design tool, two agent frameworks and more.', from_me: false, handled: false, note: null, triage_yes: false, triage_score: 0.12 },
  { id: 'i5', source: 'app', account: null, at: minsAgo(520), chat: null, chat_name: null, sender: null, sender_name: 'Bank', subject: null, text: '₹2,400 credited from a client payment.', from_me: false, handled: false, note: null, triage_yes: true, triage_score: 0.66 },
];

const thread: NoraMsg[] = [
  { id: 1, engine: 'claude', source: 'app', kind: 'message', text: 'Morning. Gym is done, nice. Statistics at 11 is the one that matters today; the rest can bend.', at: Date.now() - 3 * 3600e3, turn_id: null },
  { id: 2, engine: 'user', source: 'app', kind: 'message', text: 'Remind me to reply to the brief after lunch', at: Date.now() - 2.9 * 3600e3, turn_id: null },
  { id: 3, engine: 'claude', source: 'app', kind: 'message', text: 'Set for 17:30, right after the onboarding block. I put it under Studio and clients.', at: Date.now() - 2.9 * 3600e3 + 9000, turn_id: null },
  { id: 4, engine: 'grok', source: 'grok', kind: 'message', text: 'You skipped chapter 4 again. Tired is fine once. Want me to move it to Saturday morning?', at: Date.now() - 40 * 60e3, turn_id: null },
];

function findTask(tid: string): Task | undefined {
  for (const a of areas) for (const t of a.tasks) {
    if (t.id === tid) return t;
    const s = t.subtasks?.find((x) => x.id === tid);
    if (s) return s;
  }
}

const delay = <T,>(v: T, ms = 140): Promise<T> => new Promise((r) => setTimeout(() => r(structuredClone(v)), ms));

export function demoCall<T>(name: string, args: Record<string, unknown>): Promise<T> {
  const a = args as Record<string, string | number | boolean | undefined>;
  switch (name) {
    case 'today': return delay(today() as T);
    case 'profile': return delay(profile as T);
    case 'xp_summary': return delay(xpSummary() as T);
    case 'area_list': return delay(areaList as T);
    case 'orb_state': return delay({ color: '#6DAEFF', intensity: 0.7, label: 'Steady', reasons: ['Gym done', '4 tasks open'], state: 'calm' } as T);
    case 'inbox_list': return delay({ items: inbox.filter((i) => !i.handled) } as T);
    case 'task_list': return delay({ tasks: [task('Book the dentist', ''), task('Renew the domain', 'work', { due: null })] } as T);
    case 'mission_overview': return delay({ boards: 2, dreams: 5, drops_open: 3, goals_open: 4, proposals_pending: 1, rules: 6, rules_kept_rate: 0.67, seeds: 9, statements: 7, steps_done_7: 5, vote_days_7: 4 } as MissionOverview as T);
    case 'mission_proposal_list': return delay({ proposals: [] } as T);
    case 'journal_read': return delay({ entries: [{ id: 'j1', date: ymd, kind: 'journal', text: 'Hard day, but I showed up. Skipped revision because I was wiped after the gym. Tomorrow: revise before lunch, not after.' }] } as T);
    case 'task_done': {
      const t = findTask(String(a.id));
      if (t) t.status = 'done';
      state.xp += 10 * (t?.xp_weight || 1);
      state.xpToday += 10 * (t?.xp_weight || 1);
      return delay({ task: t, xp: 10 * (t?.xp_weight || 1) } as T);
    }
    case 'task_skip': {
      const t = findTask(String(a.id));
      if (t) { t.status = 'skipped'; t.skip_reason = String(a.reason || ''); }
      return delay({ task: t } as T);
    }
    case 'task_update': {
      const t = findTask(String(a.id));
      if (t && a.status) t.status = a.status as Task['status'];
      return delay({ task: t } as T);
    }
    case 'task_add': {
      const t = task(String(a.title), String(a.area || ''), { parent_id: (a.parent as string) || null });
      const bucket = areas.find((x) => x.area_id === a.area);
      if (a.parent) {
        const p = findTask(String(a.parent)) as TaskNode | undefined;
        if (p) (p.subtasks ||= []).push(t);
      } else if (bucket) bucket.tasks.push(node(t));
      return delay({ task: t } as T);
    }
    case 'habit_log': {
      const h = habits.find((x) => x.id === a.habit);
      if (h) {
        h.today = (a.status as Habit['today']) || 'done';
        if (h.today === 'done' || h.today === 'floor') { h.streak.current += 1; h.streak.state = 'intact'; }
      }
      return delay({ habit: h } as T);
    }
    case 'mood_log': {
      const m: Mood = { id: id(), date: ymd, at: new Date().toISOString(), valence: Number(a.valence ?? 0), energy: (a.energy as number) ?? null, labels: [], reasons: (args.reasons as string[]) || [], note: (a.note as string) || null };
      state.moods = [...state.moods, m];
      return delay({ mood: m } as T);
    }
    case 'inbox_mark': {
      const i = inbox.find((x) => x.id === a.id);
      if (i) i.handled = a.handled !== false;
      return delay({ ok: true } as T);
    }
    default:
      return delay({} as T);
  }
}

export function demoGet<T>(path: string): Promise<T> {
  if (path.startsWith('/api/nora/thread')) return delay(thread as T);
  if (path.startsWith('/api/work/projects')) return delay([
    { id: 'p1', name: 'storefront-app', folder: '', source: 't3' },
    { id: 'p2', name: 'client-portal', folder: '', source: 't3' },
    { id: 'p3', name: 'content-os', folder: '', source: 't3' },
  ] as T);
  if (path.startsWith('/api/work/threads')) return delay([
    { id: 't1', project_id: 'p1', title: 'Checkout: fix the coupon rounding', status: 'running', provider: 'claude', updated_at: Date.now() - 5 * 60e3 },
    { id: 't2', project_id: 'p2', title: 'Invoice PDF export', status: 'waiting', provider: 'codex', pending_approvals: 1, updated_at: Date.now() - 50 * 60e3 },
    { id: 't3', project_id: 'p1', title: 'Keyboard shortcuts', status: 'waiting', provider: 'claude', pending_approvals: 2, updated_at: Date.now() - 12 * 60e3 },
  ] as T);
  if (path.startsWith('/api/nora/heartbeats')) return delay([
    { id: 7, at: Date.now() - 18 * 60e3, decision: 'note', reason: 'Reels at 18:00 three days running', text: 'Instagram pulled you in at 18:00 three days running. Lock it till the gym is done?', model: '', sig: '' },
  ] as T);
  if (path.startsWith('/api/sync/push')) return delay({ results: [], latest: 0 } as T);
  return delay({} as T);
}
