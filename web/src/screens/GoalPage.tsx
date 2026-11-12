// Goal page (step one of a goal tap): the object, price and what's earned, why it matters,
// the emotional side, specs. Then the pillar page (step two).
import { motion } from 'motion/react';
import { ArrowRight, Quote, Sparkles } from 'lucide-react';
import { useProfile, useXp } from '@/lib/queries';
import { PILLARS, SLOT_PILLAR, rupees } from '@/lib/meta';
import type { Slot } from '@/lib/types';
import { useUI } from '@/lib/ui';
import { Bar, Button } from '@/ui/kit';
import { Page } from '@/ui/Page';
import { rise, spring } from '@/ui/motion';

// The bike and laptop art carry their own stone; the companion stands on the shared pedestal.
const ART: Record<Slot, { src: string; w: string; bottom: string; stone: boolean }> = {
  physical: { src: './img/bike.webp', w: '84%', bottom: '5%', stone: false },
  money: { src: './img/macbook.webp', w: '80%', bottom: '6%', stone: false },
  mind: { src: './img/companion.webp', w: '38%', bottom: '12%', stone: true },
};

export function GoalPage({ slot }: { slot: Slot }) {
  const { data: profile } = useProfile();
  const { data: xp } = useXp();
  const { pop, push } = useUI();
  const s = profile?.hero.find((h) => h.slot === slot);
  const pid = SLOT_PILLAR[slot];
  const p = PILLARS[pid];
  const pillarXp = xp?.pillars.find((x) => x.id === pid);
  const art = ART[slot];
  const left = s?.price ? Math.max(0, s.price - s.earned) : null;

  return (
    <Page onBack={pop} title={s?.name} tone={p.glow} rise>
      <div className="relative h-[min(58vh,460px)] overflow-hidden">
        <div aria-hidden className="absolute inset-0" style={{ background: `radial-gradient(60% 55% at 50% 58%, color-mix(in oklab, ${p.glow} 30%, transparent), transparent 72%)` }} />
        <div aria-hidden className="absolute left-1/2 top-0 h-full w-[80%] -translate-x-1/2" style={{ background: `conic-gradient(from 180deg at 50% 0%, transparent 165deg, color-mix(in oklab, ${p.glow} 26%, transparent) 180deg, transparent 195deg)`, filter: 'blur(16px)' }} />
        {art.stone && (
        <motion.img
          src="./img/pedestal.webp"
          alt=""
          aria-hidden
          className="absolute bottom-[4%] left-1/2 w-[78%] max-w-[420px] -translate-x-1/2"
          initial={{ opacity: 0, y: 30 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ ...spring.page, delay: 0.05 }}
          style={{ filter: 'drop-shadow(0 24px 24px rgb(0 0 0 / 0.7))' }}
        />
        )}
        <motion.img
          src={art.src}
          alt={s?.name || ''}
          className="absolute left-1/2 max-w-[460px] -translate-x-1/2"
          style={{ width: art.w, bottom: art.bottom, maxHeight: '80%', objectFit: 'contain', filter: `drop-shadow(0 0 40px color-mix(in oklab, ${p.glow} 26%, transparent)) drop-shadow(0 24px 24px rgb(0 0 0 / 0.6))` }}
          initial={{ opacity: 0, scale: 0.86, y: 30 }}
          animate={{ opacity: 1, scale: 1, y: 0 }}
          transition={{ ...spring.pop, delay: 0.08 }}
        />
        <div aria-hidden className="absolute inset-x-0 bottom-0 h-24 bg-gradient-to-b from-transparent to-ground" />
      </div>

      <div className="relative -mt-4 px-gutter">
        <motion.p variants={rise} initial="hidden" animate="show" custom={1} className="eyebrow" style={{ color: p.ink }}>
          {p.label} goal
        </motion.p>
        <motion.h1 variants={rise} initial="hidden" animate="show" custom={2} className="mt-1 text-display font-bold">
          {s?.name || 'Your goal'}
        </motion.h1>
        {s?.variant && (
          <motion.p variants={rise} initial="hidden" animate="show" custom={3} className="mt-1 text-body text-ink-2">
            {s.variant}
          </motion.p>
        )}
      </div>

      <motion.div variants={rise} initial="hidden" animate="show" custom={4} className="stone mx-gutter mt-6 rounded-xl p-5">
        {s?.price ? (
          <>
            <div className="flex items-end justify-between">
              <div>
                <p className="eyebrow">Earned</p>
                <p className="fig mt-1 text-[40px] leading-[44px]" style={{ color: p.ink }}>
                  {rupees(s.earned)}
                </p>
              </div>
              <div className="text-right">
                <p className="eyebrow">Price</p>
                <p className="fig mt-1 text-title2 text-ink">{rupees(s.price)}</p>
              </div>
            </div>
            <Bar className="mt-4" value={s.progress} color={p.hue} height={6} />
            <p className="mt-3 text-subhead text-ink-2 tnum">
              {Math.round(s.progress * 100)}% there · {rupees(left)} to go
            </p>
            {s.price_note && <p className="mt-1 text-footnote text-ink-3">{s.price_note}</p>}
          </>
        ) : (
          <>
            <p className="eyebrow">Progress</p>
            <p className="fig mt-1 text-[40px] leading-[44px]" style={{ color: p.ink }}>
              {Math.round((s?.progress || 0) * 100)}%
            </p>
            <Bar className="mt-4" value={s?.progress || 0} color={p.hue} height={6} />
            <p className="mt-3 text-subhead text-ink-2">Grows with your level and the Mind areas.</p>
          </>
        )}
        {pillarXp && (
          <div className="hair-t mt-4 flex items-center justify-between pt-4 text-subhead">
            <span className="text-ink-2">{pillarXp.name} pillar</span>
            <span className="tnum">
              Lv {pillarXp.level} · {pillarXp.xp.toLocaleString('en-IN')} XP
            </span>
          </div>
        )}
      </motion.div>

      <motion.div variants={rise} initial="hidden" animate="show" custom={5} className="mx-gutter mt-4 rounded-xl p-5" style={{ background: `linear-gradient(160deg, color-mix(in oklab, ${p.glow} 14%, var(--color-stone)), var(--color-stone))`, boxShadow: 'var(--stone-edge)' }}>
        <Quote size={20} style={{ color: p.ink }} />
        <p className="mt-2 text-title3 leading-[28px] font-semibold">{s?.why || 'Why does this one matter to you?'}</p>
        <p className="mt-2 text-subhead text-ink-2">{s?.why ? 'Your words.' : 'Tell Nora on the next call; she keeps it here, in your words.'}</p>
      </motion.div>

      {s?.about && (
        <motion.div variants={rise} initial="hidden" animate="show" custom={6} className="stone mx-gutter mt-4 flex gap-3 rounded-xl p-5">
          <Sparkles size={20} className="mt-0.5 shrink-0" style={{ color: p.ink }} />
          <div>
            <p className="text-headline font-semibold">What it stands for</p>
            <p className="mt-1 text-subhead text-ink-2">{s.about}</p>
          </div>
        </motion.div>
      )}

      {!!s?.specs.length && (
        <motion.div variants={rise} initial="hidden" animate="show" custom={7} className="mt-8">
          <h2 className="mb-3 px-gutter text-title3 font-bold">{slot === 'mind' ? 'What she stands for' : 'Specs'}</h2>
          <div className="stone mx-gutter rounded-xl">
            {s.specs.map((line, i) => {
              const [k, ...v] = line.split(':');
              const val = v.join(':').trim();
              return (
                <div key={i} className={`flex items-start gap-4 px-4 py-3.5 ${i < s.specs.length - 1 ? 'hair' : ''}`}>
                  <span className="w-24 shrink-0 text-subhead text-ink-2">{val ? k : ''}</span>
                  <span className="min-w-0 flex-1 text-subhead">{val || k}</span>
                </div>
              );
            })}
          </div>
        </motion.div>
      )}

      <div className="mx-gutter mt-8">
        <Button variant="tint" tint={p.glow} size="lg" className="w-full" onClick={() => push({ kind: 'pillar', id: pid })}>
          Open {p.label} <ArrowRight size={18} />
        </Button>
      </div>
    </Page>
  );
}
