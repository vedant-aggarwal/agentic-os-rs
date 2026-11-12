// Inbox, Life, Work and Mission: real data, simple screens.
import { motion } from 'motion/react';
import { useMemo, useState } from 'react';
import { BookHeart, ChevronRight, CircleDot, FolderGit2, Inbox as InboxIcon, Mail, MessageCircle, Smartphone, Target, Telescope, TrendingUp, Zap } from 'lucide-react';
import { useAreas, useInbox, useJournal, useLooseTasks, useMission, useProfile, useToday, useWorkProjects, useWorkThreads, useXp } from '@/lib/queries';
import { PILLARS, PILLAR_ORDER, ago, areaIcon, dueLabel } from '@/lib/meta';
import type { InboxItem } from '@/lib/types';
import { useUI } from '@/lib/ui';
import { Bar, Empty, Section, Segmented } from '@/ui/kit';
import { Ring } from '@/ui/Ring';
import { rise } from '@/ui/motion';
import { decode } from './Sheets';

function LargeTitle({ title, sub, right }: { title: string; sub?: React.ReactNode; right?: React.ReactNode }) {
  return (
    <div className="flex items-end gap-3 px-gutter pt-[calc(var(--safe-t)+20px)]">
      <div className="min-w-0 flex-1">
        <h1 className="text-display font-bold">{title}</h1>
        {sub && <p className="mt-1 text-subhead text-ink-2">{sub}</p>}
      </div>
      {right}
    </div>
  );
}

/* ═══ Inbox ═══ */
type Seg = 'all' | 'tasks' | 'messages' | 'mail';
const SOURCE_ICON = { whatsapp: MessageCircle, gmail: Mail, app: Smartphone } as Record<string, typeof Mail>;

export function InboxScreen() {
  const { data: items, isLoading, error } = useInbox();
  const { data: loose } = useLooseTasks();
  const [seg, setSeg] = useState<Seg>('all');
  const open = useUI((s) => s.open);
  const live = (items || []).filter((i) => !i.handled);
  const tasks = (loose || []).filter((t) => !t.scheduled && !t.parent_id);
  const msgs = live.filter((i) => i.source !== 'gmail');
  const mail = live.filter((i) => i.source === 'gmail');
  const yes = (list: InboxItem[]) => list.filter((i) => i.triage_yes !== false);
  const no = (list: InboxItem[]) => list.filter((i) => i.triage_yes === false);
  const shown = seg === 'messages' ? msgs : seg === 'mail' ? mail : seg === 'tasks' ? [] : live;

  return (
    <div className="pb-8">
      <LargeTitle title="Inbox" sub={`${live.length} to read · ${tasks.length} tasks without a time`} />
      <div className="px-gutter pt-5">
        <Segmented<Seg>
          value={seg}
          onChange={setSeg}
          options={[
            { value: 'all', label: 'All' },
            { value: 'tasks', label: 'Tasks', count: tasks.length },
            { value: 'messages', label: 'Chats', count: msgs.length },
            { value: 'mail', label: 'Mail', count: mail.length },
          ]}
        />
      </div>

      {(seg === 'all' || seg === 'tasks') && tasks.length > 0 && (
        <Section title="Tasks" meta={tasks.length} className="mt-6">
          <div className="stone mx-gutter rounded-xl">
            {tasks.slice(0, seg === 'all' ? 5 : 60).map((t, i, arr) => {
              const Icon = areaIcon(t.area_id);
              return (
                <div key={t.id} className="flex items-center gap-3 px-4">
                  <Icon size={17} className="shrink-0 text-ink-2" />
                  <div className={`flex-1 py-3.5 ${i < arr.length - 1 ? 'hair' : ''}`}>
                    <p className="text-body">{t.title}</p>
                    {t.due && <p className="text-footnote text-ink-2">{dueLabel(t.due)}</p>}
                  </div>
                </div>
              );
            })}
          </div>
        </Section>
      )}

      {seg !== 'tasks' && (
        <>
          {isLoading && !items && <div className="skeleton mx-gutter mt-6 h-64 rounded-xl" />}
          {error && !items && <p className="mx-gutter mt-6 text-subhead text-ink-2">The inbox didn’t load: {(error as Error).message}</p>}
          {items && !shown.length && (
            <div className="stone mx-gutter mt-6 rounded-xl">
              <Empty icon={InboxIcon} title="All clear" body="New WhatsApp and mail land here, sorted by what Nora thinks matters." />
            </div>
          )}
          {yes(shown).length > 0 && (
            <Section title="For you" meta={yes(shown).length} className="mt-6">
              <InboxList list={yes(shown)} onOpen={(item) => open({ kind: 'inbox', item })} />
            </Section>
          )}
          {no(shown).length > 0 && (
            <Section title="Morning brief" meta={no(shown).length}>
              <p className="-mt-1 mb-3 px-gutter text-footnote text-ink-3">Low priority. Nora folds these into tomorrow’s print.</p>
              <InboxList list={no(shown)} onOpen={(item) => open({ kind: 'inbox', item })} dim />
            </Section>
          )}
        </>
      )}
    </div>
  );
}

function InboxList({ list, onOpen, dim }: { list: InboxItem[]; onOpen: (i: InboxItem) => void; dim?: boolean }) {
  return (
    <div className={`stone mx-gutter overflow-hidden rounded-xl ${dim ? 'opacity-75' : ''}`}>
      {list.map((it, i) => {
        const Icon = SOURCE_ICON[it.source] || Smartphone;
        const who = it.sender_name || it.chat_name || it.sender || 'Unknown';
        return (
          <motion.button key={it.id} variants={rise} initial="hidden" animate="show" custom={i} onClick={() => onOpen(it)} className="flex w-full items-start gap-3 px-4 text-left active:bg-fill-1">
            <span className={`mt-3.5 grid size-9 shrink-0 place-items-center rounded-full ${it.source === 'whatsapp' ? 'bg-[#34d27b]/15 text-[#5fe39a]' : it.source === 'gmail' ? 'bg-money/15 text-money-ink' : 'bg-fill-2 text-ink-2'}`}>
              <Icon size={17} strokeWidth={2.1} />
            </span>
            <span className={`min-w-0 flex-1 py-3 ${i < list.length - 1 ? 'hair' : ''}`}>
              <span className="flex items-baseline gap-2">
                <span className="min-w-0 flex-1 truncate text-headline font-semibold">{who}</span>
                <span className="shrink-0 text-footnote text-ink-3 tnum">{ago(it.at)}</span>
              </span>
              {it.subject && <span className="block truncate text-subhead">{decode(it.subject)}</span>}
              <span className="line-clamp-2 text-subhead text-ink-2">{decode(it.text)}</span>
            </span>
          </motion.button>
        );
      })}
    </div>
  );
}

/* ═══ Life ═══ */
export function LifeScreen() {
  const { data: areas } = useAreas();
  const { data: xp } = useXp();
  const { data: today } = useToday();
  const push = useUI((s) => s.push);
  return (
    <div className="pb-8">
      <LargeTitle title="Life" sub="Three pillars, nine areas. Be the strongest, the richest, the smartest you can become." />
      <div className="mt-6 space-y-4">
        {PILLAR_ORDER.map((pid, k) => {
          const p = PILLARS[pid];
          const pillar = areas?.pillars.find((x) => x.id === pid);
          const px = xp?.pillars.find((x) => x.id === pid);
          const tasks = today?.areas.filter((a) => a.pillar_id === pid).flatMap((a) => a.tasks) ?? [];
          const done = tasks.filter((t) => t.status === 'done').length;
          const into = px ? (px.xp - 50 * px.level * (px.level - 1)) / (100 * px.level) : 0;
          return (
            <motion.div key={pid} variants={rise} initial="hidden" animate="show" custom={k} className="mx-gutter overflow-hidden rounded-xl" style={{ background: `radial-gradient(90% 120% at 0% 0%, color-mix(in oklab, ${p.glow} 16%, transparent), transparent 60%), var(--tex-stone) 0 0 / 128px 128px, var(--color-stone)`, boxShadow: 'var(--stone-edge)' }}>
              <button onClick={() => push({ kind: 'pillar', id: pid })} className="flex w-full items-center gap-4 p-4 text-left">
                <Ring size={58} stroke={5} value={into} color={p.hue}>
                  <span className="fig text-[20px]">{px?.level ?? 1}</span>
                </Ring>
                <span className="min-w-0 flex-1">
                  <span className="eyebrow" style={{ color: p.ink }}>
                    {p.label}
                  </span>
                  <span className="block text-title3 font-bold">{pillar?.name || p.label}</span>
                  <span className="text-footnote text-ink-2 tnum">
                    {(px?.xp ?? 0).toLocaleString('en-IN')} XP · {done}/{tasks.length} today
                  </span>
                </span>
                <ChevronRight size={18} className="text-ink-3" />
              </button>
              <div className="hair-t">
                {(pillar?.areas || []).map((a, i, arr) => {
                  const Icon = areaIcon(a.id);
                  const ax = xp?.areas.find((x) => x.id === a.id);
                  return (
                    <button key={a.id} onClick={() => push({ kind: 'area', id: a.id })} className="flex w-full items-center gap-3 px-4 text-left active:bg-fill-1">
                      <Icon size={17} style={{ color: p.ink }} />
                      <span className={`flex flex-1 items-center gap-3 py-3 ${i < arr.length - 1 ? 'hair' : ''}`}>
                        <span className="flex-1 text-callout">{a.name}</span>
                        <span className="text-footnote text-ink-2 tnum">Lv {ax?.level ?? 1}</span>
                        <ChevronRight size={16} className="text-ink-3" />
                      </span>
                    </button>
                  );
                })}
              </div>
            </motion.div>
          );
        })}
      </div>
    </div>
  );
}

/* ═══ Work ═══ */
export function WorkScreen() {
  const { data: projects, error } = useWorkProjects();
  const { data: threads } = useWorkThreads();
  const recent = useMemo(() => [...(threads || [])].sort((a, b) => Number(b.updated_at || 0) - Number(a.updated_at || 0)).slice(0, 8), [threads]);
  return (
    <div className="pb-8">
      <LargeTitle title="Work" sub="Agent sessions on the PC: Claude and Codex, by project." />
      {error && !projects && <p className="mx-gutter mt-6 text-subhead text-ink-2">Work didn’t load: {(error as Error).message}</p>}
      <Section title="Sessions" meta={recent.length || undefined}>
        {recent.length ? (
          <div className="stone mx-gutter rounded-xl">
            {recent.map((t, i) => (
              <div key={t.id} className={`mx-4 flex items-center gap-3 py-3.5 ${i < recent.length - 1 ? 'hair' : ''}`}>
                <span className={`size-2.5 shrink-0 rounded-full ${t.status === 'running' ? 'bg-[#34d27b]' : t.status === 'waiting' ? 'bg-needs' : 'bg-ink-3'}`} style={t.status === 'running' ? { boxShadow: '0 0 8px #34d27b' } : undefined} />
                <span className="min-w-0 flex-1">
                  <span className="block truncate text-body">{String(t.title || 'Untitled session')}</span>
                  <span className="text-footnote text-ink-2">
                    {String(t.provider || '')} {t.updated_at ? `· ${ago(Number(t.updated_at))}` : ''}
                  </span>
                </span>
                <span className="text-footnote text-ink-2 capitalize">{String(t.status || '')}</span>
              </div>
            ))}
          </div>
        ) : (
          <div className="stone mx-gutter rounded-xl">
            <Empty icon={Zap} title="No sessions running" body="Launch one from a project on the PC, or ask Nora by voice." />
          </div>
        )}
      </Section>
      <Section title="Projects" meta={projects?.length || undefined}>
        <div className="stone mx-gutter rounded-xl">
          {(projects || []).map((p, i, arr) => (
            <div key={p.id} className="flex items-center gap-3 px-4">
              <FolderGit2 size={18} className="shrink-0 text-money-ink" />
              <span className={`flex-1 truncate py-3.5 text-body ${i < arr.length - 1 ? 'hair' : ''}`}>{p.name}</span>
            </div>
          ))}
          {!projects?.length && <p className="p-4 text-subhead text-ink-2">No projects yet.</p>}
        </div>
      </Section>
    </div>
  );
}

/* ═══ Mission ═══ */
export function MissionScreen() {
  const { data: m } = useMission();
  const { data: xp } = useXp();
  const { data: today } = useToday();
  const { data: journal } = useJournal();
  const safe = useUI((s) => s.safe);
  const { data: profile } = useProfile();
  const hidden = new Set(safe ? profile?.private_habits || [] : []);
  const streaks = (today?.habits || []).filter((h) => h.streak.current > 0).sort((a, b) => b.streak.current - a.streak.current);
  const cards = [
    { icon: Telescope, title: 'Visualization', sub: m ? `${m.boards} boards · ${m.dreams} dreams` : '…', tint: '#9b8cff' },
    { icon: Target, title: 'Version I want to be', sub: m ? `${m.statements} statements · ${m.goals_open} goals open` : '…', tint: '#3fd0ff' },
    { icon: CircleDot, title: 'New Me', sub: m ? `${m.rules} rules · ${m.drops_open} drops to sort` : '…', tint: '#ff8a80' },
  ];
  return (
    <div className="pb-8">
      <LargeTitle title="Mission" sub="Destination first. Where you’re going and who you’re becoming." />
      <div className="no-scrollbar mt-6 flex snap-x snap-mandatory gap-3 overflow-x-auto px-gutter" style={{ scrollPaddingInline: 16 }}>
        {cards.map(({ icon: Icon, title, sub, tint }) => (
          <div key={title} className="stone relative flex h-[168px] w-[72%] max-w-[300px] shrink-0 snap-start flex-col overflow-hidden rounded-xl p-4">
            <div aria-hidden className="absolute inset-0" style={{ background: `radial-gradient(90% 90% at 100% 0%, ${tint}33, transparent 70%)` }} />
            <Icon size={22} style={{ color: tint }} className="relative" />
            <span className="relative mt-auto text-title3 font-bold">{title}</span>
            <span className="relative mt-0.5 text-footnote text-ink-2">{sub}</span>
          </div>
        ))}
      </div>

      <Section title="Growth">
        <div className="stone mx-gutter grid grid-cols-3 rounded-xl py-4">
          <div className="text-center">
            <p className="eyebrow">This week</p>
            <p className="fig mt-1 text-[26px] text-xp-ink">{xp?.week ?? 0}</p>
            <p className="text-caption text-ink-3">XP</p>
          </div>
          <div className="text-center">
            <p className="eyebrow">Today</p>
            <p className="fig mt-1 text-[26px] text-money-ink">{Math.round((today?.score.rate ?? 0) * 100)}%</p>
            <p className="text-caption text-ink-3">done</p>
          </div>
          <div className="text-center">
            <p className="eyebrow">Rules kept</p>
            <p className="fig mt-1 text-[26px] text-mind-ink">{m?.rules_kept_rate != null ? `${Math.round(m.rules_kept_rate * 100)}%` : '–'}</p>
            <p className="text-caption text-ink-3">7 days</p>
          </div>
        </div>
        {streaks.length > 0 && (
          <div className="stone mx-gutter mt-3 rounded-xl p-4">
            <p className="mb-3 flex items-center gap-2 text-subhead font-semibold">
              <TrendingUp size={16} className="text-xp" /> Streaks
            </p>
            {streaks.filter((h) => !hidden.has(h.id)).slice(0, 5).map((h) => (
              <div key={h.id} className="mb-2.5 flex items-center gap-3 last:mb-0">
                <span className="w-28 truncate text-subhead">{h.name}</span>
                <Bar className="flex-1" value={Math.min(1, h.streak.current / Math.max(h.streak.best, 21))} color="var(--color-xp)" height={5} />
                <span className="w-8 text-right text-footnote text-ink-2 tnum">{h.streak.current}</span>
              </div>
            ))}
          </div>
        )}
      </Section>

      <Section title="Journal">
        {journal?.length ? (
          <div className="space-y-3">
            {journal.slice(0, 5).map((j) => (
              <div key={j.id} className="stone mx-gutter rounded-xl p-4">
                <p className="text-footnote text-ink-2">{new Date(j.date).toLocaleDateString('en-GB', { weekday: 'long', day: 'numeric', month: 'short' })}</p>
                <p className="mt-1.5 text-callout leading-[23px]">{j.text}</p>
              </div>
            ))}
          </div>
        ) : (
          <div className="stone mx-gutter rounded-xl">
            <Empty icon={BookHeart} title="No entries yet" body="Tonight’s close-the-day call writes the first one, in your words." />
          </div>
        )}
      </Section>
    </div>
  );
}
