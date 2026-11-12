// Name + XP/level pill (opens the roadmap), Nora with her state ring (opens Nora), profile.
import { motion } from 'motion/react';
import { UserRound } from 'lucide-react';
import { useOrb, useProfile, useToday } from '@/lib/queries';
import { haptic, useUI } from '@/lib/ui';
import { IconButton } from '@/ui/kit';
import { Ring } from '@/ui/Ring';

export function TopBar({ solid }: { solid: boolean }) {
  const { data: profile } = useProfile();
  const { data: today } = useToday();
  const { data: orb } = useOrb();
  const { open, push } = useUI();
  const lv = today?.xp.level;

  return (
    <header className="fixed inset-x-0 top-0 z-20 pt-safe md:left-[84px]">
      <div className={`veil-top absolute inset-0 -bottom-8 transition-opacity duration-300 ${solid ? 'opacity-100' : 'opacity-0'}`} />
      <div className="relative mx-auto flex h-[60px] max-w-[560px] items-center gap-2.5 px-gutter">
        <div className="flex min-w-0 flex-1 items-center gap-2.5">
          <h1 className="truncate text-[26px] leading-[30px] font-bold tracking-[-0.025em]" style={{ textShadow: '0 2px 18px rgb(0 0 0 / 0.5)' }}>
            {profile?.name || 'You'}
          </h1>
          <button
            onClick={() => {
              haptic('light');
              open({ kind: 'roadmap' });
            }}
            className="glass press flex h-9 shrink-0 items-center gap-1.5 rounded-full pr-3 pl-1.5"
            aria-label={`Level ${lv?.level ?? 1}, open the level roadmap`}
          >
            <Ring size={24} stroke={3} value={lv?.progress ?? 0} color="var(--color-xp)" color2="#ff9d3a">
              <span />
            </Ring>
            <span className="text-subhead font-semibold">Lv {lv?.level ?? 1}</span>
            <span className="tnum font-mono text-footnote text-ink-2">{(lv?.total_xp ?? 0).toLocaleString('en-IN')} XP</span>
          </button>
        </div>
        <button
          onClick={() => {
            haptic('light');
            push({ kind: 'nora' });
          }}
          aria-label={`Nora, ${orb?.label || 'ready'}`}
          className="press relative grid size-11 shrink-0 place-items-center rounded-full"
        >
          <motion.span
            aria-hidden
            className="absolute inset-[-3px] rounded-full"
            style={{ background: `conic-gradient(from 0deg, ${orb?.color || '#6daeff'}, #9b8cff, ${orb?.color || '#6daeff'})`, filter: 'blur(6px)', opacity: 0.55 }}
            animate={{ rotate: 360 }}
            transition={{ repeat: Infinity, duration: 9, ease: 'linear' }}
          />
          <span className="absolute inset-0 rounded-full p-[2px]" style={{ background: `conic-gradient(from 210deg, ${orb?.color || '#6daeff'}, #9b8cff 55%, ${orb?.color || '#6daeff'})` }}>
            <img src="./img/nora-round.webp" alt="" className="size-full rounded-full border-2 border-ground object-cover" />
          </span>
        </button>
        <IconButton icon={UserRound} label="Profile" onClick={() => open({ kind: 'profile' })} />
      </div>
    </header>
  );
}
