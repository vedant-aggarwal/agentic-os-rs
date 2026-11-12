// Unlock: Face ID first, Safe mode right under it (someone is next to him: opens without
// anything private). The native shell swaps the tap for a biometric prompt (see REACT-UI-BUILD.md).
import { motion, AnimatePresence } from 'motion/react';
import { useState } from 'react';
import { ShieldCheck } from 'lucide-react';
import { haptic, useUI } from '@/lib/ui';
import { greeting } from '@/lib/meta';
import { spring } from '@/ui/motion';

type Phase = 'idle' | 'scan' | 'ok';

export function Unlock() {
  const unlock = useUI((s) => s.unlock);
  const [phase, setPhase] = useState<Phase>('idle');
  const now = new Date();

  const go = (safe: boolean) => {
    if (phase !== 'idle') return;
    haptic('light');
    if (safe) {
      unlock(true);
      return;
    }
    setPhase('scan');
    const bio = (window as unknown as { aosBiometric?: () => Promise<boolean> }).aosBiometric;
    const check = bio ? bio() : new Promise<boolean>((r) => setTimeout(() => r(true), 650));
    check.then((ok) => {
      if (!ok) return setPhase('idle');
      setPhase('ok');
      haptic('success');
      setTimeout(() => unlock(false), 420);
    });
  };

  return (
    <motion.div className="fixed inset-0 z-[60] flex flex-col overflow-hidden bg-ground" exit={{ opacity: 0, scale: 1.04 }} transition={{ duration: 0.45, ease: [0.22, 1, 0.36, 1] }}>
      <img src="./img/cave.webp" alt="" aria-hidden className="absolute inset-0 h-full w-full scale-110 object-cover opacity-70 blur-[2px]" />
      <div className="absolute inset-0" style={{ background: 'linear-gradient(180deg, rgb(5 5 7 / 0.35) 0%, rgb(5 5 7 / 0.15) 40%, rgb(5 5 7 / 0.92) 78%, #050507 100%)' }} />

      <div className="relative flex flex-1 flex-col items-center pt-safe">
        <div className="mt-[9vh] text-center">
          <p className="text-subhead font-medium text-ink-2">
            {now.toLocaleDateString('en-GB', { weekday: 'long', day: 'numeric', month: 'long' })}
          </p>
          <p className="fig mt-1 text-[76px] leading-[80px] font-[600] tracking-[-0.04em]" style={{ textShadow: '0 4px 40px rgb(0 0 0 / 0.5)' }}>
            {now.toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit' })}
          </p>
          <p className="mt-2 text-callout text-ink-2">{greeting(now)}</p>
        </div>

        <div className="flex-1" />

        <button aria-label="Unlock with Face ID" onClick={() => go(false)} className="press relative grid size-[112px] place-items-center rounded-[34px]">
          <span className="glass absolute inset-0 rounded-[34px]" />
          <FaceId phase={phase} />
        </button>
        <AnimatePresence mode="wait">
          <motion.p
            key={phase}
            initial={{ opacity: 0, y: 4 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -4 }}
            transition={{ duration: 0.18 }}
            className="mt-4 text-headline font-semibold"
          >
            {phase === 'idle' ? 'Tap to unlock' : phase === 'scan' ? 'Looking for you…' : 'Welcome back'}
          </motion.p>
        </AnimatePresence>

        <div className="mt-10 flex flex-col items-center pb-[calc(var(--safe-b)+24px)]">
          <button onClick={() => go(true)} className="press glass flex h-[52px] items-center gap-2.5 rounded-full px-6">
            <ShieldCheck size={19} strokeWidth={2.1} className="text-money" />
            <span className="text-headline font-semibold">Safe mode</span>
          </button>
          <p className="mt-3 max-w-[32ch] text-center text-footnote text-ink-3">Someone next to you? Opens without anything private.</p>
        </div>
      </div>
    </motion.div>
  );
}

/** The Face ID glyph: corners close in while scanning, then a tick. */
function FaceId({ phase }: { phase: Phase }) {
  const c = phase === 'ok' ? 'var(--color-money)' : 'var(--color-ink)';
  const inset = phase === 'scan' ? 4 : 0;
  return (
    <svg width="56" height="56" viewBox="0 0 56 56" fill="none" stroke={c} strokeWidth="3.2" strokeLinecap="round" strokeLinejoin="round" className="relative">
      {[
        'M4 17V10a6 6 0 0 1 6-6h7',
        'M39 4h7a6 6 0 0 1 6 6v7',
        'M52 39v7a6 6 0 0 1-6 6h-7',
        'M17 52h-7a6 6 0 0 1-6-6v-7',
      ].map((d, i) => (
        <motion.path
          key={i}
          d={d}
          animate={{ x: (i === 0 || i === 3 ? 1 : -1) * inset, y: (i < 2 ? 1 : -1) * inset, stroke: c }}
          transition={phase === 'scan' ? { repeat: Infinity, repeatType: 'reverse', duration: 0.45 } : spring.snappy}
        />
      ))}
      <AnimatePresence mode="wait">
        {phase === 'ok' ? (
          <motion.path key="tick" d="M18 29l7 7 13-15" initial={{ pathLength: 0 }} animate={{ pathLength: 1 }} transition={{ duration: 0.3 }} />
        ) : (
          <motion.g key="face" exit={{ opacity: 0 }} transition={{ duration: 0.12 }}>
            <path d="M19 20v4M37 20v4M28 20v10l-2.5 1" />
            <path d="M20.5 37.5c4.5 4 10.5 4 15 0" />
          </motion.g>
        )}
      </AnimatePresence>
    </svg>
  );
}
