// Work sheets: open a session (an Agentic OS thread you can reply to, or a T3 Code thread read
// only) and start a new session in a project. Data: /api/work/* (see crates/aos-work/src/http.rs).
import { useEffect, useMemo, useRef, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { Send, Square, Trash2 } from 'lucide-react';
import { api } from '@/lib/api';
import { ago } from '@/lib/meta';
import { qc, useWorkProjects } from '@/lib/queries';
import type { WorkItem, WorkThreadDetail, T3ThreadDetail } from '@/lib/types';
import { haptic, useUI } from '@/lib/ui';
import { Button, Chip } from '@/ui/kit';
import { Sheet } from '@/ui/Sheet';

const refreshLists = () => {
  void qc.invalidateQueries({ queryKey: ['work-threads'] });
  void qc.invalidateQueries({ queryKey: ['work-t3'] });
};

function Bubble({ role, text, at }: { role: string; text: string; at?: number }) {
  const me = role === 'user';
  return (
    <div className={`flex ${me ? 'justify-end' : 'justify-start'}`}>
      <div className={`max-w-[88%] whitespace-pre-wrap break-words rounded-[18px] px-3.5 py-2.5 text-subhead ${me ? 'bg-fill-3 text-ink' : 'bg-fill-1 text-ink'}`}>
        {text}
        {at ? <div className="mt-1 text-caption text-ink-3">{ago(at)}</div> : null}
      </div>
    </div>
  );
}

/** An Agentic OS session: transcript, reply box, stop and delete. */
export function WorkThreadSheet({ id }: { id: string }) {
  const { close, say } = useUI();
  const { data, refetch } = useQuery<WorkThreadDetail>({
    queryKey: ['work-thread', id],
    queryFn: () => api.get(`/api/work/threads/${id}`),
    refetchInterval: (q) => (['starting', 'working'].includes(String(q.state.data?.thread.status)) ? 2000 : 15000),
  });
  const [text, setText] = useState('');
  const end = useRef<HTMLDivElement>(null);
  const items = useMemo(() => (data?.items || []).filter((i: WorkItem) => (i.kind === 'user_message' || i.kind === 'assistant_message') && i.text.trim()), [data]);
  useEffect(() => {
    // scrollIntoView returns a Promise in newer engines: never hand it back to React
    void end.current?.scrollIntoView({ block: 'end' });
  }, [items.length]);
  const t = data?.thread;
  const busy = t?.status === 'starting' || t?.status === 'working';

  const send = async () => {
    const msg = text.trim();
    if (!msg) return;
    setText('');
    haptic('light');
    try {
      await api.post(`/api/work/threads/${id}/messages`, { text: msg });
      void refetch();
      refreshLists();
    } catch (e) {
      say((e as Error).message, 'warn');
      setText(msg);
    }
  };
  const stop = async () => {
    await api.post(`/api/work/threads/${id}/interrupt`).catch(() => {});
    void refetch();
  };
  const decide = async (approval: string, decision: 'approve' | 'deny') => {
    haptic('medium');
    await api.post(`/api/work/approvals/${approval}`, { decision }).catch((e) => say((e as Error).message, 'warn'));
    void refetch();
  };
  const remove = async () => {
    try {
      await api.del(`/api/work/threads/${id}`);
      refreshLists();
      say('Session deleted', 'ok');
      close();
    } catch (e) {
      say((e as Error).message, 'warn');
    }
  };

  return (
    <Sheet onClose={close} size="full" title={t?.title || 'Session'} subtitle={t ? `${data?.project?.name || ''} · ${t.provider} · ${t.status.replace('_', ' ')}` : 'Loading…'}>
      <div className="mt-2 flex flex-col gap-2 pb-2">
        {items.map((i) => (
          <Bubble key={i.id} role={i.kind === 'user_message' ? 'user' : 'assistant'} text={i.text} />
        ))}
        {!items.length && data && <p className="py-6 text-center text-subhead text-ink-2">No messages yet.</p>}
        {(data?.approvals || [])
          .filter((a) => a.state === 'pending')
          .map((a) => (
            <div key={a.id} className="rounded-xl bg-fill-2 p-3">
              <p className="text-subhead font-semibold">{a.title}</p>
              {a.detail && <pre className="mt-1 max-h-40 overflow-auto whitespace-pre-wrap text-footnote text-ink-2">{a.detail}</pre>}
              <div className="mt-2 flex gap-2">
                <Button variant="primary" onClick={() => decide(a.id, 'approve')}>Approve</Button>
                <Button onClick={() => decide(a.id, 'deny')}>Deny</Button>
              </div>
            </div>
          ))}
        {t?.last_error && <p className="text-footnote text-ink-2">{t.last_error}</p>}
        <div ref={end} />
      </div>
      <div className="sticky bottom-0 -mx-5 flex items-end gap-2 bg-[var(--sheet-bg,transparent)] px-5 pb-1 pt-2 backdrop-blur">
        <textarea
          value={text}
          onChange={(e) => setText(e.target.value)}
          rows={1}
          placeholder="Reply"
          className="min-h-11 flex-1 resize-none rounded-[18px] bg-fill-2 px-4 py-2.5 text-body outline-none placeholder:text-ink-3"
        />
        {busy ? (
          <Button aria-label="Stop" icon={Square} onClick={stop} />
        ) : (
          <Button aria-label="Send" variant="primary" icon={Send} disabled={!text.trim()} onClick={send} />
        )}
      </div>
      <button className="press mt-3 flex items-center gap-1.5 text-footnote text-ink-3" onClick={remove}>
        <Trash2 size={14} /> Delete this session
      </button>
    </Sheet>
  );
}

/** A thread from T3 Code: read only, the last 200 messages. */
export function T3ThreadSheet({ id }: { id: string }) {
  const close = useUI((s) => s.close);
  const { data: projects } = useWorkProjects();
  const { data, error } = useQuery<T3ThreadDetail>({ queryKey: ['work-t3-thread', id], queryFn: () => api.get(`/api/work/t3/threads/${id}`), refetchInterval: 20000 });
  const end = useRef<HTMLDivElement>(null);
  useEffect(() => {
    // scrollIntoView returns a Promise in newer engines: never hand it back to React
    void end.current?.scrollIntoView({ block: 'end' });
  }, [data?.messages.length]);
  const project = projects?.find((p) => p.id === data?.project_id)?.name;
  return (
    <Sheet onClose={close} size="full" title={data?.title || 'T3 Code thread'} subtitle={data ? `T3 Code · ${project || 'No project'} · ${data.status || 'idle'} · read only` : 'Loading…'}>
      <div className="mt-2 flex flex-col gap-2 pb-4">
        {error && <p className="text-subhead text-ink-2">{(error as Error).message}</p>}
        {data?.messages.map((m, i) => <Bubble key={i} role={m.role} text={m.text} at={i === data.messages.length - 1 ? m.created_at : undefined} />)}
        {data && !data.messages.length && <p className="py-6 text-center text-subhead text-ink-2">No messages.</p>}
        <div ref={end} />
      </div>
    </Sheet>
  );
}

const PROVIDERS = [
  { id: 'claude', label: 'Claude' },
  { id: 'codex', label: 'Codex' },
];
const MODES = [
  { id: 'supervised', label: 'Ask first' },
  { id: 'auto_edit', label: 'Auto-edit' },
  { id: 'full_access', label: 'Full access' },
];

/** Start a session in a project on the PC. */
export function WorkNewSheet({ project: initial }: { project?: string }) {
  const { close, say, open } = useUI();
  const { data: projects } = useWorkProjects();
  const [project, setProject] = useState(initial || '');
  const [provider, setProvider] = useState('claude');
  const [mode, setMode] = useState('supervised');
  const [prompt, setPrompt] = useState('');
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    if (!project && projects?.length) setProject(projects[0].id);
  }, [projects, project]);

  const start = async () => {
    if (!project || !prompt.trim()) return;
    setBusy(true);
    try {
      const t = await api.post<{ id: string }>('/api/work/threads', { project, provider, runtime_mode: mode, prompt: prompt.trim() }, 30000);
      haptic('success');
      refreshLists();
      open({ kind: 'work-thread', id: t.id });
    } catch (e) {
      say((e as Error).message, 'warn');
    } finally {
      setBusy(false);
    }
  };

  return (
    <Sheet onClose={close} title="New session" subtitle="Runs on the PC in the project folder.">
      <p className="mt-2 text-footnote text-ink-2">Project</p>
      <div className="no-scrollbar -mx-5 mt-2 flex gap-2 overflow-x-auto px-5">
        {(projects || []).map((p) => (
          <Chip key={p.id} active={project === p.id} onClick={() => setProject(p.id)}>
            {p.name}
          </Chip>
        ))}
      </div>
      <div className="mt-4 flex flex-wrap gap-2">
        {PROVIDERS.map((p) => (
          <Chip key={p.id} active={provider === p.id} onClick={() => setProvider(p.id)}>
            {p.label}
          </Chip>
        ))}
        <span className="w-2" />
        {MODES.map((m) => (
          <Chip key={m.id} active={mode === m.id} onClick={() => setMode(m.id)}>
            {m.label}
          </Chip>
        ))}
      </div>
      <textarea
        value={prompt}
        onChange={(e) => setPrompt(e.target.value)}
        rows={3}
        placeholder="What should it do?"
        className="mt-4 w-full resize-none rounded-[18px] bg-fill-2 p-4 text-body outline-none placeholder:text-ink-3"
      />
      <Button variant="primary" size="lg" className="mt-5 w-full" disabled={!project || !prompt.trim() || busy} onClick={start}>
        {busy ? 'Starting…' : 'Start'}
      </Button>
    </Sheet>
  );
}
