// "+20 XP" rising from where you ticked. One layer for the whole app.
import { AnimatePresence, motion } from 'motion/react';
import { create } from 'zustand';

interface Float {
  id: number;
  x: number;
  y: number;
  text: string;
  color: string;
}

const useFloats = create<{ items: Float[] }>(() => ({ items: [] }));
let n = 0;

export function floatXp(el: Element | null, xp: number, color = 'var(--color-xp)') {
  if (!el || xp <= 0) return;
  const r = el.getBoundingClientRect();
  const f: Float = { id: ++n, x: r.left + r.width / 2, y: r.top, text: `+${xp} XP`, color };
  useFloats.setState((s) => ({ items: [...s.items, f] }));
  setTimeout(() => useFloats.setState((s) => ({ items: s.items.filter((i) => i.id !== f.id) })), 1100);
}

export function XpFloatLayer() {
  const items = useFloats((s) => s.items);
  return (
    <div className="pointer-events-none fixed inset-0 z-[70]">
      <AnimatePresence>
        {items.map((f) => (
          <motion.span
            key={f.id}
            className="fig absolute -translate-x-1/2 whitespace-nowrap text-[15px] font-bold"
            style={{ left: f.x, top: f.y, color: f.color, textShadow: `0 0 12px ${f.color}` }}
            initial={{ opacity: 0, y: 0, scale: 0.6 }}
            animate={{ opacity: [0, 1, 1, 0], y: -64, scale: [0.6, 1.12, 1, 0.96] }}
            transition={{ duration: 1, ease: [0.22, 1, 0.36, 1], times: [0, 0.2, 0.7, 1] }}
          >
            {f.text}
          </motion.span>
        ))}
      </AnimatePresence>
    </div>
  );
}
