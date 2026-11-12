// UI state: lock, safe mode, the tab, pushed pages and the open sheet.
// The tab lives in the URL hash so a reload (or a Capacitor resume) lands on the same tab.
import { create } from 'zustand';
import { safeStore } from './session';
import type { PillarId, Slot, Task, InboxItem } from './types';

export type Tab = 'today' | 'inbox' | 'life' | 'work' | 'mission';
export const TABS: Tab[] = ['today', 'inbox', 'life', 'work', 'mission'];

export type Page =
  | { kind: 'goal'; slot: Slot }
  | { kind: 'pillar'; id: PillarId }
  | { kind: 'area'; id: string }
  | { kind: 'nora' };

export type Sheet =
  | { kind: 'mood' }
  | { kind: 'roadmap' }
  | { kind: 'profile' }
  | { kind: 'capture'; area?: string | null; parent?: string }
  | { kind: 'skip'; task: Task }
  | { kind: 'wave' }
  | { kind: 'print' }
  | { kind: 'plan' }
  | { kind: 'inbox'; item: InboxItem }
  | { kind: 'work-thread'; id: string }
  | { kind: 't3-thread'; id: string }
  | { kind: 'work-new'; project?: string };

function tabFromHash(): Tab {
  const h = location.hash.replace(/^#\/?/, '') as Tab;
  return TABS.includes(h) ? h : 'today';
}

const skipLock = new URLSearchParams(location.search).has('unlocked');

interface UI {
  unlocked: boolean;
  safe: boolean;
  tab: Tab;
  pages: Page[];
  sheet: Sheet | null;
  toast: { id: number; text: string; tone?: 'ok' | 'warn' } | null;
  unlock(safe: boolean): void;
  lock(): void;
  setSafe(v: boolean): void;
  setTab(t: Tab): void;
  push(p: Page): void;
  pop(): void;
  popAll(): void;
  open(s: Sheet): void;
  close(): void;
  say(text: string, tone?: 'ok' | 'warn'): void;
}

export const useUI = create<UI>((set, get) => ({
  unlocked: skipLock,
  safe: safeStore.get(),
  tab: tabFromHash(),
  pages: [],
  sheet: null,
  toast: null,
  unlock(safe) {
    safeStore.set(safe);
    set({ unlocked: true, safe });
  },
  lock() {
    set({ unlocked: false, pages: [], sheet: null });
  },
  setSafe(v) {
    safeStore.set(v);
    set({ safe: v });
  },
  setTab(t) {
    if (location.hash !== '#/' + t) history.replaceState(null, '', '#/' + t);
    set({ tab: t, pages: [] });
  },
  push(p) {
    set({ pages: [...get().pages, p], sheet: null });
  },
  pop() {
    set({ pages: get().pages.slice(0, -1) });
  },
  popAll() {
    set({ pages: [] });
  },
  open(s) {
    set({ sheet: s });
  },
  close() {
    set({ sheet: null });
  },
  say(text, tone) {
    const id = Date.now();
    set({ toast: { id, text, tone } });
    setTimeout(() => {
      if (get().toast?.id === id) set({ toast: null });
    }, 2600);
  },
}));

addEventListener('hashchange', () => useUI.setState({ tab: tabFromHash() }));

/** Light haptic tick where the platform has one (Android web; the native shell adds iOS). */
export function haptic(kind: 'light' | 'medium' | 'success' = 'light') {
  const w = window as unknown as { Capacitor?: { Plugins?: { Haptics?: { impact(o: { style: string }): void; notification(o: { type: string }): void } } } };
  const h = w.Capacitor?.Plugins?.Haptics;
  if (h) {
    if (kind === 'success') h.notification({ type: 'SUCCESS' });
    else h.impact({ style: kind === 'medium' ? 'MEDIUM' : 'LIGHT' });
    return;
  }
  try {
    navigator.vibrate?.(kind === 'success' ? [8, 40, 12] : kind === 'medium' ? 14 : 7);
  } catch {
    /* no vibration */
  }
}
