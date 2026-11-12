// Every sheet, rendered by kind from the UI store.
import { AnimatePresence, motion } from 'motion/react';
import { useEffect, useMemo, useRef, useState } from 'react';
import { BED, dur, hm, useDay } from './today/day';
import { Check, Lock, LogOut, Printer, Reply, Server, ShieldCheck, Sparkles } from 'lucide-react';
import { api } from '@/lib/api';
import { act, useAreas, useProfile, useToday } from '@/lib/queries';
import { MILESTONES, PILLARS, SKIP_REASONS, lookFor, xpFor, ago } from '@/lib/meta';
import { useOutbox } from '@/lib/outbox';
import { isDemo, useSession } from '@/lib/session';
import { haptic, useUI, type Sheet as SheetT } from '@/lib/ui';
import { Button, Chip } from '@/ui/kit';
import { Sheet } from '@/ui/Sheet';
import { Ring } from '@/ui/Ring';
import { MoodSheet } from './today/Mood';
import { T3ThreadSheet, WorkNewSheet, WorkThreadSheet } from './WorkSheets';

export function Sheets() {
  const sheet = useUI((s) => s.sheet);
  return <AnimatePresence>{sheet && <SheetFor key={sheet.kind} sheet={sheet} />}</AnimatePresence>;
}

function SheetFor({ sheet }: { sheet: SheetT }) {
  switch (sheet.kind) {
    case 'mood':
      return <MoodSheet />;
    case 'roadmap':
      return <Roadmap />;
    case 'profile':
      return <ProfileSheet />;
    case 'capture':
      return <Capture area={sheet.area ?? null} parent={sheet.parent} />;
    case 'skip':
      return <Skip />;
    case 'wave':
      return <Wave />;
    case 'print':
      return <Print />;
    case 'inbox':
      return <InboxItemSheet />;
    case 'plan':
      return <Plan />;
    case 'work-thread':
      return <WorkThreadSheet key={sheet.id} id={sheet.id} />;
    case 't3-thread':
      return <T3ThreadSheet id={sheet.id} />;
    case 'work-new':
      return <WorkNewSheet project={sheet.project} />;
  }
}

/* ═══ level roadmap ═══ */
function Roadmap() {
  const close = useUI((s) => s.close);
  const safe = useUI((s) => s.safe);
  const { data: today } = useToday();
  const lv = today?.xp.level;
  const L = lv?.level ?? 1;
  const next = MILESTONES.find((m) => m.level > L);
  const here = useRef<HTMLLIElement>(null);
  useEffect(() => {
    setTimeout(() => here.current?.scrollIntoView({ block: 'center', behavior: 'smooth' }), 350);
  }, []);

  return (
    <Sheet onClose={close} size="full" title="Level roadmap" subtitle={next ? `${(xpFor(next.level) - (lv?.total_xp ?? 0)).toLocaleString('en-IN')} XP to ${next.title.toLowerCase()} (level ${next.level})` : 'Past the last milestone'} tone="#ffc24b">
      <div className="mt-2 flex items-center gap-4 rounded-[22px] bg-fill-1 p-4">
        <Ring size={72} stroke={6} value={lv?.progress ?? 0} color="var(--color-xp)" color2="#ff9d3a">
          <span className="fig text-[26px]">{L}</span>
        </Ring>
        <div className="min-w-0 flex-1">
          <p className="text-headline font-semibold">Level {L}</p>
          <p className="text-subhead text-ink-2 tnum">
            {(lv?.into_level ?? 0).toLocaleString('en-IN')} / {(lv?.level_size ?? 100).toLocaleString('en-IN')} XP · {(lv?.to_next ?? 100).toLocaleString('en-IN')} to go
          </p>
          <p className="mt-0.5 text-footnote text-ink-3">Stage {lv?.stage ?? 1} of 20 · +{today?.xp.today ?? 0} XP today</p>
        </div>
      </div>

      <ol className="relative mt-6 pb-4">
        <span aria-hidden className="absolute top-4 bottom-4 left-[27px] w-[2px] rounded bg-fill-2" />
        <motion.span aria-hidden className="absolute top-4 left-[27px] w-[2px] rounded" style={{ background: 'linear-gradient(var(--color-xp), #ff9d3a)', boxShadow: '0 0 10px rgb(255 194 75 / 0.6)' }} initial={{ height: 0 }} animate={{ height: `${Math.min(100, (MILESTONES.filter((m) => m.level <= L).length / MILESTONES.length) * 100)}%` }} transition={{ duration: 0.9, ease: [0.22, 1, 0.36, 1] }} />
        {MILESTONES.map((m, i) => {
          const reached = m.level <= L;
          const isNext = m === next;
          return (
            <motion.li key={m.level} ref={isNext ? here : undefined} initial={{ opacity: 0, x: -8 }} animate={{ opacity: 1, x: 0 }} transition={{ delay: 0.05 * i }} className="relative flex gap-4 py-3">
              <span className={`relative z-10 grid size-14 shrink-0 place-items-center overflow-hidden rounded-full ${reached ? '' : 'opacity-60'}`} style={{ background: reached ? 'linear-gradient(160deg, #ffc24b, #ff8a3a)' : 'var(--color-raised)', boxShadow: isNext ? '0 0 0 2px var(--color-xp), 0 0 20px rgb(255 194 75 / 0.45)' : reached ? '0 0 16px rgb(255 194 75 / 0.4)' : 'inset 0 0 0 1px rgb(255 255 255 / 0.08)' }}>
                <img src={`./img/me-${lookFor(m.level)}-face.webp`} alt="" className={`size-[50px] rounded-full object-cover ${reached ? '' : 'grayscale'}`} />
                {!reached && !isNext && (
                  <span className="absolute inset-0 grid place-items-center bg-black/45">
                    <Lock size={16} />
                  </span>
                )}
              </span>
              <div className={`min-w-0 flex-1 pt-1 ${reached || isNext ? '' : 'opacity-70'}`}>
                <p className="flex items-baseline gap-2">
                  <span className="text-headline font-semibold">{m.title}</span>
                  <span className="font-mono text-footnote text-ink-2">Lv {m.level}</span>
                  {isNext && <span className="rounded-full px-2 py-0.5 text-caption font-semibold text-ground" style={{ background: 'var(--color-xp)' }}>Next</span>}
                </p>
                <p className="mt-0.5 text-subhead text-ink-2">{m.detail}</p>
                <div className="mt-2 flex flex-wrap gap-1.5">
                  {m.unlocks.map((u) => (
                    <span key={u} className="inline-flex items-center gap-1 rounded-full bg-fill-2 px-2.5 py-1 text-caption text-ink-2">
                      {reached ? <Check size={12} className="text-xp" /> : <Sparkles size={12} />}
                      {u}
                    </span>
                  ))}
                  {!safe && m.companion && <span className="inline-flex items-center rounded-full px-2.5 py-1 text-caption text-mind-ink" style={{ background: 'rgb(155 140 255 / 0.14)' }}>{m.companion}</span>}
                </div>
              </div>
            </motion.li>
          );
        })}
      </ol>
    </Sheet>
  );
}

/* ═══ profile ═══ */
function ProfileSheet() {
  const { close, safe, setSafe, lock, say } = useUI();
  const s = useSession();
  const pending = useOutbox((o) => o.pending);
  const { data: profile } = useProfile();
  const [sure, setSure] = useState(false);

  const unpair = async () => {
    if (!sure) return setSure(true);
    try {
      if (s.deviceId && !isDemo) await api.post(`/api/devices/${s.deviceId}/revoke`);
    } catch {
      /* the token is forgotten here either way */
    }
    s.unpair();
    close();
  };

  return (
    <Sheet onClose={close} title={profile?.name || 'You'} subtitle={isDemo ? 'Demo data. Nothing here syncs.' : s.server.replace(/^https?:\/\//, '')}>
      <div className="stone mt-2 rounded-xl">
        <button onClick={() => { setSafe(!safe); haptic('light'); say(safe ? 'Private mode' : 'Safe mode on'); }} className="hair flex min-h-[56px] w-full items-center gap-3 px-4 text-left">
          <ShieldCheck size={20} className="text-money" />
          <span className="flex-1 text-body">Safe mode</span>
          <span className={`relative h-[31px] w-[51px] rounded-full transition-colors ${safe ? 'bg-[#34d27b]' : 'bg-fill-3'}`}>
            <motion.span className="absolute top-[2px] size-[27px] rounded-full bg-white shadow" animate={{ left: safe ? 22 : 2 }} transition={{ type: 'spring', visualDuration: 0.25, bounce: 0.2 }} />
          </span>
        </button>
        <button onClick={() => { close(); lock(); }} className="hair flex min-h-[56px] w-full items-center gap-3 px-4 text-left">
          <Lock size={20} className="text-ink-2" />
          <span className="flex-1 text-body">Lock now</span>
        </button>
        <div className="flex min-h-[56px] items-center gap-3 px-4">
          <Server size={20} className="text-ink-2" />
          <span className="flex-1 text-body">Sync</span>
          <span className="text-subhead text-ink-2">{s.offline ? 'Offline' : pending ? `${pending} waiting` : 'Up to date'}</span>
        </div>
      </div>
      <p className="mt-2 px-1 text-footnote text-ink-3">Safe mode hides the companion and private habits, for when someone is next to you.</p>
      <div className="stone mt-6 rounded-xl">
        <button onClick={unpair} className="flex min-h-[56px] w-full items-center gap-3 px-4 text-left text-danger">
          <LogOut size={20} />
          <span className="flex-1 text-body">{sure ? 'Tap again to unpair this device' : 'Unpair this device'}</span>
        </button>
      </div>
    </Sheet>
  );
}

/* ═══ capture: quick add, with "at 18:00" parsing ═══ */
function Capture({ area, parent }: { area: string | null; parent?: string }) {
  const { close, say } = useUI();
  const { data: areas } = useAreas();
  const [text, setText] = useState('');
  const [pick, setPick] = useState<string | null>(area);
  const ref = useRef<HTMLTextAreaElement>(null);
  useEffect(() => {
    setTimeout(() => ref.current?.focus(), 250);
  }, []);
  const m = text.match(/\b(?:at\s+)?([01]?\d|2[0-3])[:.]([0-5]\d)\b/);
  const time = m ? `${m[1].padStart(2, '0')}:${m[2]}` : null;
  const title = (m ? text.replace(m[0], '') : text).replace(/\s{2,}/g, ' ').trim();

  const save = () => {
    if (!title) return;
    const d = new Date();
    const ymd = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
    act.add(title, pick, parent, time ? `${ymd}T${time}` : undefined);
    haptic('success');
    say(parent ? 'Step added' : 'Added to today');
    close();
  };

  return (
    <Sheet onClose={close} title={parent ? 'Add a step' : 'Capture'} subtitle={parent ? undefined : 'A task for today. Add a time like 18:00 to put it on the timeline.'}>
      <textarea
        ref={ref}
        value={text}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter' && !e.shiftKey) {
            e.preventDefault();
            save();
          }
        }}
        rows={2}
        placeholder={parent ? 'Next step' : 'What needs doing?'}
        className="mt-2 w-full resize-none rounded-[18px] bg-fill-2 p-4 text-title3 outline-none placeholder:text-ink-3"
      />
      {time && <p className="mt-2 text-subhead text-money-ink">On the timeline at {time}</p>}
      {!parent && (
        <div className="no-scrollbar -mx-5 mt-4 flex gap-2 overflow-x-auto px-5">
          {(areas?.pillars || []).flatMap((p) =>
            p.areas.map((a) => (
              <Chip key={a.id} active={pick === a.id} tint={PILLARS[p.id].glow} onClick={() => setPick(pick === a.id ? null : a.id)}>
                {a.name}
              </Chip>
            )),
          )}
        </div>
      )}
      <Button variant="primary" size="lg" className="mt-6 w-full" disabled={!title} onClick={save}>
        Add
      </Button>
    </Sheet>
  );
}

/* ═══ skip with a reason ═══ */
function Skip() {
  const { close, sheet, say } = useUI();
  const task = sheet?.kind === 'skip' ? sheet.task : null;
  if (!task) return null;
  return (
    <Sheet onClose={close} title="Skip for today" subtitle={task.title}>
      <p className="text-subhead text-ink-2">What got in the way? It feeds tonight’s review, nothing else.</p>
      <div className="mt-4 grid grid-cols-2 gap-2">
        {SKIP_REASONS.map((r) => (
          <button
            key={r.value}
            onClick={() => {
              act.skip(task, r.value);
              haptic('light');
              say('Skipped. Noted for tonight.');
              close();
            }}
            className="press h-[52px] rounded-[16px] bg-fill-2 text-callout font-medium active:bg-fill-3"
          >
            {r.label}
          </button>
        ))}
      </div>
    </Sheet>
  );
}

/* ═══ Wave: a ten-minute urge timer with paced breathing ═══ */
function Wave() {
  const close = useUI((s) => s.close);
  const [start] = useState(() => Date.now());
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    const t = setInterval(() => setNow(Date.now()), 250);
    return () => clearInterval(t);
  }, []);
  const left = Math.max(0, 600 - Math.floor((now - start) / 1000));
  const phase = Math.floor((now - start) / 4000) % 2 === 0 ? 'Breathe in' : 'Breathe out';
  return (
    <Sheet onClose={close} title="Wave" subtitle="Urges peak and pass. Ride this one out." tone="#3fd0ff">
      <div className="flex flex-col items-center py-6">
        <div className="relative grid size-[220px] place-items-center">
          <motion.span className="absolute inset-0 rounded-full" style={{ background: 'radial-gradient(circle, rgb(63 208 255 / 0.35), rgb(63 208 255 / 0) 70%)' }} animate={{ scale: [0.7, 1, 0.7] }} transition={{ duration: 8, repeat: Infinity, ease: 'easeInOut' }} />
          <motion.span className="absolute size-[120px] rounded-full" style={{ background: 'radial-gradient(circle at 35% 30%, white, #3fd0ff 45%, #0a5f86)', boxShadow: '0 0 40px #3fd0ff' }} animate={{ scale: [0.8, 1.15, 0.8] }} transition={{ duration: 8, repeat: Infinity, ease: 'easeInOut' }} />
        </div>
        <p className="mt-4 text-title2 font-bold">{phase}</p>
        <p className="fig mt-1 text-[44px] text-ink-2">
          {Math.floor(left / 60)}:{String(left % 60).padStart(2, '0')}
        </p>
        <div className="mt-6 grid w-full grid-cols-2 gap-2">
          {['Cold water', '20 push-ups', 'Walk outside', 'Call Nora'].map((a) => (
            <div key={a} className="grid h-12 place-items-center rounded-[16px] bg-fill-2 text-subhead">
              {a}
            </div>
          ))}
        </div>
        <Button variant="primary" size="lg" className="mt-6 w-full" onClick={close}>
          It passed
        </Button>
      </div>
    </Sheet>
  );
}

/* ═══ print today's plan ═══ */
function Print() {
  const { close, say } = useUI();
  const [busy, setBusy] = useState(false);
  const go = async () => {
    setBusy(true);
    try {
      await api.tool('print_morning', {});
      say('Sent to the printer');
      close();
    } catch (e) {
      say((e as Error).message, 'warn');
    } finally {
      setBusy(false);
    }
  };
  return (
    <Sheet onClose={close} title="Print today’s plan" subtitle="Schedule, tasks by area, inbox drops and Nora’s read of yesterday, on paper.">
      <div className="mt-2 flex items-center gap-3 rounded-[18px] bg-fill-1 p-4">
        <Printer size={22} className="text-xp" />
        <p className="text-subhead text-ink-2">Prints on the PC’s default printer. Plan on paper, photograph it, and Nora sorts the rest.</p>
      </div>
      <Button variant="primary" size="lg" className="mt-6 w-full" disabled={busy} onClick={go}>
        {busy ? 'Printing…' : 'Print now'}
      </Button>
    </Sheet>
  );
}

/* ═══ plan: give each unplaced task the next free slot ═══ */
function Plan() {
  const { close, say } = useUI();
  const d = useDay();
  // free slots of 30 minutes from the next half hour to 21:00, skipping booked blocks
  const slots = useMemo(() => {
    const out: number[] = [];
    const busy = d.blocks.filter((b) => !b.done);
    for (let t = Math.ceil((d.now + 5) / 30) * 30; t + 30 <= BED; t += 30) {
      if (!busy.some((b) => t < b.end && t + 30 > b.start)) out.push(t);
    }
    return out;
  }, [d.blocks, d.now]);
  const [sent, setSent] = useState(false);

  const askNora = async () => {
    setSent(true);
    try {
      await api.post('/api/nora/messages', { text: 'Plan the rest of my day: give my tasks without a time a slot before 21:00.', engine: 'auto' }, 120000);
      say('Nora is planning it');
      close();
    } catch (e) {
      setSent(false);
      say((e as Error).message, 'warn');
    }
  };

  return (
    <Sheet onClose={close} title="Plan the rest" subtitle={`${dur(d.free)} free before 21:00 · ${d.unplaced.length} without a time`}>
      {d.unplaced.length === 0 ? (
        <p className="py-6 text-center text-subhead text-ink-2">Everything has a time.</p>
      ) : (
        <div className="stone mt-1 rounded-xl">
          {d.unplaced.map(({ task, area, hue }, i) => {
            const slot = slots[i];
            return (
              <div key={task.id} className={`flex min-h-[60px] items-center gap-3 px-4 ${i < d.unplaced.length - 1 ? 'hair' : ''}`}>
                <span className="size-2 shrink-0 rounded-full" style={{ background: hue }} />
                <span className="min-w-0 flex-1 py-2.5">
                  <span className="block truncate text-body">{task.title}</span>
                  <span className="block truncate text-footnote text-ink-2">{area || 'No area'}</span>
                </span>
                {slot != null ? (
                  <button
                    onClick={() => {
                      act.schedule(task, hm(slot));
                      haptic('success');
                      say(`${task.title} at ${hm(slot)}`);
                    }}
                    className="press h-9 shrink-0 rounded-full bg-fill-2 px-3.5 font-mono text-subhead font-semibold tnum"
                  >
                    {hm(slot)}
                  </button>
                ) : (
                  <span className="text-footnote text-ink-3">No slot left</span>
                )}
              </div>
            );
          })}
        </div>
      )}
      <p className="mt-2 px-1 text-footnote text-ink-3">Each button is the next free half hour. Tap to put the task there.</p>
      {!isDemo && (
        <Button variant="secondary" size="lg" className="mt-5 w-full" disabled={sent} onClick={askNora}>
          Ask Nora to plan it
        </Button>
      )}
    </Sheet>
  );
}

/* ═══ an inbox message ═══ */
function InboxItemSheet() {
  const { close, sheet, say, push } = useUI();
  const item = sheet?.kind === 'inbox' ? sheet.item : null;
  if (!item) return null;
  const who = item.sender_name || item.chat_name || item.sender || 'Unknown';
  return (
    <Sheet onClose={close} title={who} subtitle={`${item.source === 'gmail' ? 'Mail' : item.source === 'whatsapp' ? 'WhatsApp' : 'App'} · ${ago(item.at)}`}>
      {item.subject && <p className="text-headline font-semibold">{item.subject}</p>}
      <p className="mt-2 text-callout whitespace-pre-wrap text-ink-2">{decode(item.text)}</p>
      <div className="mt-6 grid grid-cols-2 gap-2">
        <Button
          variant="secondary"
          size="lg"
          icon={Reply}
          onClick={() => {
            close();
            push({ kind: 'nora' });
          }}
        >
          Reply via Nora
        </Button>
        <Button
          variant="primary"
          size="lg"
          icon={Check}
          onClick={() => {
            act.inboxHandled(item, true);
            haptic('success');
            say('Marked handled');
            close();
          }}
        >
          Handled
        </Button>
      </div>
    </Sheet>
  );
}

export function decode(s: string): string {
  return s.replace(/&#39;/g, '’').replace(/&quot;/g, '"').replace(/&amp;/g, '&').replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/[͏‌­​]+/g, '').replace(/(\s*͏\s*)+/g, ' ').trim();
}
