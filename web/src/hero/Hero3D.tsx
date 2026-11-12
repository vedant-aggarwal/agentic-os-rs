// React wrapper for the three.js hero module (assets/hero3d/hero3d.js, copied to ./hero3d/).
// API: mount(el, opts) / update({level, privateMode, focused}) / unmount(); DOM events
// hero3d:ready {stats}, hero3d:goal {goal}, hero3d:error. It loads after the day's data so the
// first paint never waits on WebGL. Without personal presets it reports `personal: false` and
// we unmount it again (the 2.5D art is the better stand-in). `?hero=3d` forces it on.
import { useEffect, useRef, useState } from 'react';
import type { Slot } from '@/lib/types';

interface Mod {
  mount(el: HTMLElement, opts: Record<string, unknown>): Promise<unknown>;
  update(o: Record<string, unknown>): void;
  unmount(): void;
}

const GOAL_OF: Record<Slot, string> = { physical: 'Physical', money: 'Money', mind: 'Mind' };
const SLOT_OF: Record<string, Slot> = { Physical: 'physical', Money: 'money', Mind: 'mind' };
const force = new URLSearchParams(location.search).get('hero');

export function Hero3D({
  level,
  privateMode,
  focused,
  onState,
  onGoal,
}: {
  level: number;
  privateMode: boolean;
  focused: Slot | null;
  onState: (s: 'off' | 'loading' | 'on') => void;
  onGoal: (s: Slot) => void;
}) {
  const el = useRef<HTMLDivElement>(null);
  const mod = useRef<Mod | null>(null);
  const live = useRef(false);
  const [on, setOn] = useState(false);
  const latest = useRef({ level, privateMode, focused });
  latest.current = { level, privateMode, focused };

  useEffect(() => {
    if (force === '2d' || !el.current) return;
    const node = el.current;
    let cancelled = false;
    const start = () => {
      onState('loading');
      const url = new URL('hero3d/hero3d.js', document.baseURI).href;
      import(/* @vite-ignore */ url)
        .then(async (m: Mod) => {
          if (cancelled) return;
          mod.current = m;
          const { level, privateMode, focused } = latest.current;
          await m.mount(node, {
            level,
            privateMode,
            focused: focused ? GOAL_OF[focused] : null,
            personalBase: new URL('personal/', document.baseURI).href,
            maxPixelRatio: 2,
          });
        })
        .catch(() => onState('off'));
    };
    const ready = (e: Event) => {
      const stats = (e as CustomEvent).detail?.stats as { personal?: boolean } | undefined;
      if (stats?.personal || force === '3d') {
        live.current = true;
        setOn(true);
        onState('on');
      } else {
        mod.current?.unmount();
        onState('off');
      }
    };
    const goal = (e: Event) => {
      const g = (e as CustomEvent).detail?.goal as string | undefined;
      if (g && SLOT_OF[g]) onGoal(SLOT_OF[g]);
    };
    const fail = () => onState('off');
    node.addEventListener('hero3d:ready', ready);
    node.addEventListener('hero3d:goal', goal);
    node.addEventListener('hero3d:error', fail);
    // after first paint and idle time, never in the way of the data
    const idle = (window as unknown as { requestIdleCallback?: (cb: () => void, o?: { timeout: number }) => number }).requestIdleCallback;
    const h = idle ? idle(start, { timeout: 1500 }) : window.setTimeout(start, 600);
    return () => {
      cancelled = true;
      if (!idle) clearTimeout(h);
      node.removeEventListener('hero3d:ready', ready);
      node.removeEventListener('hero3d:goal', goal);
      node.removeEventListener('hero3d:error', fail);
      mod.current?.unmount();
      live.current = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    if (live.current) mod.current?.update({ level, privateMode, focused: focused ? GOAL_OF[focused] : null });
  }, [level, privateMode, focused]);

  return <div ref={el} className="absolute inset-0" style={{ touchAction: 'pan-y', zIndex: on ? 20 : -1, pointerEvents: on ? 'auto' : 'none', opacity: on ? 1 : 0, transition: 'opacity 700ms' }} aria-hidden={!on} />;
}
