// The useful first screen, ported from the old app's Today (js/today.js hero(), needsYou(), tray()):
//   Now card: the block you are in with a countdown ring, Next, and whether the day still fits.
//   Needs you: Nora's top open ask with its answer right here, and Work sessions waiting on him.
//   To place: today's tasks without a time, one tap from the Plan sheet.
import { AnimatePresence, motion } from 'motion/react';
import { useMemo, useState } from 'react';
import { Briefcase, ChevronRight, Moon } from 'lucide-react';
import { api } from '@/lib/api';
import { act, qc, useHeartbeats, useNoraThread, useProposals, useWorkThreads } from '@/lib/queries';
import { ago } from '@/lib/meta';
import type { Task } from '@/lib/types';
import { haptic, useUI } from '@/lib/ui';
import { Ring } from '@/ui/Ring';
import { spring } from '@/ui/motion';
import { floatXp } from '@/ui/XpFloat';
import { startCall } from '@/call/store';
import { BED, dur, hm, useDay } from './day';

/* ═══ Now ═══ */

export function NowCard() {
  const d = useDay();
  const { push, open } = useUI();
  if (!d.ready) return <div className="skeleton mx-gutter h-[220px] rounded-[20px]" />;

  const c = d.current;
  let kicker: React.ReactNode, title: string, meta: string, ringVal: number, ringHue: string, inner: [string, string], onTap: () => void;
  if (d.asleep) {
    const wake = d.now >= BED ? 24 * 60 + 5 * 60 : 5 * 60;
    kicker = (
      <>
        <Moon size={14} /> Past bedtime
      </>
    );
    title = 'Sleep';
    meta = `21:00–05:00 · phone docked`;
    ringVal = d.now >= BED ? (d.now - BED) / (8 * 60) : (d.now + 3 * 60) / (8 * 60);
    ringHue = '#9b8cff';
    inner = [dur(wake - d.now), 'to 05:00'];
    onTap = () => open({ kind: 'roadmap' });
  } else if (c) {
    const p = (d.now - c.start) / (c.end - c.start);
    kicker = (
      <>
        <span className="size-2 rounded-full" style={{ background: c.hue }} />
        <span className="truncate">Now{c.area ? ` · ${c.area}` : ''}</span>
      </>
    );
    title = c.title;
    meta = `${hm(c.start)}–${hm(c.end)} · ${dur(d.now - c.start)} in`;
    ringVal = p;
    ringHue = c.hue;
    inner = [dur(c.end - d.now), 'left'];
    onTap = () => (c.areaId ? push({ kind: 'area', id: c.areaId }) : undefined);
  } else {
    const end = d.next ? d.next.start : BED;
    kicker = <span>Free now</span>;
    title = d.next ? `Until ${d.next.title}` : 'Nothing else timed today';
    meta = `${hm(d.now)}–${hm(end)}`;
    ringVal = 0;
    ringHue = 'var(--color-calm)';
    inner = [dur(end - d.now), 'free'];
    onTap = () => open({ kind: 'plan' });
  }

  const n = d.unplaced.length;
  return (
    <motion.section layout transition={spring.layout} className="stone mx-gutter overflow-hidden rounded-[20px]" aria-label="Now">
      <button onClick={onTap} className="grid w-full grid-cols-[minmax(0,1fr)_auto] items-center gap-3 px-4 pt-4 pb-3 text-left active:bg-fill-1">
        <span className="flex min-w-0 flex-col">
          <span className="flex min-w-0 items-center gap-2 text-subhead font-semibold text-ink-2">{kicker}</span>
          <span className="mt-1 line-clamp-2 text-title2 font-bold text-balance">{title}</span>
          <span className="mt-0.5 text-subhead text-ink-2 tnum">{meta}</span>
        </span>
        <Ring size={88} stroke={8} value={ringVal} color={ringHue}>
          <span className="flex flex-col items-center leading-none">
            <b className="fig text-[19px] whitespace-nowrap">{inner[0]}</b>
            <small className="mt-1 text-caption font-semibold text-ink-2">{inner[1]}</small>
          </span>
        </Ring>
      </button>

      {d.next && !d.asleep && (
        <HRow label="Next" onClick={() => (d.next?.areaId ? push({ kind: 'area', id: d.next.areaId }) : undefined)}>
          <b className="font-semibold tnum">{hm(d.next.start)}</b>
          <span className="flex min-w-0 flex-1 items-center gap-2">
            <span className="size-[7px] shrink-0 rounded-full" style={{ background: d.next.hue }} />
            <span className="truncate">{d.next.title}</span>
          </span>
          <span className="shrink-0 text-footnote text-ink-3 tnum">in {dur(d.next.start - d.now)}</span>
          <ChevronRight size={16} className="shrink-0 text-ink-3" />
        </HRow>
      )}

      {!d.asleep && (
        <div className="relative flex min-h-12 items-center gap-2 py-1 pr-3 pl-4 text-subhead before:absolute before:top-0 before:right-0 before:left-4 before:h-px before:bg-line">
          <span className="w-[38px] shrink-0 self-start pt-2.5 text-footnote font-semibold text-ink-3">{d.evening ? 'Day' : 'Left'}</span>
          <span className="flex min-w-0 flex-1 flex-col py-2">
            {d.evening ? (
              <>
                <b className="font-semibold">{n ? `${n} task${n > 1 ? 's' : ''} left` : 'Nothing left'}</b>
                <small className="text-footnote text-ink-2">
                  {d.done} done today · sleep at 21:00
                </small>
              </>
            ) : n ? (
              <>
                <b className="font-semibold">
                  {n} task{n > 1 ? 's' : ''} without a time
                </b>
                <small className="text-footnote text-ink-2 tnum">
                  {dur(d.free)} free before 21:00
                </small>
              </>
            ) : (
              <>
                <b className="font-semibold">{d.total ? 'Everything has a time.' : 'Nothing planned yet.'}</b>
                <small className="text-footnote text-ink-2 tnum">{dur(d.free)} free before 21:00</small>
              </>
            )}
          </span>
          {d.evening ? (
            <button onClick={() => startCall('Close the day')} className="press h-9 shrink-0 rounded-full px-4 text-subhead font-semibold text-ground" style={{ background: 'var(--color-mind)' }}>
              Close the day
            </button>
          ) : (
            <button
              onClick={() => open(n ? { kind: 'plan' } : { kind: 'capture' })}
              className={`press h-9 shrink-0 rounded-full px-4 text-subhead font-semibold ${n ? 'text-ground' : 'bg-fill-2 text-ink'}`}
              style={n ? { background: 'var(--color-phys)', boxShadow: '0 4px 16px rgb(255 90 78 / 0.3)' } : undefined}
            >
              {n ? 'Plan' : 'Add'}
            </button>
          )}
        </div>
      )}
    </motion.section>
  );
}

function HRow({ label, children, onClick }: { label: string; children: React.ReactNode; onClick?: () => void }) {
  return (
    <button onClick={onClick} className="relative flex min-h-12 w-full items-center gap-2 px-4 py-1 text-left text-subhead before:absolute before:top-0 before:right-0 before:left-4 before:h-px before:bg-line active:bg-fill-1">
      <span className="w-[38px] shrink-0 text-footnote font-semibold text-ink-3">{label}</span>
      {children}
    </button>
  );
}

/* ═══ Needs you ═══ */

const DISMISS = 'aos.dismissed';
function dismissed(): Set<string> {
  try {
    return new Set(JSON.parse(localStorage.getItem(DISMISS) || '[]'));
  } catch {
    return new Set();
  }
}
function dismiss(key: string) {
  const s = dismissed();
  s.add(key);
  try {
    localStorage.setItem(DISMISS, JSON.stringify([...s].slice(-200)));
  } catch {
    /* fine */
  }
}

interface Ask {
  key: string;
  at: number;
  kicker: string;
  title: string;
  body?: string;
  acts: { label: string; primary?: boolean; run: () => Promise<unknown> | void; toast: string }[];
}

export function NeedsYou() {
  const { data: heartbeats } = useHeartbeats();
  const { data: proposals } = useProposals();
  const { data: thread } = useNoraThread();
  const { data: threads } = useWorkThreads();
  const { push, setTab, say } = useUI();
  const [gone, setGone] = useState<Set<string>>(dismissed);

  const asks = useMemo<Ask[]>(() => {
    const out: Ask[] = [];
    const dayStart = new Date().setHours(0, 0, 0, 0);
    for (const p of proposals || []) {
      const key = `p:${p.id}`;
      if (gone.has(key)) continue;
      const title = p.reason || p.quote || `${p.op || 'Change'} ${p.target_kind || ''}`.trim();
      out.push({
        key,
        at: typeof p.created_at === 'number' ? p.created_at : Date.parse(String(p.created_at || '')) || Date.now(),
        kicker: `Proposal · ${p.target_kind || 'mission'}`,
        title,
        body: p.quote && p.quote !== title ? `“${p.quote}”` : undefined,
        acts: [
          { label: 'Accept', primary: true, toast: 'Accepted', run: () => api.tool('mission_proposal_decide', { id: p.id, accept: true }) },
          { label: 'Not now', toast: 'Okay, not now', run: () => api.tool('mission_proposal_decide', { id: p.id, accept: false, reason: 'not now' }) },
        ],
      });
    }
    for (const b of heartbeats || []) {
      const key = `h:${b.id}`;
      if (b.at < dayStart || gone.has(key) || !b.text || b.decision === 'none') continue;
      out.push({
        key,
        at: b.at,
        kicker: b.decision === 'alarm' ? 'Reminder' : 'Check-in',
        title: b.text,
        acts: [
          { label: 'Yes, do it', primary: true, toast: 'Sent to Nora', run: () => api.post('/api/nora/messages', { text: `Yes, go ahead: "${b.text}"`, engine: 'auto' }, 120000) },
          { label: 'Not today', toast: 'Okay, not today', run: () => api.post('/api/nora/messages', { text: `Not today: "${b.text}"`, engine: 'auto' }, 120000) },
        ],
      });
    }
    // her latest word in the thread, when nothing above is open
    if (!out.length) {
      const last = [...(thread || [])].reverse().find((m) => m.kind === 'message' && m.engine !== 'user' && m.text.trim());
      if (last && Date.now() - last.at < 12 * 3600e3 && !gone.has(`m:${last.id}`)) {
        out.push({
          key: `m:${last.id}`,
          at: last.at,
          kicker: last.engine === 'grok' ? 'Grok' : 'Message',
          title: last.text.replace(/\s+/g, ' ').slice(0, 220),
          acts: [
            { label: 'Reply', primary: true, toast: '', run: () => push({ kind: 'nora' }) },
            { label: 'Got it', toast: 'Okay', run: () => undefined },
          ],
        });
      }
    }
    return out.sort((a, b) => b.at - a.at);
  }, [heartbeats, proposals, thread, gone, push]);

  const jobs = (threads || []).filter((t) => Number(t.pending_approvals) > 0 || t.status === 'waiting');
  const a = asks[0];
  if (!a && !jobs.length) return null;

  const answer = async (x: Ask['acts'][number]) => {
    haptic(x.primary ? 'success' : 'light');
    dismiss(a.key);
    setGone(dismissed());
    try {
      await x.run();
      if (x.toast) say(x.toast);
      void qc.invalidateQueries({ queryKey: ['proposals'] });
    } catch (e) {
      say((e as Error).message, 'warn');
    }
  };

  return (
    <section className="stone mx-gutter mt-3 overflow-hidden rounded-[20px]" aria-label="Needs you">
      <AnimatePresence initial={false}>
        {a && (
          <motion.div key={a.key} initial={{ height: 0, opacity: 0 }} animate={{ height: 'auto', opacity: 1 }} exit={{ height: 0, opacity: 0 }} transition={spring.layout} className="overflow-hidden">
            <div className="flex items-start gap-3 px-4 py-3.5">
              <img src="./img/nora-round.webp" alt="" className="mt-0.5 size-9 shrink-0 rounded-full object-cover" style={{ boxShadow: '0 0 0 2px var(--color-stone), 0 0 14px rgb(155 140 255 / 0.45)' }} />
              <div className="min-w-0 flex-1">
                <span className="flex min-h-5 items-center justify-between gap-2 text-footnote font-semibold text-ink-2">
                  <span className="truncate">
                    Nora · <span className="tnum">{ago(a.at)}</span> · {a.kicker}
                  </span>
                  {asks.length > 1 && (
                    <button onClick={() => push({ kind: 'nora' })} className="-my-3 inline-flex min-h-11 shrink-0 items-center gap-0.5 text-footnote font-semibold text-ink-2">
                      {asks.length - 1} more <ChevronRight size={13} />
                    </button>
                  )}
                </span>
                <p className="mt-0.5 line-clamp-3 text-headline font-semibold text-pretty">{a.title}</p>
                {a.body && <p className="mt-0.5 line-clamp-2 text-subhead text-ink-2">{a.body}</p>}
                <div className="mt-3 flex flex-wrap gap-2">
                  {a.acts.map((x) => (
                    <button
                      key={x.label}
                      onClick={() => void answer(x)}
                      className="press h-9 rounded-full px-4 text-subhead font-semibold"
                      style={x.primary ? { background: 'rgb(255 159 10 / 0.16)', color: '#ffb340' } : { background: 'var(--color-fill-2)' }}
                    >
                      {x.label}
                    </button>
                  ))}
                </div>
              </div>
            </div>
          </motion.div>
        )}
      </AnimatePresence>
      {jobs.length > 0 && (
        <button onClick={() => setTab('work')} className={`flex min-h-[60px] w-full items-center gap-3 px-4 text-left active:bg-fill-1 ${a ? 'hair-t' : ''}`}>
          <span className="relative grid size-9 shrink-0 place-items-center rounded-full bg-fill-2">
            <Briefcase size={17} />
            <em className="absolute -top-1.5 -right-1.5 grid h-[18px] min-w-[18px] place-items-center rounded-full bg-needs px-1 text-[11px] font-bold text-ground not-italic shadow-[0_0_0_2px_var(--color-stone)]">{jobs.length}</em>
          </span>
          <span className="min-w-0 flex-1 py-2.5">
            <span className="block text-headline font-semibold">
              {jobs.length} job{jobs.length > 1 ? 's' : ''} need{jobs.length === 1 ? 's' : ''} you
            </span>
            <span className="block truncate text-footnote text-ink-2">{jobs.map((j) => String(j.title || 'Session').split(':')[0]).join(', ')} · waiting on your answer</span>
          </span>
          <ChevronRight size={18} className="shrink-0 text-ink-3" />
        </button>
      )}
    </section>
  );
}

/* ═══ To place ═══ */

export function ToPlace() {
  const d = useDay();
  const open = useUI((s) => s.open);
  if (!d.unplaced.length) return null;
  return (
    <section className="mt-6" aria-label="To place">
      <div className="mb-2.5 flex items-baseline gap-2 px-gutter">
        <h2 className="text-title3 font-bold">To place</h2>
        <span className="text-subhead text-ink-2 tnum">{d.unplaced.length}</span>
        <span className="flex-1" />
        <button onClick={() => open({ kind: 'plan' })} className="press -my-2 inline-flex h-11 items-center gap-0.5 text-subhead font-medium text-ink-2">
          Plan <ChevronRight size={16} />
        </button>
      </div>
      <div className="no-scrollbar flex snap-x gap-2.5 overflow-x-auto px-gutter pb-1" style={{ scrollPaddingInline: 16 }}>
        {d.unplaced.map(({ task, area, hue }) => (
          <Pill key={task.id} task={task} area={area} hue={hue} onPlace={() => open({ kind: 'plan' })} />
        ))}
      </div>
    </section>
  );
}

function Pill({ task, area, hue, onPlace }: { task: Task; area: string | null; hue: string; onPlace: () => void }) {
  return (
    <div className="stone flex min-h-[80px] w-[212px] shrink-0 snap-start items-start gap-3 rounded-[14px] p-3">
      <button
        aria-label={`Done: ${task.title}`}
        disabled={task.pending}
        onClick={(e) => {
          const xp = act.tick(task);
          haptic('success');
          floatXp(e.currentTarget, xp);
        }}
        className="-m-2 grid size-11 shrink-0 place-items-center"
      >
        <span className="size-[22px] rounded-full shadow-[inset_0_0_0_1.75px_rgb(255_255_255/0.32)]" />
      </button>
      <button onClick={onPlace} className="flex min-w-0 flex-1 flex-col gap-1 self-stretch text-left">
        <span className="line-clamp-2 text-subhead font-semibold">{task.title}</span>
        <span className="flex items-center gap-1.5 text-footnote text-ink-2">
          <span className="size-[7px] rounded-full" style={{ background: hue }} />
          <span className="truncate">{area || 'No area'}</span>
        </span>
      </button>
    </div>
  );
}

/* ═══ the compact stat line under the hero (replaces the three big numbers) ═══ */

export function DayLine() {
  const d = useDay();
  if (!d.ready) return <div className="h-5" />;
  return (
    <p className="flex items-center justify-center gap-2 text-footnote text-ink-2 tnum">
      <span>{new Date().toLocaleDateString('en-GB', { weekday: 'short', day: 'numeric', month: 'short' })}</span>
      <span className="text-ink-3">·</span>
      <span>
        <b className="font-semibold text-ink">{d.done}</b> of {d.total} done
      </span>
    </p>
  );
}
