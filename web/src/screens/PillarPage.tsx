// Pillar page: its level, the three areas with XP and today's tasks, the habits that feed it.
import { motion } from 'motion/react';
import { ChevronRight } from 'lucide-react';
import { useAreas, useToday, useXp } from '@/lib/queries';
import { PILLARS, areaIcon } from '@/lib/meta';
import type { PillarId } from '@/lib/types';
import { useUI } from '@/lib/ui';
import { Bar, Section } from '@/ui/kit';
import { Page } from '@/ui/Page';
import { Ring } from '@/ui/Ring';
import { rise } from '@/ui/motion';
import { TasksByArea } from './today/Tasks';

export function PillarPage({ id }: { id: PillarId }) {
  const { data: areas } = useAreas();
  const { data: xp } = useXp();
  const { data: today } = useToday();
  const { pop, push } = useUI();
  const p = PILLARS[id];
  const pillar = areas?.pillars.find((x) => x.id === id);
  const px = xp?.pillars.find((x) => x.id === id);
  const lvlXp = px ? px.xp - 50 * px.level * (px.level - 1) : 0;
  const size = px ? 100 * px.level : 100;
  const tasks = today?.areas.filter((a) => a.pillar_id === id).flatMap((a) => a.tasks) ?? [];
  const done = tasks.filter((t) => t.status === 'done').length;

  return (
    <Page onBack={pop} title={pillar?.name || p.label} tone={p.glow}>
      <div className="px-gutter pt-[calc(var(--safe-t)+68px)]">
        <div className="flex items-center gap-5">
          <Ring size={96} stroke={7} value={lvlXp / size} color={p.hue}>
            <div className="text-center">
              <p className="text-caption text-ink-2">Level</p>
              <p className="fig text-[30px] leading-[32px]">{px?.level ?? 1}</p>
            </div>
          </Ring>
          <div className="min-w-0">
            <p className="eyebrow" style={{ color: p.ink }}>
              {p.label}
            </p>
            <h1 className="text-display font-bold">{pillar?.name || p.label}</h1>
            <p className="text-subhead text-ink-2 tnum">
              {(px?.xp ?? 0).toLocaleString('en-IN')} XP · {done}/{tasks.length} today
            </p>
          </div>
        </div>
      </div>

      <Section title="Areas">
        <div className="stone mx-gutter rounded-xl">
          {(pillar?.areas || []).map((a, i, arr) => {
            const Icon = areaIcon(a.id);
            const ax = xp?.areas.find((x) => x.id === a.id);
            const open = today?.areas.find((g) => g.area_id === a.id)?.tasks.filter((t) => t.status === 'open').length ?? 0;
            return (
              <motion.button key={a.id} variants={rise} initial="hidden" animate="show" custom={i} onClick={() => push({ kind: 'area', id: a.id })} className="flex w-full items-center gap-3 px-4 text-left active:bg-fill-1">
                <span className="grid size-10 shrink-0 place-items-center rounded-[12px]" style={{ background: `color-mix(in oklab, ${p.hue} 18%, transparent)`, color: p.ink }}>
                  <Icon size={19} strokeWidth={2.1} />
                </span>
                <span className={`flex min-w-0 flex-1 items-center gap-3 py-3.5 ${i < arr.length - 1 ? 'hair' : ''}`}>
                  <span className="min-w-0 flex-1">
                    <span className="block truncate text-body">{a.name}</span>
                    <span className="mt-1.5 flex items-center gap-2">
                      <Bar className="flex-1" value={ax ? (ax.xp % 150) / 150 : 0} color={p.hue} height={3} />
                      <span className="text-caption text-ink-2 tnum">Lv {ax?.level ?? 1}</span>
                    </span>
                  </span>
                  <span className="text-footnote text-ink-2 tnum">{open ? `${open} open` : ''}</span>
                  <ChevronRight size={18} className="text-ink-3" />
                </span>
              </motion.button>
            );
          })}
        </div>
      </Section>

      <Section title="Today" meta={tasks.length ? `${done} of ${tasks.length}` : undefined}>
        <TasksByArea filterPillar={id} />
      </Section>
    </Page>
  );
}
