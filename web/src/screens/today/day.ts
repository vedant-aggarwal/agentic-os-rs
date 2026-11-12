// The shape of today, computed from the server's day: what is on now, what is next, what has no
// time yet and how much free time is left before bed (21:00, SYSTEM rule 9). Tasks carry a start
// time but no length, so a block runs until the next timed item, between 15 and 90 minutes.
import { useEffect, useMemo, useState } from 'react';
import { useToday } from '@/lib/queries';
import { hhmm, minutesOf, pillarOf, scheduledTime } from '@/lib/meta';
import type { TaskNode } from '@/lib/types';

export const BED = 21 * 60;
export const WAKE = 5 * 60;

export interface Block {
  key: string;
  title: string;
  start: number;
  end: number;
  hue: string;
  area: string | null;
  areaId: string | null;
  task?: TaskNode;
  kind: 'task' | 'reminder';
  done: boolean;
}

export function dur(min: number): string {
  const m = Math.max(0, Math.round(min));
  if (m < 60) return `${m}m`;
  const h = Math.floor(m / 60);
  const r = m % 60;
  return r ? `${h}h ${r}m` : `${h}h`;
}

export const hm = (min: number) => `${String(Math.floor(min / 60) % 24).padStart(2, '0')}:${String(Math.round(min) % 60).padStart(2, '0')}`;

/** Minutes since midnight, refreshed every 30 s. */
export function useNow(): number {
  const get = () => {
    const d = new Date();
    return d.getHours() * 60 + d.getMinutes() + d.getSeconds() / 60;
  };
  const [n, setN] = useState(get);
  useEffect(() => {
    const t = setInterval(() => setN(get()), 30000);
    return () => clearInterval(t);
  }, []);
  return n;
}

export function useDay() {
  const { data: today } = useToday();
  const now = useNow();
  return useMemo(() => {
    const blocks: Block[] = [];
    const unplaced: { task: TaskNode; area: string | null; hue: string }[] = [];
    let done = 0;
    let total = 0;
    for (const a of today?.areas || []) {
      const p = pillarOf(a.pillar_id);
      for (const t of a.tasks) {
        total++;
        if (t.status === 'done') done++;
        const time = scheduledTime(t.scheduled);
        const open = t.status === 'open' || t.status === 'doing';
        if (time) blocks.push({ key: t.id, title: t.title, start: minutesOf(time), end: 0, hue: p?.hue || 'var(--color-calm)', area: a.area, areaId: a.area_id, task: t, kind: 'task', done: !open });
        else if (open) unplaced.push({ task: t, area: a.area, hue: p?.hue || 'var(--color-ink-2)' });
      }
    }
    for (const al of today?.alarms || []) {
      if (al.status !== 'pending' && al.status !== 'scheduled') continue;
      const d = new Date(al.at);
      blocks.push({ key: al.id, title: al.label, start: minutesOf(hhmm(d)), end: 0, hue: 'var(--color-xp)', area: null, areaId: null, kind: 'reminder', done: false });
    }
    blocks.sort((a, b) => a.start - b.start);
    blocks.forEach((b, i) => {
      const next = blocks.slice(i + 1).find((x) => x.start > b.start);
      const len = b.kind === 'reminder' ? 15 : next ? Math.min(90, Math.max(15, next.start - b.start)) : 60;
      b.end = b.start + len;
    });
    const current = blocks.find((b) => b.kind === 'task' && !b.done && b.start <= now && now < b.end) || null;
    const next = blocks.find((b) => !b.done && b.start > now && b !== current) || null;
    const booked = blocks.filter((b) => !b.done && b.end > now && b.start < BED).reduce((s, b) => s + (Math.min(b.end, BED) - Math.max(b.start, now)), 0);
    const free = Math.max(0, BED - now - booked);
    const asleep = now >= BED || now < WAKE;
    const evening = now >= BED - 30 && !asleep;
    return { ready: !!today, now, blocks, current, next, unplaced, free, asleep, evening, done, total };
  }, [today, now]);
}
