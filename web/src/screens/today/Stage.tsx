// The top half: the cave, the hero turntable and the stat line under it (Opal's SCORE row).
// The stat line follows what is at the front: your day for the character, the goal's
// earned-vs-price for a goal.
import { AnimatePresence, motion } from 'motion/react';
import { useCallback, useEffect, useState } from 'react';
import { ChevronRight } from 'lucide-react';
import { Hero, ITEM_GLOW, type HeroItem } from '@/hero/Hero';
import { useProfile, useToday } from '@/lib/queries';
import { PILLARS, SLOT_PILLAR, rupees } from '@/lib/meta';
import { useUI } from '@/lib/ui';
import { Bar } from '@/ui/kit';
import { DayLine } from './Cards';

export function Stage({ onAmbient }: { onAmbient: (c: string) => void }) {
  const { data: today } = useToday();
  const { data: profile } = useProfile();
  const { safe, push, open } = useUI();
  const [front, setFront] = useState<HeroItem>('me');
  const [h, setH] = useState(() => heroHeight());
  const lv = today?.xp.level;
  const privateMode = !safe && profile?.companion !== false;

  useEffect(() => {
    const r = () => setH(heroHeight());
    addEventListener('resize', r);
    return () => removeEventListener('resize', r);
  }, []);

  const onFront = useCallback(
    (it: HeroItem) => {
      setFront(it);
      onAmbient(ITEM_GLOW[it]);
    },
    [onAmbient],
  );

  const slot = front === 'me' ? null : profile?.hero.find((s) => s.slot === front);
  const pillar = front === 'me' ? null : PILLARS[SLOT_PILLAR[front]];

  return (
    <div className="relative">
      <Hero
        level={lv?.level ?? 1}
        privateMode={privateMode}
        vivid={Math.min(1, Math.max(0, ((lv?.level ?? 1) - 1) / 19))}
        onGoal={(s) => push({ kind: 'goal', slot: s })}
        onSelf={() => open({ kind: 'roadmap' })}
        onFront={onFront}
        height={h}
        ready={!!today}
      />

      <div className={`relative mx-gutter transition-[height] duration-300 ${front === 'me' || !slot ? 'h-9' : 'h-[84px]'}`}>
        <AnimatePresence mode="wait" initial={false}>
          {front === 'me' || !slot ? (
            <motion.div
              key="me"
              className="absolute inset-0 flex items-center"
              initial={{ opacity: 0, y: 6 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: -6 }}
              transition={{ duration: 0.22 }}
            >
              <div className="w-full">
                <DayLine />
              </div>
            </motion.div>
          ) : (
            <motion.button
              key={front}
              onClick={() => push({ kind: 'goal', slot: slot.slot })}
              className="absolute inset-0 flex flex-col justify-center text-left"
              initial={{ opacity: 0, y: 6 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: -6 }}
              transition={{ duration: 0.22 }}
            >
              <span className="flex items-center gap-1.5">
                <span className="eyebrow" style={{ color: pillar?.ink }}>
                  {pillar?.label}
                </span>
                <span className="flex-1" />
                <span className="flex items-center text-footnote text-ink-2">
                  Open <ChevronRight size={15} />
                </span>
              </span>
              <span className="mt-0.5 truncate text-title3 font-bold">{slot.name}</span>
              {slot.price ? (
                <>
                  <Bar className="mt-2.5" value={slot.progress} color={pillar?.hue || 'white'} />
                  <span className="mt-1.5 text-footnote text-ink-2 tnum">
                    {rupees(slot.earned)} earned of {rupees(slot.price)}
                  </span>
                </>
              ) : (
                <span className="mt-1 text-footnote text-ink-2">{slot.variant}</span>
              )}
            </motion.button>
          )}
        </AnimatePresence>
      </div>
    </div>
  );
}

function heroHeight() {
  const vh = window.innerHeight;
  const w = Math.min(window.innerWidth, 560);
  return Math.round(Math.max(360, Math.min(vh * 0.48, w * 1.14, 480)));
}
