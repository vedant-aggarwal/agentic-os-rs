// Call state. The WebRTC session lives in engine.js (window.aosCall), created once by the app; the
// call screen and the pill read this store. Server events (`call_state`, `call_transcript`,
// `call_handoff`, `orb_state`) come through the shared Nora stream. Ported from src/call/mod.rs.
import { create } from 'zustand';
import './engine.js';
import { noraStream } from '@/lib/live';
import { isDemo, useSession } from '@/lib/session';
import { qc } from '@/lib/queries';
import type { OrbState } from '@/lib/types';

export type Phase = 'idle' | 'connecting' | 'live' | 'ended' | 'failed';

interface CallState {
  phase: Phase;
  callId: string | null;
  startedAt: number;
  kind: string;
  muted: boolean;
  speaker: boolean;
  who: 'none' | 'you' | 'nora';
  thinking: number;
  status: string;
  caption: string;
  captionDone: boolean;
  heard: string;
  heardDone: boolean;
  summary: string;
  actions: string[];
  writeup: string;
  error: string | null;
  /** full screen (true) or the pill (false) */
  open: boolean;
  chat: boolean;
}

const blank: CallState = {
  phase: 'idle', callId: null, startedAt: 0, kind: 'quick', muted: false, speaker: true, who: 'none', thinking: 0, status: '',
  caption: '', captionDone: false, heard: '', heardDone: false, summary: '', actions: [], writeup: 'none', error: null, open: false, chat: false,
};

export const useCall = create<CallState>(() => ({ ...blank }));

/** The orb's state word, which also drives the ring on her portrait. */
export function mood(c: CallState): 'connecting' | 'muted' | 'thinking' | 'speaking' | 'listening' | 'idle' | 'ended' {
  if (c.phase === 'connecting') return 'connecting';
  if (c.phase !== 'live') return 'ended';
  if (c.muted) return 'muted';
  if (c.thinking > 0) return 'thinking';
  if (c.who === 'nora') return 'speaking';
  if (c.who === 'you') return 'listening';
  return 'idle';
}

export const onCall = () => {
  const p = useCall.getState().phase;
  return p === 'live' || p === 'connecting';
};

/* ═══ actions ═══ */

let demoTimer: ReturnType<typeof setTimeout>[] = [];

export function startCall(reason?: string) {
  const c = useCall.getState();
  if (c.phase === 'live' || c.phase === 'connecting') {
    useCall.setState({ open: true });
    return;
  }
  useCall.setState({ ...blank, phase: 'connecting', open: true });
  if (isDemo) return demoCall();
  const { server, token } = useSession.getState();
  void window.aosCall?.start({ base: server, token: token || '', reason: reason || null });
}

export function endCall() {
  if (isDemo) {
    demoTimer.forEach(clearTimeout);
    useCall.setState({ phase: 'ended', who: 'none', thinking: 0, status: 'Call ended', open: true, chat: false, writeup: 'done', summary: 'Moved revision to Saturday 10:00 and set a 17:25 reminder for the client brief.', actions: ['Saturday 10:00: revise chapter 4', '17:25 reminder: reply to the brief'] });
    return;
  }
  void window.aosCall?.end();
}

export function setMuted(m: boolean) {
  if (isDemo) return useCall.setState({ muted: m });
  window.aosCall?.setMuted(m);
}

export function toggleSpeaker() {
  if (isDemo) return useCall.setState((s) => ({ speaker: !s.speaker }));
  void window.aosCall?.toggleSpeaker();
}

export function closeWrap() {
  useCall.setState({ ...blank });
}

/* ═══ engine events ═══ */

addEventListener('aos-call', (e) => {
  const ev = (e as CustomEvent).detail as Record<string, unknown>;
  const set = useCall.setState;
  switch (ev.t) {
    case 'hello':
      if (ev.phase === 'live') set({ phase: 'live', callId: (ev.callId as string) || null, startedAt: Number(ev.startedAt) || Date.now() });
      break;
    case 'phase':
      if (ev.phase === 'connecting') set({ phase: 'connecting' });
      else if (ev.phase === 'live') set({ phase: 'live', callId: (ev.callId as string) || null, startedAt: Number(ev.startedAt) || Date.now(), kind: String(ev.kind || 'quick'), status: '' });
      else if (ev.phase === 'ended') set({ phase: 'ended', thinking: 0, who: 'none', status: 'Call ended', open: true, chat: false });
      else if (ev.phase === 'failed') set({ phase: 'failed', open: true, error: String(ev.error || 'The call didn’t connect.') });
      break;
    case 'who':
      set({ who: (ev.who as CallState['who']) || 'none' });
      break;
    case 'muted':
      set({ muted: !!ev.muted });
      break;
    case 'speaker':
      set({ speaker: ev.speaker !== false, status: ev.routed ? '' : 'This device has one speaker' });
      break;
  }
});

/* ═══ server events ═══ */

function toolStatus(title: string): string {
  const name = (title.split('__').pop() || title).trim().split(/\s+/)[0] || '';
  const known: [string, string][] = [
    ['alarm', 'Setting an alarm'], ['task_add', 'Adding a task'], ['task_done', 'Ticking a task'], ['task', 'Checking your tasks'],
    ['habit', 'Checking your habits'], ['journal', 'Writing your journal'], ['mood', 'Logging your mood'], ['remember', 'Saving that'],
    ['recall', 'Checking her memory'], ['today', 'Looking at today'], ['inbox', 'Checking your inbox'], ['calendar', 'Checking your calendar'],
  ];
  const hit = known.find(([k]) => name.includes(k));
  return hit ? hit[1] : name ? `Running ${name.replace(/_/g, ' ')}` : 'Working on it';
}

noraStream((raw) => {
  const ev = raw as Record<string, unknown> & { type: string };
  if (ev.type === 'orb_state') {
    qc.setQueryData<OrbState>(['orb'], ev.orb as OrbState);
    return;
  }
  const c = useCall.getState();
  const mine = (id: unknown) => !!c.callId && id === c.callId;
  if (ev.type === 'call_state') {
    const v = ev.call as Record<string, unknown>;
    if (!mine(v.id)) return;
    const patch: Partial<CallState> = {
      thinking: Number(v.thinking) || 0,
      writeup: String(v.writeup || 'none'),
      summary: String(v.summary || ''),
      actions: Array.isArray(v.actions) ? (v.actions as string[]) : [],
    };
    if (v.status === 'ended' && c.phase === 'live') {
      useCall.setState(patch);
      endCall();
      return;
    }
    if (!patch.thinking && c.phase === 'live') patch.status = '';
    useCall.setState(patch);
  } else if (ev.type === 'call_transcript') {
    if (!mine(ev.call_id)) return;
    const text = String(ev.text || '');
    const done = !!ev.done;
    const you = ev.who === 'you';
    const cur = you ? c.heard : c.caption;
    const finished = you ? c.heardDone : c.captionDone;
    let next = done ? text : (finished ? '' : cur) + text;
    if (next.length > 240) next = next.slice(-240);
    useCall.setState(you ? { heard: next, heardDone: done } : { caption: next, captionDone: done });
  } else if (ev.type === 'call_handoff') {
    if (!mine(ev.call_id)) return;
    const s = String(ev.status || '');
    useCall.setState({ status: s === 'running' ? 'Working on it' : s === 'tool' ? toolStatus(String(ev.text || '')) : s === 'failed' ? 'That didn’t work' : '' });
  }
});

/* ═══ demo: a scripted call so the screen can be seen without a mic ═══ */
function demoCall() {
  demoTimer.forEach(clearTimeout);
  const at = (ms: number, s: Partial<CallState>) => demoTimer.push(setTimeout(() => useCall.setState(s), ms));
  at(700, { phase: 'live', callId: 'demo', startedAt: Date.now() - 64000, who: 'nora', caption: 'Hey, I’m here. What do you need?', captionDone: true });
  at(2200, { who: 'you', heard: 'Move chapter four to Saturday and remind me about the brief', heardDone: true });
  at(3400, { who: 'none', thinking: 1, status: 'Checking your tasks' });
  at(6200, { thinking: 0, status: '', who: 'nora', caption: 'Done. Chapter four is on Saturday at ten, and I’ll ping you at 17:25 for the brief.', captionDone: true });
}
