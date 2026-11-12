// Compact timeline: timed tasks and reminders around a Now line. Hours already lived fold into
// one row; the rest of the day gets the space.
import { motion } from 'motion/react';
import { useMemo, useState } from 'react';
import { Bell, ChevronDown } from 'lucide-react';
import { useToday } from '@/lib/queries';
import { areaIcon, hhmm, minutesOf, pillarOf, scheduledTime } from '@/lib/meta';
import type { Task } from '@/lib/types';
import { useUI } from '@/lib/ui';
import { rise } from '@/ui/motion';

interface Item {
  key: string;
  time: string;
  title: string;
  hue: string;
  icon: React.ComponentType<{ size?: number; strokeWidth?: number }>;
  done: boolean;
  meta: string;
  task?: Task;
}

export function Timeline() {
  const { data: today } = useToday();
  const push = useUI((s) => s.push);
  const [showPast, setShowPast] = useState(false);
  const nowMin = new Date().getHours() * 60 + new Date().getMinutes();

  const items = useMemo<Item[]>(() => {
    if (!today) return [];
    const out: Item[] = [];
    for (const a of today.areas) {
      const p = pillarOf(a.pillar_id);
      for (const t of a.tasks) {
        const time = scheduledTime(t.scheduled);
        if (!time) continue;
        out.push({
          key: t.id,
          time,
          title: t.title,
          hue: p?.hue || 'var(--color-ink-2)',
          icon: areaIcon(a.area_id),
          done: t.status === 'done',
          meta: [a.area, t.rollup ? `${t.rollup.done}/${t.rollup.total}` : null].filter(Boolean).join(' · '),
          task: t,
        });
      }
    }
    for (const al of today.alarms) {
      if (al.status !== 'pending' && al.status !== 'scheduled') continue;
      const d = new Date(al.at);
      out.push({ key: al.id, time: hhmm(d), title: al.label, hue: 'var(--color-xp)', icon: Bell, done: false, meta: al.kind === 'alarm' ? 'Alarm' : 'Reminder' });
    }
    return out.sort((a, b) => minutesOf(a.time) - minutesOf(b.time));
  }, [today]);

  const past = items.filter((i) => minutesOf(i.time) < nowMin - 30);
  const rest = items.filter((i) => minutesOf(i.time) >= nowMin - 30);
  const nowIdx = rest.findIndex((i) => minutesOf(i.time) > nowMin);

  if (!today) return <div className="skeleton mx-gutter h-40 rounded-xl" />;
  if (!items.length)
    return (
      <div className="stone mx-gutter rounded-xl px-4 py-5 text-subhead text-ink-2">
        Nothing timed today. Give a task a time (<span className="font-mono text-ink">at 18:00</span>) or ask Nora to plan the day.
      </div>
    );

  const row = (i: Item, idx: number, dim: boolean) => {
    const Icon = i.icon;
    return (
      <motion.button
        key={i.key}
        variants={rise}
        initial="hidden"
        animate="show"
        custom={idx}
        onClick={() => i.task?.area_id && push({ kind: 'area', id: i.task.area_id })}
        className={`flex w-full items-center gap-3 py-2.5 text-left ${dim || i.done ? 'opacity-50' : ''}`}
      >
        <span className="w-12 shrink-0 font-mono text-footnote text-ink-2 tnum">{i.time}</span>
        <span className="relative grid size-8 shrink-0 place-items-center rounded-full" style={{ background: `color-mix(in oklab, ${i.hue} 20%, transparent)`, color: `color-mix(in oklab, ${i.hue} 80%, white)` }}>
          <Icon size={15} strokeWidth={2.2} />
        </span>
        <span className="min-w-0 flex-1">
          <span className={`block truncate text-callout font-medium ${i.done ? 'line-through decoration-ink-3' : ''}`}>{i.title}</span>
          {i.meta && <span className="block truncate text-footnote text-ink-2">{i.meta}</span>}
        </span>
      </motion.button>
    );
  };

  return (
    <div className="stone mx-gutter rounded-xl px-4 py-1.5">
      {past.length > 0 && (
        <button onClick={() => setShowPast((v) => !v)} className="hair flex w-full items-center gap-3 py-3 text-left">
          <span className="w-12 shrink-0 font-mono text-footnote text-ink-3 tnum">{past[0].time}</span>
          <span className="flex -space-x-1">
            {past.slice(0, 4).map((p) => (
              <span key={p.key} className="size-2.5 rounded-full ring-2 ring-stone" style={{ background: p.hue }} />
            ))}
          </span>
          <span className="flex-1 text-subhead text-ink-2">
            Earlier · {past.filter((p) => p.done).length} of {past.length} done
          </span>
          <ChevronDown size={16} className={`text-ink-3 transition-transform ${showPast ? 'rotate-180' : ''}`} />
        </button>
      )}
      {showPast && past.map((i, k) => row(i, k, true))}
      {rest.map((i, k) => (
        <div key={i.key}>
          {k === (nowIdx === -1 ? rest.length : nowIdx) && <NowLine />}
          {row(i, k, false)}
        </div>
      ))}
      {nowIdx === -1 && <NowLine />}
    </div>
  );
}

function NowLine() {
  const t = hhmm(new Date());
  return (
    <div className="relative flex items-center gap-2 py-1" aria-label={`Now, ${t}`}>
      <span className="rounded-full bg-phys px-1.5 py-0.5 font-mono text-[11px] leading-none font-semibold text-white tnum" style={{ boxShadow: '0 0 12px rgb(255 90 78 / 0.6)' }}>
        {t}
      </span>
      <span className="relative h-px flex-1 bg-phys/70">
        <span className="absolute -top-[3px] left-0 size-[7px] rounded-full bg-phys" style={{ animation: 'breathe 2s ease-in-out infinite' }} />
      </span>
    </div>
  );
}
