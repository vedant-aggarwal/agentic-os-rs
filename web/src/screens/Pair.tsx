// Pairing: server URL + the 6-digit code from `aos pair new`. The reply's device token is kept
// on this device and sent as a Bearer token on every request.
import { motion } from 'motion/react';
import { useEffect, useRef, useState } from 'react';
import { Link2, Loader2, Server, Terminal } from 'lucide-react';
import { api, ApiError, OfflineError } from '@/lib/api';
import { pageOrigin, useSession } from '@/lib/session';
import { haptic } from '@/lib/ui';
import { Button } from '@/ui/kit';
import { spring } from '@/ui/motion';

function defaultName() {
  const ua = navigator.userAgent;
  if (/iPhone/.test(ua)) return 'iPhone';
  if (/iPad/.test(ua)) return 'iPad';
  if (/Android/.test(ua)) return 'Android phone';
  if (/Windows/.test(ua)) return 'Windows browser';
  return 'Browser';
}

export function Pair({ repair }: { repair?: boolean }) {
  const s = useSession();
  const [server, setServer] = useState(s.server || pageOrigin() || 'https://');
  const [editServer, setEditServer] = useState(!s.server && !pageOrigin());
  const [code, setCode] = useState('');
  const [name, setName] = useState(defaultName());
  const [health, setHealth] = useState<'checking' | 'ok' | 'down'>('checking');
  const [version, setVersion] = useState('');
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState('');
  const codeRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    let live = true;
    setHealth('checking');
    const t = setTimeout(() => {
      api
        .health(server)
        .then((h) => {
          if (!live) return;
          setHealth('ok');
          setVersion(h.version);
        })
        .catch(() => live && setHealth('down'));
    }, 300);
    return () => {
      live = false;
      clearTimeout(t);
    };
  }, [server]);

  const submit = async () => {
    if (code.length !== 6 || busy) return;
    setBusy(true);
    setErr('');
    try {
      const p = await api.claim(server, code, name.trim() || defaultName());
      haptic('success');
      s.setPaired(server.replace(/\/+$/, ''), p.token, p.device.id);
    } catch (e) {
      haptic('medium');
      setErr(e instanceof OfflineError ? 'Can’t reach that server. Check the URL and that the PC is on.' : e instanceof ApiError ? e.message : 'Something went wrong. Try again.');
      setCode('');
      codeRef.current?.focus();
    } finally {
      setBusy(false);
    }
  };

  useEffect(() => {
    if (code.length === 6) void submit();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [code]);

  return (
    <div className="fixed inset-0 z-[55] overflow-y-auto bg-ground">
      <div aria-hidden className="pointer-events-none absolute inset-x-0 top-0 h-[460px]" style={{ background: 'radial-gradient(80% 60% at 50% 0%, rgb(63 208 255 / 0.16), transparent 70%), radial-gradient(60% 50% at 80% 10%, rgb(155 140 255 / 0.14), transparent 70%)' }} />
      <div className="relative mx-auto flex min-h-full max-w-[440px] flex-col px-6 pt-safe pb-[calc(var(--safe-b)+24px)]">
        <motion.div initial={{ opacity: 0, y: 12 }} animate={{ opacity: 1, y: 0 }} transition={spring.page} className="mt-[8vh]">
          <span className="glass grid size-14 place-items-center rounded-[18px]">
            <Link2 size={26} strokeWidth={2} className="text-money" />
          </span>
          <h1 className="mt-6 text-display font-bold">{repair ? 'Pair this device again' : 'Connect to your server'}</h1>
          <p className="mt-2 text-body text-ink-2">
            {repair ? 'The server stopped accepting this device’s key. Nothing on the phone was deleted.' : 'One code links this device to the Agentic OS server on your PC.'}
          </p>
        </motion.div>

        <div className="stone mt-8 rounded-xl">
          <div className="flex min-h-[56px] items-center gap-3 px-4">
            <Server size={18} className="shrink-0 text-ink-2" />
            {editServer ? (
              <input
                value={server}
                onChange={(e) => setServer(e.target.value.trim())}
                inputMode="url"
                autoCapitalize="off"
                autoCorrect="off"
                spellCheck={false}
                placeholder="https://your-server"
                className="h-[56px] min-w-0 flex-1 bg-transparent text-body outline-none placeholder:text-ink-3"
              />
            ) : (
              <span className="min-w-0 flex-1 truncate text-body">{server.replace(/^https?:\/\//, '')}</span>
            )}
            <span className="flex items-center gap-1.5 text-footnote text-ink-2">
              <span className={`size-2 rounded-full ${health === 'ok' ? 'bg-[#34d27b]' : health === 'down' ? 'bg-danger' : 'bg-ink-3'}`} style={health === 'ok' ? { boxShadow: '0 0 8px #34d27b' } : undefined} />
              {health === 'ok' ? `Online${version ? ' · ' + version : ''}` : health === 'down' ? 'Not reachable' : 'Checking'}
            </span>
            {!editServer && (
              <button className="press -mr-2 h-11 px-2 text-subhead font-medium text-money-ink" onClick={() => setEditServer(true)}>
                Change
              </button>
            )}
          </div>
        </div>

        <label className="mt-8 block text-footnote font-semibold tracking-wide text-ink-2 uppercase">Pairing code</label>
        <div className="relative mt-3" onClick={() => codeRef.current?.focus()}>
          <input
            ref={codeRef}
            value={code}
            onChange={(e) => setCode(e.target.value.replace(/\D/g, '').slice(0, 6))}
            inputMode="numeric"
            autoComplete="one-time-code"
            aria-label="Six digit pairing code"
            autoFocus
            className="absolute inset-0 h-full w-full opacity-0"
          />
          <div className="grid grid-cols-6 gap-2" aria-hidden>
            {Array.from({ length: 6 }, (_, i) => {
              const ch = code[i];
              const focus = i === code.length;
              return (
                <motion.div
                  key={i}
                  className="stone fig grid h-[60px] place-items-center rounded-[14px] text-[28px]"
                  animate={{ scale: ch ? [1.08, 1] : 1, boxShadow: focus ? 'inset 0 0 0 1.5px rgb(63 208 255 / 0.8), 0 0 18px rgb(63 208 255 / 0.2)' : 'inset 0 1px 0 rgb(255 255 255 / 0.07)' }}
                  transition={{ duration: 0.2 }}
                >
                  {ch || ''}
                </motion.div>
              );
            })}
          </div>
        </div>
        {err && <p className="mt-3 text-subhead text-danger">{err}</p>}

        <div className="stone mt-6 flex min-h-[56px] items-center gap-3 rounded-xl px-4">
          <span className="text-body text-ink-2">Name</span>
          <input value={name} onChange={(e) => setName(e.target.value)} className="h-[56px] min-w-0 flex-1 bg-transparent text-right text-body outline-none" />
        </div>

        <div className="mt-6 flex items-start gap-3 rounded-xl bg-fill-1 p-4">
          <Terminal size={18} className="mt-0.5 shrink-0 text-ink-2" />
          <p className="text-footnote text-ink-2">
            On the PC, make a code with <span className="font-mono text-ink">aos pair new</span>. It works once and lasts an hour.
          </p>
        </div>

        <div className="flex-1" />
        <Button variant="primary" size="lg" className="mt-8 w-full" disabled={code.length !== 6 || busy} onClick={submit}>
          {busy ? <Loader2 size={20} className="animate-spin" /> : 'Connect'}
        </Button>
        {repair && (
          <button className="press mt-2 h-11 text-subhead text-ink-2" onClick={() => s.flagRepair(false)}>
            Not now
          </button>
        )}
      </div>
    </div>
  );
}
