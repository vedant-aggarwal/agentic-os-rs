// The hero: his character on the centre pedestal, goal objects on pedestals either side, the
// cave behind. A turntable you can swipe; tap a goal at the front to open its page.
// The 2.5D art paints first. The three.js module (assets/hero3d) mounts after the data, and
// replaces the art only when his personal presets exist (otherwise it is a faceless stand-in).
import { animate, motion, useMotionValue, useTransform, type MotionValue } from 'motion/react';
import { useEffect, useMemo, useRef, useState } from 'react';
import { lookFor } from '@/lib/meta';
import type { Slot } from '@/lib/types';
import { haptic } from '@/lib/ui';
import { spring } from '@/ui/motion';
import { Hero3D } from './Hero3D';

export type HeroItem = 'me' | Slot;

export const ITEM_GLOW: Record<HeroItem, string> = {
  me: '#8f9bff',
  physical: '#ff5a4e',
  money: '#3fd0ff',
  mind: '#9b8cff',
};

interface Props {
  level: number;
  privateMode: boolean;
  /** 0..1, how clearly the companion shows */
  vivid: number;
  onGoal: (slot: Slot) => void;
  onSelf: () => void;
  onFront: (item: HeroItem) => void;
  height: number;
  ready: boolean; // data loaded: the 3D module may start
}

const img = (n: string) => `./img/${n}.webp`;

export function Hero({ level, privateMode, vivid, onGoal, onSelf, onFront, height, ready }: Props) {
  const items = useMemo<HeroItem[]>(() => (privateMode ? ['me', 'money', 'mind', 'physical'] : ['me', 'money', 'physical']), [privateMode]);
  const n = items.length;
  const spin = useMotionValue(0);
  const [front, setFront] = useState<HeroItem>('me');
  const ref = useRef<HTMLDivElement>(null);
  const [w, setW] = useState(390);
  const [three, setThree] = useState<'off' | 'loading' | 'on'>('off');

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setW(el.clientWidth));
    ro.observe(el);
    setW(el.clientWidth);
    return () => ro.disconnect();
  }, []);

  // keep the front item when the list changes (safe mode toggles)
  useEffect(() => {
    const i = items.indexOf(front);
    spin.set(i >= 0 ? i : 0);
    if (i < 0) setFront('me');
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [items]);

  useEffect(() => spin.on('change', (v) => {
    const idx = ((Math.round(v) % n) + n) % n;
    const it = items[idx];
    setFront((f) => (f === it ? f : it));
  }), [spin, n, items]);

  useEffect(() => onFront(front), [front, onFront]);

  const settle = (target: number) => {
    animate(spin, target, spring.hero);
  };

  const goTo = (i: number) => {
    const cur = spin.get();
    let d = i - (((Math.round(cur) % n) + n) % n);
    if (d > n / 2) d -= n;
    if (d < -n / 2) d += n;
    haptic('light');
    settle(Math.round(cur) + d);
  };

  // pointer: horizontal drag spins, a tap selects
  const g = useRef<{ x: number; y: number; s: number; t: number; drag: boolean; id: number } | null>(null);
  const onDown = (e: React.PointerEvent) => {
    g.current = { x: e.clientX, y: e.clientY, s: spin.get(), t: performance.now(), drag: false, id: e.pointerId };
  };
  const onMove = (e: React.PointerEvent) => {
    const s = g.current;
    if (!s) return;
    const dx = e.clientX - s.x;
    const dy = e.clientY - s.y;
    if (!s.drag && Math.abs(dx) > 8 && Math.abs(dx) > Math.abs(dy) * 1.2) {
      s.drag = true;
      (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    }
    if (s.drag) spin.set(s.s - dx / (w * 0.42));
  };
  const onUp = (e: React.PointerEvent) => {
    const s = g.current;
    g.current = null;
    if (!s) return;
    if (s.drag) {
      const dt = Math.max(16, performance.now() - s.t);
      const v = (e.clientX - s.x) / dt; // px per ms
      const proj = spin.get() - (v * 180) / (w * 0.42);
      const target = Math.round(proj);
      if (target !== Math.round(s.s)) haptic('light');
      settle(target);
    }
  };

  const tapItem = (i: number) => {
    if (g.current?.drag) return;
    const it = items[i];
    if (it === front) {
      haptic('medium');
      if (it === 'me') onSelf();
      else onGoal(it);
    } else goTo(i);
  };

  const H = height;
  const look = lookFor(level);

  return (
    <div
      ref={ref}
      className="relative w-full select-none"
      style={{ height: H, touchAction: 'pan-y' }}
      onPointerDown={onDown}
      onPointerMove={onMove}
      onPointerUp={onUp}
      onPointerCancel={() => (g.current = null)}
    >
      {/* the light from above, in the front item's colour */}
      <motion.div
        aria-hidden
        className="pointer-events-none absolute left-1/2 top-0 -translate-x-1/2"
        style={{
          width: w * 0.9,
          height: H * 0.95,
          background: `conic-gradient(from 180deg at 50% 0%, transparent 162deg, color-mix(in oklab, ${ITEM_GLOW[front]} 30%, transparent) 180deg, transparent 198deg)`,
          filter: 'blur(18px)',
          mixBlendMode: 'screen',
          transition: 'background 600ms',
        }}
      />
      {/* floor glow under the front pedestal */}
      <div
        aria-hidden
        className="pointer-events-none absolute left-1/2 -translate-x-1/2 rounded-[50%]"
        style={{
          bottom: H * 0.02,
          width: w * 0.8,
          height: H * 0.16,
          background: `radial-gradient(closest-side, color-mix(in oklab, ${ITEM_GLOW[front]} 34%, transparent), transparent)`,
          transition: 'background 600ms',
          animation: 'breathe 5s ease-in-out infinite',
        }}
      />

      <div className={`absolute inset-0 transition-opacity duration-700 ${three === 'on' ? 'opacity-0' : 'opacity-100'}`}>
        {items.map((it, i) => (
          <Slot
            key={it}
            item={it}
            index={i}
            n={n}
            spin={spin}
            w={w}
            h={H}
            look={look}
            vivid={vivid}
            showGhost={privateMode && it === 'me'}
            onTap={() => tapItem(i)}
          />
        ))}
      </div>

      {ready && (
        <Hero3D
          level={level}
          privateMode={privateMode}
          focused={front === 'me' ? null : front}
          onState={setThree}
          onGoal={(s) => onGoal(s)}
        />
      )}
    </div>
  );
}

function wrap(o: number, n: number) {
  o = ((o % n) + n) % n;
  return o > n / 2 ? o - n : o;
}

interface SlotProps {
  item: HeroItem;
  index: number;
  n: number;
  spin: MotionValue<number>;
  w: number;
  h: number;
  look: string;
  vivid: number;
  showGhost: boolean;
  onTap: () => void;
}

/** One pedestal with its object, placed on the turntable by its offset from the front. */
function Slot({ item, index, n, spin, w, h, look, vivid, showGhost, onTap }: SlotProps) {
  const off = useTransform(spin, (s) => wrap(index - s, n));
  // front (o = 0) -> sides (|o| = 1) -> behind (|o| >= 1.5)
  const x = useTransform(off, (o) => {
    const a = Math.min(Math.abs(o), 1.5);
    const side = a <= 1 ? a * 0.335 : 0.335 - (a - 1) * 0.67;
    return Math.sign(o) * side * w;
  });
  const y = useTransform(off, (o) => {
    const a = Math.min(Math.abs(o), 1.5);
    return -(a <= 1 ? a * 0.1 : 0.1 + (a - 1) * 0.08) * h;
  });
  const scale = useTransform(off, (o) => {
    const a = Math.min(Math.abs(o), 1.5);
    return a <= 1 ? 1 - a * 0.5 : 0.5 - (a - 1) * 0.2;
  });
  const opacity = useTransform(off, (o) => {
    const a = Math.abs(o);
    return a <= 1 ? 1 - a * 0.12 : Math.max(0, 0.88 - (a - 1) * 1.9);
  });
  const zIndex = useTransform(off, (o) => Math.round(10 - Math.abs(o) * 4));
  const bright = useTransform(off, (o) => `brightness(${1 - Math.min(1, Math.abs(o)) * 0.28}) saturate(${1 - Math.min(1, Math.abs(o)) * 0.15})`);
  const ghostOpacity = useTransform(off, (o) => Math.max(0, 1 - Math.abs(o) * 2.2) * (0.22 + vivid * 0.6));

  const pedW = w * 0.6;
  const pedH = pedW * (370 / 900);
  const top = pedH * 0.42; // where the art stands on the stone

  let art: React.ReactNode = null;
  if (item === 'me') {
    const fh = h * 0.61;
    art = <img src={img(`me-${look}`)} alt="Your character" draggable={false} className="hero-breathe absolute left-1/2 -translate-x-1/2" style={{ bottom: top - 4, height: fh, maxWidth: 'none' }} />;
  } else if (item === 'mind') {
    const fh = h * 0.61;
    art = (
      <img
        src={img('companion')}
        alt="Companion"
        draggable={false}
        className="hero-breathe absolute left-1/2 -translate-x-1/2"
        style={{ bottom: top - 4, height: fh, maxWidth: 'none', opacity: 0.35 + vivid * 0.65, filter: `saturate(${0.35 + vivid * 0.65}) drop-shadow(0 0 18px rgb(155 140 255 / ${0.5 - vivid * 0.3}))` }}
      />
    );
  } else if (item === 'physical') {
    // the bike and laptop art come with their own stone
    art = <img src={img('bike')} alt="Physical goal" draggable={false} className="absolute bottom-0 left-1/2 -translate-x-1/2" style={{ width: pedW * 1.04, maxWidth: 'none', filter: 'drop-shadow(0 18px 18px rgb(0 0 0 / 0.7))' }} />;
  } else {
    art = <img src={img('macbook')} alt="Money goal" draggable={false} className="absolute bottom-0 left-1/2 -translate-x-1/2" style={{ width: pedW * 1.0, maxWidth: 'none', filter: 'drop-shadow(0 18px 18px rgb(0 0 0 / 0.7))' }} />;
  }
  const ownStone = item === 'physical' || item === 'money';

  return (
    <motion.div
      className="absolute"
      style={{ left: '50%', bottom: h * 0.035, x, y, scale, opacity, zIndex, originY: 1, width: 0, height: 0 }}
    >
      <motion.button
        aria-label={item === 'me' ? 'Your level' : `${item} goal`}
        onClick={onTap}
        className="absolute bottom-0 block"
        style={{ left: -pedW / 2, width: pedW, height: h * 0.8, filter: bright }}
      >
        {showGhost && (
          <motion.img
            src={img('companion')}
            alt=""
            aria-hidden
            draggable={false}
            className="pointer-events-none absolute"
            style={{
              left: pedW * 0.5,
              bottom: top + 2,
              height: h * 0.6,
              maxWidth: 'none',
              opacity: ghostOpacity,
              filter: `saturate(${0.2 + vivid * 0.8}) blur(${(1 - vivid) * 0.6}px) drop-shadow(0 0 22px rgb(155 140 255 / 0.45))`,
            }}
          />
        )}
        {!ownStone && <img src={img('pedestal')} alt="" aria-hidden draggable={false} className="absolute bottom-0 left-0" style={{ width: pedW, height: pedH, filter: 'drop-shadow(0 18px 18px rgb(0 0 0 / 0.7))' }} />}
        {art}
      </motion.button>
    </motion.div>
  );
}
