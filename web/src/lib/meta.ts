// Pillars, slots, levels and the small formatters every screen shares.
import { Brain, Dumbbell, BookOpen, Briefcase, Store, Sparkles, HeartPulse, Scissors, GraduationCap, type LucideIcon } from 'lucide-react';
import type { PillarId, Slot } from './types';

export interface PillarMeta {
  id: PillarId;
  slot: Slot;
  label: string; // the goal pillar as the hero names it
  hue: string;
  ink: string;
  glow: string;
}

export const PILLARS: Record<PillarId, PillarMeta> = {
  body: { id: 'body', slot: 'physical', label: 'Physical', hue: 'var(--color-phys)', ink: 'var(--color-phys-ink)', glow: '#ff5a4e' },
  wealth: { id: 'wealth', slot: 'money', label: 'Money', hue: 'var(--color-money)', ink: 'var(--color-money-ink)', glow: '#3fd0ff' },
  mind: { id: 'mind', slot: 'mind', label: 'Mind', hue: 'var(--color-mind)', ink: 'var(--color-mind-ink)', glow: '#9b8cff' },
};
export const PILLAR_ORDER: PillarId[] = ['body', 'wealth', 'mind'];

export const SLOT_PILLAR: Record<Slot, PillarId> = { physical: 'body', money: 'wealth', mind: 'mind' };

export function pillarOf(id: string | null | undefined): PillarMeta | null {
  return id && id in PILLARS ? PILLARS[id as PillarId] : null;
}

const AREA_ICONS: Record<string, LucideIcon> = {
  fitness: Dumbbell,
  health: HeartPulse,
  care: Scissors,
  work: Briefcase,
  business: Store,
  skills: Sparkles,
  learning: GraduationCap,
  reading: BookOpen,
  mindset: Brain,
};
export function areaIcon(id: string | null | undefined): LucideIcon {
  return (id && AREA_ICONS[id]) || Sparkles;
}

/** 3,64,856 style grouping (Indian). */
export function rupees(n: number | null | undefined): string {
  if (n == null) return '';
  return '₹' + Math.round(n).toLocaleString('en-IN');
}

export function hhmm(d: Date): string {
  return d.toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit', hour12: false });
}

/** "09:40" from a local `YYYY-MM-DDTHH:MM`, or null when the task has a day only. */
export function scheduledTime(s: string | null): string | null {
  if (!s || s.length < 16) return null;
  return s.slice(11, 16);
}

export function minutesOf(t: string): number {
  const [h, m] = t.split(':').map(Number);
  return h * 60 + m;
}

export function ago(iso: string | number): string {
  const t = typeof iso === 'number' ? iso : Date.parse(iso);
  const m = Math.round((Date.now() - t) / 60000);
  if (m < 1) return 'now';
  if (m < 60) return `${m}m`;
  const h = Math.round(m / 60);
  if (h < 24) return `${h}h`;
  const d = Math.round(h / 24);
  return d < 7 ? `${d}d` : new Date(t).toLocaleDateString('en-GB', { day: 'numeric', month: 'short' });
}

/** "Today", "Tomorrow", "Overdue · 9 Nov", "Fri 14 Nov" for a YYYY-MM-DD due date. */
export function dueLabel(ymd: string): string {
  const d = new Date(ymd + 'T00:00:00');
  const t = new Date();
  t.setHours(0, 0, 0, 0);
  const days = Math.round((d.getTime() - t.getTime()) / 86400000);
  const short = d.toLocaleDateString('en-GB', { weekday: 'short', day: 'numeric', month: 'short' });
  if (days === 0) return 'Due today';
  if (days === 1) return 'Due tomorrow';
  if (days < 0) return `Overdue · ${short}`;
  return `Due ${short}`;
}

export function greeting(d = new Date()): string {
  const h = d.getHours();
  if (h < 5) return 'Late night';
  if (h < 12) return 'Good morning';
  if (h < 17) return 'Good afternoon';
  return 'Good evening';
}

/* ═══ levels ═══ */

/** Total XP needed to reach level L (server rule: 50 x L x (L - 1)). */
export const xpFor = (L: number) => 50 * L * (L - 1);

/** The character art that exists for a level (2.5D fallback). */
export function lookFor(level: number): 'l1' | 'l5' | 'l10' | 'l20' {
  if (level >= 20) return 'l20';
  if (level >= 10) return 'l10';
  if (level >= 5) return 'l5';
  return 'l1';
}

export interface Milestone {
  level: number;
  title: string;
  detail: string;
  unlocks: string[];
  companion?: string;
}

export const MILESTONES: Milestone[] = [
  { level: 1, title: 'Day one', detail: 'Where the story starts.', unlocks: ['Your character', 'Daily XP'], companion: 'A faint outline.' },
  { level: 3, title: 'Showing up', detail: 'Three levels of honest days.', unlocks: ['Streak flame on habits', 'Weekly score'] },
  { level: 5, title: 'Leaner', detail: 'Sharper face, fitted hoodie.', unlocks: ['New outfit', 'Second aura layer'], companion: 'Her colour starts to come in.' },
  { level: 7, title: 'Momentum', detail: 'The system runs without you pushing it.', unlocks: ['Mood graph in 3D', 'Pillar themes'] },
  { level: 10, title: 'Fit', detail: 'Athletic build, new wardrobe.', unlocks: ['Bomber jacket and watch', 'Gold level ring'], companion: 'Fully there, in colour.' },
  { level: 15, title: 'Sharp', detail: 'Turtleneck, leather, the walk.', unlocks: ['Night theme', 'Level-up replay'] },
  { level: 20, title: 'Peak', detail: 'Long coat, the full aura.', unlocks: ['Light wings', 'Final form'], companion: 'Vivid, beside you.' },
];

/* ═══ skip reasons (SYSTEM: repair, not blame) ═══ */
export const SKIP_REASONS: { label: string; value: string }[] = [
  { label: 'Tired', value: 'tired' },
  { label: 'Forgot', value: 'forgot' },
  { label: 'Too big', value: 'too big' },
  { label: 'Avoided it', value: 'avoided' },
  { label: 'Phone / reels', value: 'phone' },
  { label: 'Sick', value: 'sick' },
  { label: 'Other', value: 'other' },
];

export const MOOD_REASONS = ['Sleep', 'Food', 'People', 'Work', 'Body', 'Other'];
