// Mood and energy on the front (he forgets otherwise). The card shows the last check-in as a
// glowing circle (size = energy, colour = mood) and pulses gently when today has none.
// The sheet: drag the glowing circle (left-right = how it feels, up-down = energy), then why.
import { AnimatePresence, motion, useMotionValue, useTransform } from 'motion/react';
import { useRef, useState } from 'react';
import { ChevronRight } from 'lucide-react';
import { act, useToday } from '@/lib/queries';
import { MOOD_REASONS, hhmm } from '@/lib/meta';
import { haptic, useUI } from '@/lib/ui';
import { Button, Chip } from '@/ui/kit';
import { Sheet } from '@/ui/Sheet';
import { spring } from '@/ui/motion';

const STOPS: [number, [number, number, number]][] = [
  [-1, [122, 92, 255]],
  [-0.4, [77, 124, 255]],
  [0.1, [63, 208, 255]],
  [0.55, [72, 220, 150]],
  [1, [255, 194, 75]],
];

export function moodColor(v: number): string {
  for (let i = 1; i < STOPS.length; i++) {
    const [b, cb] = STOPS[i];
    const [a, ca] = STOPS[i - 1];
    if (v <= b) {
      const t = (v - a) / (b - a);
      const c = ca.map((x, k) => Math.round(x + (cb[k] - x) * t));
      return `rgb(${c[0]} ${c[1]} ${c[2]})`;
    }
  }
  return 'rgb(255 194 75)';
}

export function moodWord(v: number): string {
  if (v < -0.66) return 'Very unpleasant';
  if (v < -0.25) return 'Unpleasant';
  if (v < -0.08) return 'Slightly unpleasant';
  if (v <= 0.08) return 'Neutral';
  if (v <= 0.25) return 'Slightly pleasant';
  if (v <= 0.66) return 'Pleasant';
  return 'Very pleasant';
}

const ENERGY = ['', 'Drained', 'Low', 'Steady', 'Good', 'Full'];

export function MoodCard() {
  const { data: today } = useToday();
  const open = useUI((s) => s.open);
  const last = today?.moods.at(-1);
  const color = last ? moodColor(last.valence) : 'rgb(255 255 255 / 0.5)';
  const size = last ? 16 + (last.energy ?? 3) * 3 : 22;

  return (
    <button onClick={() => open({ kind: 'mood' })} className="stone press mx-gutter flex w-[calc(100%-32px)] items-center gap-4 rounded-xl p-4 text-left">
      <span className="relative grid size-14 shrink-0 place-items-center rounded-full bg-fill-1">
        {!last && (
          <span
            className="absolute inset-0 rounded-full"
            style={{ background: 'conic-gradient(from 0deg, #7a5cff, #3fd0ff, #48dc96, #ffc24b, #7a5cff)', padding: 2.5, WebkitMask: 'radial-gradient(closest-side, transparent 85%, #000 87%)', mask: 'radial-gradient(closest-side, transparent 85%, #000 87%)', animation: 'spin-slow 8s linear infinite' }}
          />
        )}
        <span
          className="rounded-full"
          style={{
            width: size,
            height: size,
            background: last ? `radial-gradient(circle at 35% 30%, white, ${color} 45%, color-mix(in oklab, ${color} 60%, black))` : 'transparent',
            boxShadow: last ? `0 0 18px ${color}, 0 0 36px color-mix(in oklab, ${color} 50%, transparent)` : undefined,
            animation: last ? undefined : 'breathe 3.6s ease-in-out infinite',
          }}
        />
      </span>
      <span className="min-w-0 flex-1">
        <span className="block text-headline font-semibold">{last ? moodWord(last.valence) : 'How are you right now?'}</span>
        <span className="mt-0.5 block truncate text-subhead text-ink-2">
          {last ? `Energy ${last.energy ?? '–'} of 5 · ${hhmm(new Date(last.at))}${last.reasons.length ? ' · ' + last.reasons.join(', ') : ''}` : 'Mood and energy, ten seconds'}
        </span>
      </span>
      <ChevronRight size={18} className="shrink-0 text-ink-3" />
    </button>
  );
}

export function MoodSheet() {
  const close = useUI((s) => s.close);
  const say = useUI((s) => s.say);
  const [step, setStep] = useState<0 | 1>(0);
  const [val, setVal] = useState(0);
  const [energy, setEnergy] = useState(3);
  const [why, setWhy] = useState<string[]>([]);
  const [note, setNote] = useState('');
  const pad = useRef<HTMLDivElement>(null);
  const x = useMotionValue(0.5);
  const y = useMotionValue(0.5);
  const left = useTransform(x, (v) => `${v * 100}%`);
  const top = useTransform(y, (v) => `${v * 100}%`);
  const orbSize = useTransform(y, (v) => 56 + (1 - v) * 56);
  const color = moodColor(val);

  const setFrom = (e: React.PointerEvent) => {
    const r = pad.current!.getBoundingClientRect();
    const nx = Math.max(0, Math.min(1, (e.clientX - r.left) / r.width));
    const ny = Math.max(0, Math.min(1, (e.clientY - r.top) / r.height));
    x.set(nx);
    y.set(ny);
    const v = Math.round((nx * 2 - 1) * 100) / 100;
    const en = Math.max(1, Math.min(5, Math.round((1 - ny) * 4) + 1));
    if (en !== energy) haptic('light');
    setVal(v);
    setEnergy(en);
  };

  const save = () => {
    act.mood(val, energy, why, note.trim() || undefined);
    haptic('success');
    say('Mood saved');
    close();
  };

  return (
    <Sheet onClose={close} title={step === 0 ? 'How do you feel?' : 'What’s behind it?'} subtitle={step === 0 ? 'Drag the light. Left to right is how it feels, up is energy.' : 'Optional. Skipping still saves.'} tone={color}>
      <AnimatePresence mode="wait" initial={false}>
        {step === 0 ? (
          <motion.div key="pad" initial={{ opacity: 0, x: -20 }} animate={{ opacity: 1, x: 0 }} exit={{ opacity: 0, x: -20 }} transition={spring.page}>
            <div className="mt-2 text-center">
              <p className="text-title2 font-bold" style={{ color: `color-mix(in oklab, ${color} 55%, white)` }}>
                {moodWord(val)}
              </p>
              <p className="mt-0.5 text-subhead text-ink-2">
                Energy {energy} of 5 · {ENERGY[energy]}
              </p>
            </div>
            <div
              ref={pad}
              className="relative mx-auto mt-5 aspect-square w-full max-w-[320px] touch-none rounded-[32px]"
              style={{
                background: `radial-gradient(60% 60% at ${50 + val * 30}% 50%, color-mix(in oklab, ${color} 22%, transparent), transparent 70%), var(--color-fill-1)`,
                boxShadow: 'inset 0 1px 0 rgb(255 255 255 / 0.06)',
              }}
              onPointerDown={(e) => {
                (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
                setFrom(e);
              }}
              onPointerMove={(e) => e.buttons && setFrom(e)}
            >
              {/* guides */}
              <div className="absolute inset-x-6 top-1/2 h-px bg-white/[0.06]" />
              <div className="absolute inset-y-6 left-1/2 w-px bg-white/[0.06]" />
              <span className="absolute top-3 left-1/2 -translate-x-1/2 text-caption text-ink-3">More energy</span>
              <span className="absolute bottom-3 left-1/2 -translate-x-1/2 text-caption text-ink-3">Less</span>
              <span className="absolute top-1/2 left-3 -translate-y-1/2 text-caption text-ink-3">Unpleasant</span>
              <span className="absolute top-1/2 right-3 -translate-y-1/2 text-caption text-ink-3">Pleasant</span>
              <motion.span
                className="absolute -translate-x-1/2 -translate-y-1/2 rounded-full"
                style={{
                  left,
                  top,
                  width: orbSize,
                  height: orbSize,
                  background: `radial-gradient(circle at 35% 30%, white 0%, ${color} 38%, color-mix(in oklab, ${color} 55%, black) 100%)`,
                  boxShadow: `0 0 30px ${color}, 0 0 80px color-mix(in oklab, ${color} 60%, transparent)`,
                }}
              />
            </div>
            <div className="mt-6 flex gap-3">
              <Button variant="secondary" size="lg" className="flex-1" onClick={save}>
                Save
              </Button>
              <Button variant="primary" size="lg" className="flex-1" onClick={() => setStep(1)}>
                Why?
              </Button>
            </div>
          </motion.div>
        ) : (
          <motion.div key="why" initial={{ opacity: 0, x: 20 }} animate={{ opacity: 1, x: 0 }} exit={{ opacity: 0, x: 20 }} transition={spring.page}>
            <div className="mt-2 flex flex-wrap gap-2">
              {MOOD_REASONS.map((r) => (
                <Chip key={r} active={why.includes(r)} tint={color} onClick={() => setWhy((w) => (w.includes(r) ? w.filter((x) => x !== r) : [...w, r]))}>
                  {r}
                </Chip>
              ))}
            </div>
            <textarea
              value={note}
              onChange={(e) => setNote(e.target.value)}
              placeholder={val < 0 ? 'What’s weighing on you?' : 'What made it good?'}
              rows={3}
              className="mt-4 w-full resize-none rounded-[18px] bg-fill-2 p-4 text-callout outline-none placeholder:text-ink-3"
            />
            <div className="mt-5 flex gap-3">
              <Button variant="secondary" size="lg" className="flex-1" onClick={() => setStep(0)}>
                Back
              </Button>
              <Button variant="primary" size="lg" className="flex-1" onClick={save}>
                Save
              </Button>
            </div>
          </motion.div>
        )}
      </AnimatePresence>
    </Sheet>
  );
}

