// Nora: her character with the state ring in place of the old orb, the one merged thread
// (Claude and Grok Bot), streaming replies, engine switch as icons.
import { AnimatePresence, motion } from 'motion/react';
import { useEffect, useMemo, useRef, useState } from 'react';
import { ArrowUp, Bot, Phone, Sparkles, Wand2 } from 'lucide-react';
import { api } from '@/lib/api';
import { noraStream } from '@/lib/live';
import { qc, useNoraThread, useOrb } from '@/lib/queries';
import { ago } from '@/lib/meta';
import type { NoraMsg } from '@/lib/types';
import { haptic, useUI } from '@/lib/ui';
import { startCall } from '@/call/store';
import { Page } from '@/ui/Page';

type Engine = 'auto' | 'claude' | 'grok';
const ENGINES: { id: Engine; label: string; icon: typeof Bot }[] = [
  { id: 'auto', label: 'Auto', icon: Wand2 },
  { id: 'claude', label: 'Claude', icon: Sparkles },
  { id: 'grok', label: 'Grok', icon: Bot },
];

export function NoraPage() {
  const { pop, say } = useUI();
  const { data: orb } = useOrb();
  const { data: thread } = useNoraThread();
  const [engine, setEngine] = useState<Engine>('auto');
  const [text, setText] = useState('');
  const [typing, setTyping] = useState<string | null>(null); // streaming text of the reply
  const [step, setStep] = useState<string | null>(null);
  const [sending, setSending] = useState(false);
  const end = useRef<HTMLDivElement>(null);
  const color = orb?.color || '#6daeff';

  useEffect(
    () =>
      noraStream((e) => {
        if (e.type === 'turn_started') setTyping('');
        else if (e.type === 'delta') setTyping((t) => (t ?? '') + (e as { text: string }).text);
        else if (e.type === 'tool') setStep((e as { title: string; done: boolean }).done ? null : (e as { title: string }).title);
        else if (e.type === 'message' || e.type === 'message_updated') {
          const msg = (e as { msg: NoraMsg }).msg;
          qc.setQueryData<NoraMsg[]>(['nora'], (old = []) => {
            const i = old.findIndex((m) => m.id === msg.id);
            if (i >= 0) return old.map((m) => (m.id === msg.id ? msg : m));
            return [...old, msg];
          });
          if (msg.engine !== 'user') {
            setTyping(null);
            setStep(null);
          }
        } else if (e.type === 'turn_done') {
          setTyping(null);
          setStep(null);
        } else if (e.type === 'orb_state') qc.setQueryData(['orb'], (e as { orb: unknown }).orb);
      }),
    [],
  );

  const msgs = useMemo(() => (thread || []).filter((m) => m.kind === 'message' && m.text.trim()).slice(-60), [thread]);

  useEffect(() => {
    end.current?.scrollIntoView({ block: 'end' });
  }, [msgs.length, typing]);

  const send = async () => {
    const t = text.trim();
    if (!t || sending) return;
    setText('');
    setSending(true);
    haptic('light');
    const tmp: NoraMsg = { id: -Date.now(), engine: 'user', source: 'app', kind: 'message', text: t, at: Date.now(), turn_id: null };
    qc.setQueryData<NoraMsg[]>(['nora'], (old = []) => [...old, tmp]);
    try {
      await api.post('/api/nora/messages', { text: t, engine }, 120000);
      void qc.invalidateQueries({ queryKey: ['nora'] });
    } catch (e) {
      say((e as Error).message, 'warn');
      setText(t);
      qc.setQueryData<NoraMsg[]>(['nora'], (old = []) => old.filter((m) => m.id !== tmp.id));
    } finally {
      setSending(false);
    }
  };

  const call = () => {
    haptic('medium');
    startCall('Tapped Call on the Nora page');
  };

  return (
    <Page
      onBack={pop}
      title="Nora"
      tone={color}
      flush
      trailing={
        <button aria-label="Call Nora" onClick={call} className="glass press grid size-11 place-items-center rounded-full">
          <Phone size={19} />
        </button>
      }
    >
      <div className="flex min-h-full flex-col">
        <div className="flex flex-col items-center pt-[calc(var(--safe-t)+56px)]">
          <div className="relative grid size-[132px] place-items-center">
            <motion.span aria-hidden className="absolute inset-[-14px] rounded-full" style={{ background: `conic-gradient(from 0deg, ${color}, #9b8cff, #ff8ad8, ${color})`, filter: 'blur(18px)', opacity: 0.55 }} animate={{ rotate: 360, scale: typing !== null ? [1, 1.08, 1] : 1 }} transition={{ rotate: { repeat: Infinity, duration: 12, ease: 'linear' }, scale: { repeat: Infinity, duration: 1.4 } }} />
            <span className="absolute inset-0 rounded-full p-[3px]" style={{ background: `conic-gradient(from 200deg, ${color}, #9b8cff 50%, ${color})` }}>
              <img src="./img/nora.webp" alt="Nora" className="size-full rounded-full border-[3px] border-ground object-cover" />
            </span>
          </div>
          <p className="mt-4 text-title2 font-bold">Nora</p>
          <p className="mt-0.5 flex items-center gap-1.5 text-subhead text-ink-2">
            <span className="size-2 rounded-full" style={{ background: color, boxShadow: `0 0 8px ${color}` }} />
            {typing !== null ? 'Writing…' : orb?.label || 'Here'}
            {orb?.reasons?.[0] && typing === null ? <span className="text-ink-3"> · {orb.reasons[0]}</span> : null}
          </p>
        </div>

        <div className="mt-6 flex-1 space-y-2 px-gutter pb-4">
          {msgs.map((m, i) => {
            const mine = m.engine === 'user' && m.source !== 'grok';
            const prev = msgs[i - 1];
            const gap = !prev || m.at - prev.at > 30 * 60e3;
            return (
              <div key={m.id}>
                {gap && <p className="py-2 text-center text-caption text-ink-3">{ago(m.at)}</p>}
                <motion.div initial={{ opacity: 0, y: 6 }} animate={{ opacity: 1, y: 0 }} className={`flex ${mine ? 'justify-end' : 'justify-start'}`}>
                  <div className={`max-w-[82%] rounded-[20px] px-3.5 py-2.5 text-callout whitespace-pre-wrap ${mine ? 'rounded-br-[6px] bg-ink text-ground' : 'stone rounded-bl-[6px]'}`}>
                    {!mine && m.engine === 'grok' && <span className="mb-0.5 block text-caption font-semibold text-mind-ink">Grok</span>}
                    {m.text}
                  </div>
                </motion.div>
              </div>
            );
          })}
          <AnimatePresence>
            {typing !== null && (
              <motion.div initial={{ opacity: 0, y: 6 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0 }} className="flex justify-start">
                <div className="stone max-w-[82%] rounded-[20px] rounded-bl-[6px] px-3.5 py-2.5 text-callout whitespace-pre-wrap">
                  {step && <span className="mb-1 block text-caption text-ink-3">{step}</span>}
                  {typing || <Dots />}
                </div>
              </motion.div>
            )}
          </AnimatePresence>
        </div>

        <div className="sticky bottom-0 px-gutter pt-6 pb-[calc(var(--safe-b)+10px)]" style={{ background: 'linear-gradient(180deg, rgb(5 5 7 / 0), #050507 30%)' }}>
          <div className="mb-2 flex gap-1.5">
            {ENGINES.map(({ id, label, icon: Icon }) => (
              <button key={id} aria-label={label} aria-pressed={engine === id} onClick={() => setEngine(id)} className={`press flex h-8 items-center gap-1 rounded-full px-2.5 text-footnote font-medium ${engine === id ? 'bg-fill-3 text-ink' : 'text-ink-3'}`}>
                <Icon size={14} /> {label}
              </button>
            ))}
          </div>
          <div className="glass flex items-end gap-2 rounded-[24px] p-1.5 pl-4">
            <textarea
              value={text}
              onChange={(e) => setText(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter' && !e.shiftKey) {
                  e.preventDefault();
                  void send();
                }
              }}
              rows={1}
              placeholder="Message Nora"
              className="max-h-32 min-h-[36px] flex-1 resize-none bg-transparent py-2 text-callout outline-none placeholder:text-ink-3"
            />
            <button aria-label="Send" disabled={!text.trim() || sending} onClick={send} className="press grid size-9 shrink-0 place-items-center rounded-full bg-ink text-ground disabled:opacity-30">
              <ArrowUp size={19} strokeWidth={2.6} />
            </button>
          </div>
          <div ref={end} />
        </div>
      </div>
    </Page>
  );
}

function Dots() {
  return (
    <span className="inline-flex gap-1 py-1">
      {[0, 1, 2].map((i) => (
        <motion.span key={i} className="size-1.5 rounded-full bg-ink-2" animate={{ opacity: [0.3, 1, 0.3] }} transition={{ repeat: Infinity, duration: 1, delay: i * 0.15 }} />
      ))}
    </span>
  );
}
