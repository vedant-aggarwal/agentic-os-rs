// Shapes the Rust server returns (crates/aos-core model.rs, today.rs, xp.rs; aos-tools profile;
// aos-inbox; aos-nora store). Only the fields the UI reads are typed.

export type PillarId = 'body' | 'wealth' | 'mind';
export type Slot = 'physical' | 'money' | 'mind';
export type TaskStatus = 'open' | 'doing' | 'done' | 'skipped' | 'dropped';
export type HabitLogStatus = 'done' | 'floor' | 'missed' | 'excused';
export type StreakState = 'intact' | 'bent' | 'broken';

export interface Task {
  id: string;
  parent_id: string | null;
  area_id: string | null;
  project_id: string | null;
  title: string;
  notes: string | null;
  status: TaskStatus;
  due: string | null;
  /** `YYYY-MM-DD` or `YYYY-MM-DDTHH:MM`, local. */
  scheduled: string | null;
  xp_weight: number;
  skip_reason: string | null;
  sort?: number;
  /** Set on the client while an outbox op for it is waiting. */
  pending?: boolean;
}

export interface TaskNode extends Task {
  subtasks?: Task[];
  rollup?: { total: number; done: number; skipped: number };
}

export interface AreaTasks {
  area_id: string | null;
  area: string | null;
  pillar_id: string | null;
  tasks: TaskNode[];
}

export interface Streak {
  best: number;
  current: number;
  last_done: string | null;
  state: StreakState;
}

export interface Habit {
  id: string;
  name: string;
  area_id: string | null;
  anchor: boolean;
  cadence: string;
  cue: string | null;
  floor: string | null;
  normal: string | null;
  xp: number;
  due_today: boolean;
  today: HabitLogStatus | null;
  streak: Streak;
}

export interface Level {
  level: number;
  level_exact: number;
  into_level: number;
  level_size: number;
  progress: number;
  to_next: number;
  total_xp: number;
  stage: number;
  stage_progress: number;
  evolution: number;
}

export interface Mood {
  id: string;
  date: string;
  at: string;
  valence: number;
  energy: number | null;
  labels: string[];
  reasons: string[];
  note: string | null;
}

export interface Alarm {
  id: string;
  at: string;
  label: string;
  kind: string;
  status: string;
  task_id: string | null;
}

export interface Goal {
  id: string;
  name: string;
  pillar_id: PillarId | null;
  area_id: string | null;
  earned_paise: number;
  target_paise: number | null;
  remaining_paise: number | null;
  progress: number;
  specs: string[];
  images: string[];
  status: string;
  why: string | null;
}

export interface Today {
  date: string;
  score: { counted: number; done: number; open: number; rate: number; skipped: number };
  areas: AreaTasks[];
  habits: Habit[];
  moods: Mood[];
  journal_written: boolean;
  alarms: Alarm[];
  xp: { level: Level; today: number };
  goals: Goal[];
}

export interface HeroSlot {
  slot: Slot;
  goal_id: string | null;
  name: string;
  variant: string | null;
  about: string | null;
  price: number | null;
  price_note: string | null;
  earned: number;
  progress: number;
  specs: string[];
  why: string | null;
}

export interface Profile {
  name: string;
  avatar: string;
  companion: boolean;
  private_habits: string[];
  hero: HeroSlot[];
}

export interface Area {
  id: string;
  name: string;
  pillar_id: PillarId;
  color: string | null;
  sort: number;
  archived: boolean;
}

export interface Pillar {
  id: PillarId;
  name: string;
  sort: number;
  areas: Area[];
}

export interface AreaList {
  pillars: Pillar[];
  unfiled: Area[];
}

export interface XpSummary {
  overall: Level;
  pillars: { id: PillarId; name: string; level: number; xp: number }[];
  areas: { id: string; name: string; pillar_id: PillarId; level: number; xp: number }[];
  today: number;
  week: number;
  unassigned: number;
}

export interface InboxItem {
  id: string;
  source: 'whatsapp' | 'gmail' | 'app' | string;
  account: string | null;
  at: string;
  chat: string | null;
  chat_name: string | null;
  sender: string | null;
  sender_name: string | null;
  subject: string | null;
  text: string;
  from_me: boolean;
  handled: boolean;
  note: string | null;
  triage_yes: boolean | null;
  triage_score: number | null;
}

export interface OrbState {
  color: string;
  intensity: number;
  label: string;
  reasons: string[];
  state: string;
}

export interface NoraMsg {
  id: number;
  engine: 'user' | 'claude' | 'grok' | 'system' | string;
  source: string;
  kind: string;
  text: string;
  at: number;
  turn_id: string | null;
}

export type NoraEvent =
  | { type: 'message'; msg: NoraMsg }
  | { type: 'message_updated'; msg: NoraMsg }
  | { type: 'turn_started'; turn_id: string }
  | { type: 'delta'; turn_id: string; text: string }
  | { type: 'tool'; turn_id: string; name: string; title: string; done: boolean }
  | { type: 'turn_done' }
  | { type: 'orb_state'; orb: OrbState }
  | { type: string; [k: string]: unknown };

export interface JournalEntry {
  id: string;
  date: string;
  kind: string;
  text: string;
  at?: string;
}

export interface MissionOverview {
  boards: number;
  dreams: number;
  drops_open: number;
  goals_open: number;
  proposals_pending: number;
  rules: number;
  rules_kept_rate: number | null;
  seeds: number;
  statements: number;
  steps_done_7: number;
  vote_days_7: number;
}

export interface WorkProject {
  id: string;
  name: string;
  folder: string;
  source: string;
}

export interface WorkThread {
  id: string;
  project_id?: string | null;
  title?: string;
  status?: string;
  provider?: string;
  updated_at?: number;
  /** 't3' for T3 Code's own threads (read only) */
  source?: string;
  preview?: string;
  [k: string]: unknown;
}

export interface WorkItem {
  id: string;
  kind: 'user_message' | 'assistant_message' | 'reasoning' | 'tool_call' | 'plan' | 'error' | 'system';
  status: string;
  title: string;
  text: string;
  created_at: number;
}

export interface WorkApproval {
  id: string;
  state: 'pending' | 'approved' | 'denied' | 'expired';
  title: string;
  detail: string;
}

export interface WorkThreadDetail {
  thread: { id: string; title: string; provider: string; status: string; last_error?: string | null; project_id: string };
  project?: WorkProject | null;
  items: WorkItem[];
  approvals: WorkApproval[];
}

export interface T3ThreadDetail {
  id: string;
  project_id: string | null;
  title: string;
  provider: string;
  status: string;
  updated_at: number;
  messages: { role: string; text: string; created_at: number }[];
}

export interface Paired {
  token: string;
  device: { id: string; name: string; platform: string | null };
}
