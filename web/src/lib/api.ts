// Typed client for the Rust server.
//   POST /api/tools/{name}  every tool (today, task_add, inbox_list, ...)
//   POST /api/sync/push     the offline outbox, idempotent by op_id
//   GET  /api/events        SSE change feed (?token= because EventSource can't set headers)
//   /api/nora/*, /api/work/* Nora's thread and the Work supervisor
// A 401 flags "re-pair" and keeps the token. A network failure flags offline. Neither logs out.
import { isDemo, useSession } from './session';
import { demoCall, demoGet } from './demo';
import type { Paired } from './types';

export class ApiError extends Error {
  constructor(public status: number, public code: string, message: string) {
    super(message);
  }
}
export class OfflineError extends Error {
  constructor() {
    super('Can’t reach your server');
  }
}

function base(): string {
  return useSession.getState().server.replace(/\/+$/, '');
}

export function authUrl(path: string): string {
  const { token } = useSession.getState();
  const sep = path.includes('?') ? '&' : '?';
  return `${base()}${path}${token ? `${sep}token=${encodeURIComponent(token)}` : ''}`;
}

async function request<T>(method: string, path: string, body?: unknown, timeoutMs = 20000): Promise<T> {
  const s = useSession.getState();
  const ctl = new AbortController();
  const timer = setTimeout(() => ctl.abort(), timeoutMs);
  let res: Response;
  try {
    res = await fetch(base() + path, {
      method,
      headers: {
        ...(body !== undefined ? { 'Content-Type': 'application/json' } : {}),
        ...(s.token ? { Authorization: `Bearer ${s.token}` } : {}),
      },
      body: body !== undefined ? JSON.stringify(body) : undefined,
      signal: ctl.signal,
    });
  } catch {
    s.flagOffline(true);
    throw new OfflineError();
  } finally {
    clearTimeout(timer);
  }
  s.flagOffline(false);
  const text = await res.text();
  let data: unknown = null;
  try {
    data = text ? JSON.parse(text) : null;
  } catch {
    data = text;
  }
  if (res.status === 401) {
    s.flagRepair(true);
    throw new ApiError(401, 'unauthorized', 'This device needs to be paired again');
  }
  if (!res.ok) {
    const err = (data as { error?: { code?: string; message?: string } | string })?.error;
    const code = typeof err === 'object' ? err?.code || 'error' : String(err || 'error');
    const msg = typeof err === 'object' ? err?.message || res.statusText : (data as { message?: string })?.message || res.statusText;
    throw new ApiError(res.status, code, msg);
  }
  s.flagRepair(false);
  return data as T;
}

export const api = {
  tool<T>(name: string, args: Record<string, unknown> = {}): Promise<T> {
    if (isDemo) return demoCall<T>(name, args);
    return request<T>('POST', `/api/tools/${name}`, args);
  },
  get<T>(path: string): Promise<T> {
    if (isDemo) return demoGet<T>(path);
    return request<T>('GET', path);
  },
  del<T>(path: string): Promise<T> {
    if (isDemo) return demoGet<T>(path);
    return request<T>('DELETE', path);
  },
  post<T>(path: string, body: unknown = {}, timeoutMs?: number): Promise<T> {
    if (isDemo) return demoGet<T>(path);
    return request<T>('POST', path, body, timeoutMs);
  },
  health(server: string): Promise<{ ok: boolean; version: string }> {
    return fetch(server.replace(/\/+$/, '') + '/api/health').then((r) => {
      if (!r.ok) throw new ApiError(r.status, 'health', r.statusText);
      return r.json();
    });
  },
  async claim(server: string, code: string, name: string): Promise<Paired> {
    const platform = /iPhone|iPad/.test(navigator.userAgent) ? 'ios' : /Android/.test(navigator.userAgent) ? 'android' : 'web';
    let res: Response;
    try {
      res = await fetch(server.replace(/\/+$/, '') + '/api/pair/claim', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ code, name, platform }),
      });
    } catch {
      throw new OfflineError();
    }
    const data = await res.json().catch(() => ({}));
    if (res.status === 429) throw new ApiError(429, 'rate_limited', 'Too many tries. Wait a minute and try again.');
    if (!res.ok) throw new ApiError(res.status, data?.error?.code || 'bad_code', data?.error?.message || 'That code is wrong, used or expired.');
    return data as Paired;
  },
};
