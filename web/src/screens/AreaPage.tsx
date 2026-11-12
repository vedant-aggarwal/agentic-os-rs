// Area page: everything open in one area (task_list), its habits, and the pillar's goal.
import { useQuery } from '@tanstack/react-query';
import { ChevronRight, ListTodo } from 'lucide-react';
import { api } from '@/lib/api';
import { useAreas, useProfile, useToday, useXp } from '@/lib/queries';
import { PILLARS, areaIcon, dueLabel, rupees } from '@/lib/meta';
import type { TaskNode } from '@/lib/types';
import { useUI } from '@/lib/ui';
import { Empty, Section } from '@/ui/kit';
import { Page } from '@/ui/Page';
import { AreaCard } from './today/Tasks';

export function AreaPage({ id }: { id: string }) {
  const { data: areas } = useAreas();
  const { data: today } = useToday();
  const { data: xp } = useXp();
  const { data: profile } = useProfile();
  const { pop, push, open } = useUI();
  const area = areas?.pillars.flatMap((p) => p.areas).find((a) => a.id === id);
  const p = area ? PILLARS[area.pillar_id] : PILLARS.mind;
  const Icon = areaIcon(id);
  const ax = xp?.areas.find((x) => x.id === id);
  const todayGroup = today?.areas.find((a) => a.area_id === id);
  const all = useQuery({
    queryKey: ['area-tasks', id],
    queryFn: async () => (await api.tool<{ tasks: TaskNode[] }>('task_list', { area: id, status: 'open', limit: 80 })).tasks || [],
  });
  const later = (all.data || []).filter((t) => !todayGroup?.tasks.some((x) => x.id === t.id));
  const habits = (today?.habits || []).filter((h) => h.area_id === id);
  const goal = profile?.hero.find((h) => h.slot === p.slot);

  return (
    <Page onBack={pop} title={area?.name} tone={p.glow}>
      <div className="px-gutter pt-[calc(var(--safe-t)+68px)]">
        <span className="grid size-14 place-items-center rounded-[18px]" style={{ background: `color-mix(in oklab, ${p.hue} 20%, transparent)`, color: p.ink }}>
          <Icon size={26} strokeWidth={2} />
        </span>
        <h1 className="mt-4 text-display font-bold">{area?.name || 'Area'}</h1>
        <p className="mt-1 text-subhead text-ink-2 tnum">
          Level {ax?.level ?? 1} · {(ax?.xp ?? 0).toLocaleString('en-IN')} XP
        </p>
      </div>

      <Section title="Today">
        {todayGroup?.tasks.length ? (
          <AreaCard group={todayGroup} />
        ) : (
          <div className="stone mx-gutter rounded-xl">
            <Empty icon={ListTodo} title="Nothing here today" action="Add a task" onAction={() => open({ kind: 'capture', area: id })} />
          </div>
        )}
      </Section>

      {later.length > 0 && (
        <Section title="Open" meta={later.length}>
          <div className="stone mx-gutter rounded-xl">
            {later.map((t, i) => (
              <div key={t.id} className={`mx-4 py-3.5 ${i < later.length - 1 ? 'hair' : ''}`}>
                <p className="text-body">{t.title}</p>
                {t.due && <p className="mt-0.5 text-footnote text-ink-2">{dueLabel(t.due)}</p>}
              </div>
            ))}
          </div>
        </Section>
      )}

      {habits.length > 0 && (
        <Section title="Habits">
          <div className="stone mx-gutter rounded-xl">
            {habits.map((h, i) => (
              <div key={h.id} className={`mx-4 flex items-center py-3.5 ${i < habits.length - 1 ? 'hair' : ''}`}>
                <span className="flex-1 text-body">{h.name}</span>
                <span className="text-subhead text-ink-2 tnum">{h.streak.current} days</span>
              </div>
            ))}
          </div>
        </Section>
      )}

      {goal && (
        <Section title="Goal">
          <button onClick={() => push({ kind: 'goal', slot: goal.slot })} className="stone press mx-gutter flex w-[calc(100%-32px)] items-center gap-3 rounded-xl p-4 text-left">
            <span className="min-w-0 flex-1">
              <span className="block truncate text-headline font-semibold">{goal.name}</span>
              <span className="text-footnote text-ink-2 tnum">{goal.price ? `${rupees(goal.earned)} of ${rupees(goal.price)}` : goal.variant}</span>
            </span>
            <ChevronRight size={18} className="text-ink-3" />
          </button>
        </Section>
      )}
    </Page>
  );
}
