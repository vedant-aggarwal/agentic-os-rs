// Nora's call screen and the pill (design/specs/orb-and-call.md, ported from src/call/screen.rs).
// Full screen: name and timer, her portrait with the state ring, the particle orb, live caption,
// status, and four controls (mute, speaker, chat, end). Leaving keeps the call in a pill.
import { AnimatePresence, motion } from 'motion/react';
import { useEffect, useRef, useState } from 'react';
import { ChevronDown, MessageCircle, Mic, MicOff, PhoneOff, Volume2, VolumeX } from 'lucide-react';
import { useNoraThread, useOrb } from '@/lib/queries';
import { useUI } from '@/lib/ui';
import { spring } from '@/ui/motion';
import { closeWrap, endCall, mood, setMuted, startCall, toggleSpeaker, useCall } from './store';

function useTimer(startedAt: number, live: boolean) {
  const [, force] = useState(0);
  useEffect(() => {
    if (!live) return;
    const t = setInterval(() => force((n) => n + 1), 1000);
    return () => clearInterval(t);
  }, [live]);
  const s = startedAt ? Math.max(0, Math.floor((Date.now() - startedAt) / 1000)) : 0;
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;
}

export function CallLayer() {
  const c = useCall();
  const show = c.phase !== 'idle';
  return (
    <>
      <AnimatePresence>{show && c.open && <CallScreen key="screen" />}</AnimatePresence>
      <AnimatePresence>{(c.phase === 'live' || c.phase === 'connecting') && !c.open && <CallPill key="pill" />}</AnimatePresence>
    </>
  );
}

function CallScreen() {
  const c = useCall();
  const { data: orb } = useOrb();
  const safe = useUI((s) => s.safe);
  const color = orb?.color || '#6daeff';
  const m = mood(c);
  const live = c.phase === 'live';
  const timer = useTimer(c.startedAt, live);
  const canvas = useRef<HTMLCanvasElement>(null);
  const [fallback, setFallback] = useState(false);
  const [why, setWhy] = useState(false);

  useEffect(() => {
    const el = canvas.current;
    if (!el || !window.aosCall) return setFallback(true);
    const ok = window.aosCall.mountOrb(el);
    setFallback(!ok || el.dataset.fallback === '1');
    return () => window.aosCall?.unmountOrb();
  }, []);

  useEffect(() => {
    window.aosCall?.setOrb({ color, thinking: m === 'thinking' || m === 'connecting', muted: c.muted, stress: orb?.state === 'stressed' });
  }, [color, m, c.muted, orb?.state]);

  const ended = c.phase === 'ended' || c.phase === 'failed';

  return (
    <motion.div
      className="fixed inset-0 z-[80] grid grid-rows-[auto_1fr_auto] overflow-hidden bg-ground px-4 pt-[calc(var(--safe-t)+8px)] pb-[calc(var(--safe-b)+18px)]"
      style={{ ['--orb' as string]: color }}
      initial={{ opacity: 0, scale: 0.94, y: -18 }}
      animate={{ opacity: 1, scale: 1, y: 0 }}
      exit={{ opacity: 0, scale: 0.94, y: -24 }}
      transition={spring.page}
      data-mood={m}
    >
      {/* the soft panel in his colour behind the orb */}
      <div
        aria-hidden
        className="pointer-events-none absolute inset-x-3.5 top-[calc(var(--safe-t)+70px)] bottom-[calc(var(--safe-b)+124px)] rounded-[44px]"
        style={{
          background: `radial-gradient(70% 46% at 50% 62%, color-mix(in oklab, ${color} 42%, transparent), transparent 72%), radial-gradient(90% 60% at 50% 18%, color-mix(in oklab, ${color} 12%, transparent), transparent 70%), linear-gradient(180deg, #0c0c12, #08080c)`,
          boxShadow: 'inset 0 1px 0 rgb(255 255 255 / 0.06), 0 0 0 1px rgb(255 255 255 / 0.04)',
          filter: m === 'muted' ? 'saturate(0.3) brightness(0.7)' : undefined,
          transition: 'background 1.6s, filter 400ms',
        }}
      />

      <div className="relative grid min-h-14 grid-cols-[44px_1fr_56px] items-center gap-2">
        <button aria-label="Leave the call screen" onClick={() => useCall.setState({ open: false, chat: false })} className="glass press grid size-11 place-items-center rounded-full" disabled={ended}>
          <ChevronDown size={22} />
        </button>
        <div className="flex flex-col items-center leading-tight">
          <span className="text-headline font-semibold">Nora</span>
          <span className="font-mono text-footnote text-ink-2 tnum">{c.phase === 'connecting' ? 'Connecting…' : ended ? 'Ended' : timer}</span>
        </div>
        <span className="relative size-[52px] justify-self-end rounded-full">
          <motion.span
            aria-hidden
            className="absolute inset-0 rounded-full p-[2px]"
            style={{
              background: `conic-gradient(from 210deg, ${color}, #9b8cff, #46e0d4, ${color})`,
              WebkitMask: 'linear-gradient(#000 0 0) content-box exclude, linear-gradient(#000 0 0)',
              mask: 'linear-gradient(#000 0 0) content-box exclude, linear-gradient(#000 0 0)',
              filter: m === 'muted' || m === 'ended' ? 'grayscale(1)' : undefined,
              opacity: m === 'muted' || m === 'ended' ? 0.5 : 1,
            }}
            animate={m === 'thinking' ? { rotate: 360 } : m === 'connecting' ? { opacity: [0.45, 1] } : { rotate: 0 }}
            transition={m === 'thinking' ? { repeat: Infinity, duration: 2.4, ease: 'linear' } : { repeat: m === 'connecting' ? Infinity : 0, repeatType: 'reverse', duration: 1.2 }}
          />
          <img src="./img/nora-round.webp" alt="" className="size-full rounded-full object-cover p-1" />
        </span>
      </div>

      <div className="relative flex min-h-0 flex-col items-center justify-center px-5 pt-3 text-center">
        {!ended && (
          <p className="line-clamp-3 min-h-[2.6em] max-w-[24ch] text-[clamp(22px,6.2vw,30px)] leading-[1.22] font-[560] tracking-[-0.015em] text-balance">
            {c.caption || (c.phase === 'connecting' ? 'Calling Nora…' : '')}
          </p>
        )}
        {c.heard && !ended && <p className="mt-2.5 line-clamp-2 max-w-[34ch] text-subhead text-ink-2">{c.heard}</p>}
        <div
          className={`relative mt-2 aspect-square touch-none select-none transition-[width] duration-500 ${ended ? 'w-[min(52vw,220px,28svh)]' : 'w-[min(78vw,340px,42svh)]'}`}
          onPointerDown={() => {
            const t = setTimeout(() => setWhy(true), 500);
            const up = () => {
              clearTimeout(t);
              setWhy(false);
              removeEventListener('pointerup', up);
            };
            addEventListener('pointerup', up);
          }}
        >
          <canvas ref={canvas} className="absolute -inset-[14%] h-[128%] w-[128%]" />
          {fallback && (
            <span
              className="absolute inset-[22%] rounded-full"
              style={{
                background: `radial-gradient(circle at 35% 30%, #fff 0, color-mix(in oklab, ${color} 70%, #fff) 18%, ${color} 55%, transparent 72%)`,
                transform: 'scale(calc(1 + var(--call-lvl, 0) * 0.25))',
                animation: m === 'thinking' ? 'breathe 1.2s ease-in-out infinite' : 'breathe 4s ease-in-out infinite',
              }}
            />
          )}
        </div>
        <p className="mt-0.5 min-h-[1.4em] text-subhead" style={{ color: m === 'thinking' ? `color-mix(in oklab, ${color} 60%, white)` : 'var(--color-ink-2)' }}>
          {ended ? '' : c.status || (m === 'muted' ? 'Muted' : m === 'listening' ? 'Listening' : m === 'speaking' ? 'Speaking' : m === 'thinking' ? 'Thinking' : '')}
        </p>
        {why && !safe && orb && (
          <div className="mt-2.5 flex flex-col gap-0.5 rounded-[18px] bg-[rgb(20_20_26/0.85)] px-4 py-3 text-footnote text-ink-2 shadow-[inset_0_0_0_1px_var(--color-line)]">
            <strong style={{ color }}>{orb.label}</strong>
            {orb.reasons.map((r) => (
              <span key={r}>{r}</span>
            ))}
          </div>
        )}
      </div>

      {ended ? (
        <Wrap />
      ) : (
        <div className="relative grid grid-cols-4 gap-3.5 justify-self-center rounded-full bg-[rgb(24_24_31/0.6)] px-3.5 py-2.5 shadow-[inset_0_0_0_1px_rgb(255_255_255/0.08)] backdrop-blur-[18px]">
          <CtlButton label={c.muted ? 'Unmute' : 'Mute'} on={c.muted} onClick={() => setMuted(!c.muted)}>
            {c.muted ? <MicOff size={24} /> : <Mic size={24} />}
          </CtlButton>
          <CtlButton label="Speaker" on={!c.speaker} onClick={toggleSpeaker}>
            {c.speaker ? <Volume2 size={24} /> : <VolumeX size={24} />}
          </CtlButton>
          <CtlButton label="Chat" on={c.chat} onClick={() => useCall.setState({ chat: !c.chat })}>
            <MessageCircle size={24} />
          </CtlButton>
          <button aria-label="End call" onClick={endCall} className="press grid size-16 place-items-center rounded-full text-white" style={{ background: 'linear-gradient(180deg, #ff7a63, #ff5a4e 55%, #f2453a)', boxShadow: '0 8px 24px rgb(255 90 78 / 0.35)' }}>
            <PhoneOff size={25} />
          </button>
        </div>
      )}

      <AnimatePresence>{c.chat && !ended && <CallChat key="chat" />}</AnimatePresence>
    </motion.div>
  );
}

function CtlButton({ label, on, onClick, children }: { label: string; on?: boolean; onClick: () => void; children: React.ReactNode }) {
  return (
    <button aria-label={label} aria-pressed={on} onClick={onClick} className={`press grid size-16 place-items-center rounded-full transition-colors ${on ? 'bg-ink text-ground' : 'bg-raised text-ink'}`}>
      {children}
    </button>
  );
}

function Wrap() {
  const c = useCall();
  return (
    <motion.div initial={{ opacity: 0, y: 20 }} animate={{ opacity: 1, y: 0 }} transition={spring.page} className="relative mx-1 flex flex-col gap-2.5 self-end rounded-[28px] bg-[rgb(17_17_21/0.92)] p-5 shadow-[inset_0_0_0_1px_var(--color-line)]">
      <h2 className="text-title3 font-semibold">{c.phase === 'failed' ? 'The call didn’t connect' : 'Call ended'}</h2>
      {c.phase === 'failed' ? (
        <p className="text-subhead text-ink-2">{c.error}</p>
      ) : c.summary ? (
        <>
          <p className="text-callout leading-[1.45]">{c.summary}</p>
          {c.actions.length > 0 && (
            <ul className="list-disc pl-5 text-subhead">
              {c.actions.map((a) => (
                <li key={a} className="mt-1">
                  {a}
                </li>
              ))}
            </ul>
          )}
          <p className="text-footnote text-ink-3">Saved to the Nora thread and your journal.</p>
        </>
      ) : (
        <p className="text-subhead text-ink-2">{c.writeup === 'running' || c.writeup === 'none' ? 'Writing up the call…' : 'Nothing to write up.'}</p>
      )}
      <div className="mt-1 flex gap-2">
        {c.phase === 'failed' && (
          <button onClick={() => startCall()} className="press h-12 flex-1 rounded-full bg-fill-2 text-subhead font-semibold">
            Try again
          </button>
        )}
        <button onClick={closeWrap} className="press h-12 flex-1 rounded-full bg-ink text-subhead font-semibold text-ground">
          Done
        </button>
      </div>
    </motion.div>
  );
}

function CallChat() {
  const { data: thread } = useNoraThread();
  const msgs = (thread || []).filter((m) => m.kind === 'message' && m.text.trim()).slice(-30);
  const end = useRef<HTMLDivElement>(null);
  useEffect(() => {
    end.current?.scrollIntoView({ block: 'end' });
  }, [msgs.length]);
  return (
    <motion.div
      className="absolute inset-x-0 bottom-0 z-[3] flex h-[64%] flex-col rounded-t-[30px] bg-[rgb(12_12_16/0.96)] px-3.5 pt-3.5 pb-[calc(var(--safe-b)+14px)] shadow-[0_-1px_0_rgb(255_255_255/0.08)]"
      initial={{ y: '102%' }}
      animate={{ y: 0 }}
      exit={{ y: '102%' }}
      transition={spring.sheet}
    >
      <button onClick={() => useCall.setState({ chat: false })} className="press self-center rounded-full bg-fill-2 px-4 py-2 text-footnote font-medium">
        Close chat
      </button>
      <div className="no-scrollbar mt-3 flex flex-1 flex-col gap-2 overflow-y-auto">
        {msgs.map((m) => {
          const mine = m.engine === 'user' && m.source !== 'grok';
          return (
            <div key={m.id} className={`flex ${mine ? 'justify-end' : 'justify-start'}`}>
              <div className={`max-w-[82%] rounded-[18px] px-3.5 py-2 text-subhead whitespace-pre-wrap ${mine ? 'bg-ink text-ground' : 'bg-raised'}`}>
                {!mine && <span className="mr-1.5 inline-block size-3 rounded-full align-[-1px]" style={{ background: 'radial-gradient(circle at 35% 30%, #fff 0, color-mix(in oklab, var(--orb) 70%, #fff) 22%, var(--orb) 60%, transparent 75%)' }} />}
                {m.text}
              </div>
            </div>
          );
        })}
        <div ref={end} />
      </div>
    </motion.div>
  );
}

/** The call left running: a capsule at the top with the timer, mute and end. */
function CallPill() {
  const c = useCall();
  const { data: orb } = useOrb();
  const timer = useTimer(c.startedAt, c.phase === 'live');
  const color = orb?.color || '#6daeff';
  return (
    <motion.div
      className="fixed inset-x-0 top-[calc(var(--safe-t)+66px)] z-[75] flex justify-center"
      initial={{ y: -40, opacity: 0, scale: 0.9 }}
      animate={{ y: 0, opacity: 1, scale: 1 }}
      exit={{ y: -40, opacity: 0, scale: 0.9 }}
      transition={spring.snappy}
    >
      <div className="flex h-12 items-center gap-1 rounded-full bg-black py-1 pr-1 pl-3 shadow-[0_10px_30px_rgb(0_0_0/0.6),inset_0_0_0_1px_rgb(255_255_255/0.08)]">
        <button onClick={() => useCall.setState({ open: true })} className="flex h-10 items-center gap-2.5 pr-2" aria-label="Back to the call">
          <span
            className="size-5 rounded-full"
            style={{ background: `radial-gradient(circle at 35% 30%, #fff 0, color-mix(in oklab, ${color} 70%, #fff) 22%, ${color} 60%, transparent 75%)`, transform: 'scale(calc(1 + var(--call-lvl, 0) * 0.4))' }}
          />
          <span className="text-subhead font-semibold">Nora</span>
          <span className="font-mono text-footnote text-ink-2 tnum">{c.phase === 'connecting' ? '…' : timer}</span>
        </button>
        <button aria-label={c.muted ? 'Unmute' : 'Mute'} onClick={() => setMuted(!c.muted)} className={`grid size-10 place-items-center rounded-full ${c.muted ? 'bg-ink text-ground' : 'bg-fill-2'}`}>
          {c.muted ? <MicOff size={17} /> : <Mic size={17} />}
        </button>
        <button aria-label="End call" onClick={endCall} className="grid size-10 place-items-center rounded-full bg-[#ff5a4e] text-white">
          <PhoneOff size={17} />
        </button>
      </div>
    </motion.div>
  );
}
