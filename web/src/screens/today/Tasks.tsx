// Tasks by pillar and area, with subtasks. Tap the circle to tick (XP floats up), hold a row to
// skip with a reason (repair, not blame: no red, no shake). Add from any area.
import { AnimatePresence, motion } from 'motion/react';
import { useRef, useState } from 'react';
import { Check, ChevronDown, Clock, Plus, ListTodo } from 'lucide-react';
import { act, useAreas, useToday } from '@/lib/queries';
import { PILLARS, PILLAR_ORDER, areaIcon, pillarOf, scheduledTime } from '@/lib/meta';
import type { AreaTasks, Task, TaskNode } from '@/lib/types';
import { haptic, useUI } from '@/lib/ui';
import { Empty, Section } from '@/ui/kit';
import { floatXp } from '@/ui/XpFloat';
import { spring } from '@/ui/motion';

export function TasksByArea({ filterPillar }: { filterPillar?: string } = {}) {
  const { data: today } = useToday();
  const { data: areas } = useAreas();
  const open = useUI((s) => s.open);

  if (!today) return <div className="skeleton mx-gutter mt-3 h-48 rounded-xl" />;

  const byPillar = PILLAR_ORDER.filter((p) => !filterPillar || p === filterPillar).map((pid) => ({
    pid,
    groups: today.areas.filter((a) => (a.pillar_id || 'none') === pid && a.tasks.length),
  }));
  const loose = filterPillar ? [] : today.areas.filter((a) => !a.pillar_id && a.tasks.length);
  const total = today.areas.reduce((n, a) => n + a.tasks.length, 0);

  if (total === 0)
    return (
      <div className="stone mx-gutter rounded-xl">
        <Empty icon={ListTodo} title="No tasks for today yet" body="Add the first one, or photograph your paper plan and let Nora sort it." action="Add a task" onAction={() => open({ kind: 'capture' })} />
      </div>
    );

  return (
    <div className="space-y-6">
      {byPillar.map(({ pid, groups }) => {
        if (!groups.length) return null;
        const p = PILLARS[pid];
        const all = groups.flatMap((g) => g.tasks);
        const done = all.filter((t) => t.status === 'done').length;
        const pillarName = areas?.pillars.find((x) => x.id === pid)?.name || p.label;
        return (
          <div key={pid}>
            {!filterPillar && (
              <div className="mb-2 flex items-center gap-2 px-gutter">
                <span className="size-2 rounded-full" style={{ background: p.hue, boxShadow: `0 0 10px ${p.glow}` }} />
                <span className="eyebrow" style={{ color: p.ink }}>
                  {pillarName}
                </span>
                <span className="text-footnote text-ink-3 tnum">
                  {done}/{all.length}
                </span>
              </div>
            )}
            <div className="space-y-3">
              {groups.map((g) => (
                <AreaCard key={g.area_id || 'none'} group={g} />
              ))}
            </div>
          </div>
        );
      })}
      {loose.map((g) => (
        <AreaCard key="loose" group={g} />
      ))}
    </div>
  );
}

export function AreaCard({ group }: { group: AreaTasks }) {
  const open = useUI((s) => s.open);
  const p = pillarOf(group.pillar_id);
  const Icon = areaIcon(group.area_id);
  const hue = p?.hue || 'var(--color-ink-2)';
  return (
    <div className="stone mx-gutter overflow-hidden rounded-xl">
      <div className="flex items-center gap-2.5 pt-1 pr-1 pl-4">
        <Icon size={16} strokeWidth={2.2} style={{ color: `color-mix(in oklab, ${hue} 80%, white)` }} />
        <span className="flex-1 text-subhead font-semibold text-ink-2">{group.area || 'No area'}</span>
        <button aria-label={`Add to ${group.area || 'tasks'}`} onClick={() => open({ kind: 'capture', area: group.area_id })} className="press grid size-11 place-items-center rounded-full text-ink-3 active:bg-fill-1">
          <Plus size={19} />
        </button>
      </div>
      <motion.ul layout transition={spring.layout} className="pb-1">
        {group.tasks.map((t) => (
          <TaskRow key={t.id} task={t} hue={hue} />
        ))}
      </motion.ul>
    </div>
  );
}

function TaskRow({ task, hue }: { task: TaskNode; hue: string }) {
  const [expanded, setExpanded] = useState(false);
  const open = useUI((s) => s.open);
  const hold = useRef<ReturnType<typeof setTimeout> | null>(null);
  const held = useRef(false);
  const subs = task.subtasks || [];
  const time = scheduledTime(task.scheduled);
  const closed = task.status === 'done' || task.status === 'skipped' || task.status === 'dropped';

  const startHold = () => {
    held.current = false;
    if (closed) return;
    hold.current = setTimeout(() => {
      held.current = true;
      haptic('medium');
      open({ kind: 'skip', task });
    }, 450);
  };
  const endHold = () => hold.current && clearTimeout(hold.current);

  return (
    <motion.li layout="position" transition={spring.layout} className="relative">
      <div
        className="flex min-h-[58px] items-start gap-3 px-4 py-2.5 select-none active:bg-fill-1"
        onPointerDown={startHold}
        onPointerUp={endHold}
        onPointerLeave={endHold}
        onPointerCancel={endHold}
        onContextMenu={(e) => e.preventDefault()}
        onClick={() => {
          if (held.current) return;
          if (subs.length) setExpanded((v) => !v);
        }}
      >
        <TickButton task={task} hue={hue} />
        <div className="hair min-w-0 flex-1 pb-2.5 -mb-2.5">
          <p className={`text-body ${task.status === 'done' ? 'text-ink-3 line-through decoration-ink-3/70' : task.status === 'skipped' ? 'text-ink-3' : ''}`}>{task.title}</p>
          <p className="mt-0.5 flex flex-wrap items-center gap-x-2 text-footnote text-ink-2">
            {time && (
              <span className="inline-flex items-center gap-1 tnum">
                <Clock size={12} />
                {time}
              </span>
            )}
            {task.rollup && (
              <span className="tnum">
                {task.rollup.done}/{task.rollup.total} steps
              </span>
            )}
            {task.status === 'skipped' && <span>Skipped{task.skip_reason ? ` · ${task.skip_reason}` : ''}</span>}
            {task.pending && <span className="text-ink-3">Saving…</span>}
            {!time && !task.rollup && task.status !== 'skipped' && !task.pending && <span className="text-ink-3">+{10 * (task.xp_weight || 1)} XP</span>}
          </p>
        </div>
        {subs.length > 0 && <ChevronDown size={18} className={`mt-1 shrink-0 text-ink-3 transition-transform ${expanded ? 'rotate-180' : ''}`} />}
      </div>
      <AnimatePresence initial={false}>
        {expanded && subs.length > 0 && (
          <motion.ul initial={{ height: 0, opacity: 0 }} animate={{ height: 'auto', opacity: 1 }} exit={{ height: 0, opacity: 0 }} transition={spring.layout} className="overflow-hidden">
            {subs.map((s) => (
              <li key={s.id} className="flex min-h-[46px] items-center gap-3 pr-4 pl-[52px]">
                <TickButton task={s} hue={hue} small />
                <span className={`hair flex-1 py-3 text-callout ${s.status === 'done' ? 'text-ink-3 line-through' : ''}`}>{s.title}</span>
              </li>
            ))}
            <li>
              <button onClick={() => open({ kind: 'capture', area: task.area_id, parent: task.id })} className="flex h-11 w-full items-center gap-3 pl-[56px] text-left text-footnote text-ink-3">
                <Plus size={15} /> Add a step
              </button>
            </li>
          </motion.ul>
        )}
      </AnimatePresence>
    </motion.li>
  );
}

/** The check: fills from the centre in the pillar's colour (Aura "Charge"). */
export function TickButton({ task, hue, small }: { task: Task; hue: string; small?: boolean }) {
  const ref = useRef<HTMLButtonElement>(null);
  const done = task.status === 'done';
  const skipped = task.status === 'skipped';
  const s = small ? 22 : 26;
  return (
    <button
      ref={ref}
      aria-label={done ? `Reopen ${task.title}` : `Done: ${task.title}`}
      disabled={task.pending || skipped}
      onClick={(e) => {
        e.stopPropagation();
        const xp = act.tick(task);
        haptic(xp ? 'success' : 'light');
        floatXp(ref.current, xp, `color-mix(in oklab, ${hue} 70%, white)`);
      }}
      onPointerDown={(e) => e.stopPropagation()}
      className="-m-[9px] grid shrink-0 place-items-center p-[9px] disabled:opacity-40"
      style={{ width: s + 18, height: s + 18 }}
    >
      <span className="relative grid place-items-center rounded-full" style={{ width: s, height: s, boxShadow: `inset 0 0 0 1.75px ${done ? 'transparent' : skipped ? 'rgb(255 255 255 / 0.18)' : 'rgb(255 255 255 / 0.32)'}` }}>
        <AnimatePresence>
          {done && (
            <motion.span
              className="absolute inset-0 grid place-items-center rounded-full"
              style={{ background: hue, boxShadow: `0 0 14px color-mix(in oklab, ${hue} 70%, transparent)` }}
              initial={{ scale: 0.2, opacity: 0 }}
              animate={{ scale: 1, opacity: 1 }}
              exit={{ scale: 0.2, opacity: 0 }}
              transition={spring.pop}
            >
              <Check size={small ? 13 : 15} strokeWidth={3.2} className="text-ground" />
            </motion.span>
          )}
        </AnimatePresence>
        {skipped && <span className="h-[1.75px] w-2.5 rounded bg-ink-3" />}
      </span>
    </button>
  );
}

export function TasksSection() {
  const { data: today } = useToday();
  const all = today?.areas.flatMap((a) => a.tasks) ?? [];
  const done = all.filter((t) => t.status === 'done').length;
  const open = useUI((s) => s.open);
  return (
    <Section title="Tasks" meta={all.length ? `${done} of ${all.length}` : undefined} action="Add" onAction={() => open({ kind: 'capture' })}>
      <TasksByArea />
    </Section>
  );
}
