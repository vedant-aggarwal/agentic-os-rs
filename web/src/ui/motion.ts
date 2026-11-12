// Motion presets. Springs wherever a finger can interrupt; values follow the old app's table
// (response / damping) converted to Motion's visualDuration + bounce.
import type { Transition } from 'motion/react';

export const spring = {
  /** pages and sheets: 0.42 s, no overshoot */
  page: { type: 'spring', visualDuration: 0.42, bounce: 0 } as Transition,
  /** sheets after a flick */
  sheet: { type: 'spring', visualDuration: 0.38, bounce: 0.04 } as Transition,
  /** small things that land: checks, chips, the tab lens */
  snappy: { type: 'spring', visualDuration: 0.3, bounce: 0.18 } as Transition,
  /** the rare rewarding moment: XP float, level ring */
  pop: { type: 'spring', visualDuration: 0.5, bounce: 0.32 } as Transition,
  /** hero turntable */
  hero: { type: 'spring', visualDuration: 0.55, bounce: 0.08 } as Transition,
  /** layout shifts in lists */
  layout: { type: 'spring', visualDuration: 0.36, bounce: 0.06 } as Transition,
};

export const ease = {
  settle: [0.22, 1, 0.36, 1] as const,
  move: [0.32, 0.72, 0, 1] as const,
};

/** List entrance: 30 ms stagger, 8 px rise. */
export const rise = {
  hidden: { opacity: 0, y: 8 },
  show: (i: number = 0) => ({ opacity: 1, y: 0, transition: { delay: Math.min(i, 8) * 0.03, duration: 0.32, ease: ease.settle } }),
};
