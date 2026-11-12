// Habits and streaks. The anchor (journaling) leads. Tap = done, tap again = the floor version
// (it still counts), again = clear. Never miss twice: one miss bends a streak, two end it.
import { motion } from 'motion/react';
import { useRef } from 'react';
import { Anchor, Check, Flame } from 'lucide-react';
import { act, useProfile, useToday } from '@/lib/queries';
import { areaIcon, pillarOf } from '@/lib/meta';
import type { Habit } from '@/lib/types';
import { haptic, useUI } from '@/lib/ui';
import { Section } from '@/ui/kit';
import { Ring } from '@/ui/Ring';
import { floatXp } from '@/ui/XpFloat';

const AREA_PILLAR: Record<string, string> = {
  fitness: 'body', health: 'body', care: 'body', work: 'wealth', business: 'wealth', skills: 'wealth', learning: 'mind', reading: 'mind', mindset: 'mind',
};

function streakLine(h: Habit): string {
  const n = h.streak.current;
  if (h.today === 'done') return n > 1 ? `${n}-day streak` : 'Done today';
  if (h.today === 'floor') return `Floor counts · ${n}-day streak`;
  if (h.streak.state === 'bent') return 'Missed one. Today keeps it alive';
  if (h.streak.state === 'broken' || n === 0) return h.streak.best > 0 ? `Start again · best ${h.streak.best}` : 'Start today';
  return `${n}-day streak`;
}

export function HabitsSection() {
  const { data: today } = useToday();
  const { data: profile } = useProfile();
  const safe = useUI((s) => s.safe);
  if (!today) return null;
  const hidden = new Set(safe ? profile?.private_habits || [] : []);
  const habits = today.habits.filter((h) => h.due_today && !hidden.has(h.id));
  if (!habits.length) return null;
  const anchor = habits.find((h) => h.anchor);
  const rest = habits.filter((h) => h !== anchor);
  const done = habits.filter((h) => h.today === 'done' || h.today === 'floor').length;

  return (
    <Section title="Habits" meta={`${done} of ${habits.length}`}>
      {anchor && <AnchorCard h={anchor} />}
      <div className="mx-gutter mt-3 grid grid-cols-2 gap-3">
        {rest.map((h) => (
          <HabitTile key={h.id} h={h} />
        ))}
      </div>
    </Section>
  );
}

function cycle(h: Habit, el: Element | null) {
  const next = h.today === null || h.today === 'missed' || h.today === 'excused' ? 'done' : h.today === 'done' && h.floor ? 'floor' : null;
  const xp = act.habit(h, next);
  haptic(next ? 'success' : 'light');
  floatXp(el, xp);
}

function hueOf(h: Habit) {
  return pillarOf(AREA_PILLAR[h.area_id || ''])?.hue || 'var(--color-xp)';
}

function AnchorCard({ h }: { h: Habit }) {
  const ref = useRef<HTMLButtonElement>(null);
  const on = h.today === 'done' || h.today === 'floor';
  const hue = hueOf(h);
  return (
    <button ref={ref} onClick={() => cycle(h, ref.current)} className="stone press relative mx-gutter flex w-[calc(100%-32px)] items-center gap-4 overflow-hidden rounded-xl p-4 text-left">
      {on && <div aria-hidden className="absolute inset-0" style={{ background: `radial-gradient(80% 120% at 0% 50%, color-mix(in oklab, ${hue} 22%, transparent), transparent 70%)` }} />}
      <Ring size={52} stroke={4} value={on ? 1 : Math.min(1, h.streak.current / 30)} color={hue} glow={on}>
        {on ? <Check size={20} strokeWidth={3} style={{ color: hue }} /> : <Anchor size={19} className="text-ink-2" />}
      </Ring>
      <span className="relative min-w-0 flex-1">
        <span className="flex items-center gap-2">
          <span className="text-headline font-semibold">{h.name}</span>
          <span className="rounded-full bg-fill-2 px-2 py-0.5 text-caption font-semibold text-ink-2">Anchor</span>
        </span>
        <span className="mt-0.5 block truncate text-subhead text-ink-2">{streakLine(h)}</span>
        {h.cue && <span className="mt-0.5 block truncate text-footnote text-ink-3">{h.today === 'floor' ? `Floor: ${h.floor}` : `${h.cue[0].toUpperCase()}${h.cue.slice(1)}`}</span>}
      </span>
      <span className="relative flex flex-col items-end">
        <span className="fig flex items-center gap-1 text-[26px] leading-none" style={{ color: h.streak.current ? 'var(--color-xp-ink)' : 'var(--color-ink-3)' }}>
          <Flame size={18} strokeWidth={2.2} />
          {h.streak.current}
        </span>
        <span className="mt-1 text-caption text-ink-3">best {h.streak.best}</span>
      </span>
    </button>
  );
}

function HabitTile({ h }: { h: Habit }) {
  const ref = useRef<HTMLButtonElement>(null);
  const on = h.today === 'done' || h.today === 'floor';
  const hue = hueOf(h);
  const Icon = areaIcon(h.area_id);
  return (
    <motion.button ref={ref} whileTap={{ scale: 0.96 }} onClick={() => cycle(h, ref.current)} className="stone relative flex min-h-[112px] flex-col overflow-hidden rounded-[22px] p-3.5 text-left">
      {on && <div aria-hidden className="absolute inset-0" style={{ background: `radial-gradient(100% 80% at 0% 0%, color-mix(in oklab, ${hue} 24%, transparent), transparent 70%)` }} />}
      <span className="relative flex items-center justify-between">
        <span className="grid size-9 place-items-center rounded-full" style={{ background: on ? hue : 'var(--color-fill-2)', color: on ? 'var(--color-ground)' : `color-mix(in oklab, ${hue} 75%, white)`, boxShadow: on ? `0 0 16px color-mix(in oklab, ${hue} 60%, transparent)` : undefined }}>
          {on ? <Check size={17} strokeWidth={3} /> : <Icon size={17} strokeWidth={2.1} />}
        </span>
        <span className="fig flex items-center gap-0.5 text-[17px]" style={{ color: h.streak.current ? 'var(--color-xp-ink)' : 'var(--color-ink-3)' }}>
          <Flame size={14} strokeWidth={2.4} />
          {h.streak.current}
        </span>
      </span>
      <span className="relative mt-auto pt-3 text-headline font-semibold">{h.name}</span>
      <span className={`relative mt-0.5 text-footnote ${h.streak.state === 'bent' && !on ? 'text-needs' : 'text-ink-2'}`}>{streakLine(h)}</span>
    </motion.button>
  );
}
