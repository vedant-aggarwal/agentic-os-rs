// Voice input for every field. Runs inside a Dioxus eval: `dioxus` is the channel to Rust.
//
// - A button with data-aos-mic="<target>" toggles recording. The click is handled here, inside the
//   real tap, so the microphone and the chimes are allowed on iOS and Safari.
// - MediaRecorder cuts a chunk every second. Each chunk goes to IndexedDB first (the device keeps
//   it), then to the server (PUT /api/voice/takes/{id}/parts/{p}/chunks/{n}). A crash, a closed app
//   or a dead network loses at most the last second. Every few minutes a new "part" starts, because
//   only the first chunk of a recorder carries the file header.
// - Stop sends finish: the server joins the chunks, Codex transcribes, the text comes back as a
//   {type:"text", target, text} message. Target "@focus" types into the last field you touched.
// - Three chimes: start (rising), stop (falling), alert (low double buzz) for an accidental stop.
// - Recover: takes that never finished stay in IndexedDB. list / recover / download / discard.
//
// Messages to Rust: {type:"state", recording, working:[]}, {type:"text", target, text, id},
// {type:"error", target, kind, message}, {type:"orphans", n}, {type:"recovered", n}.

const V = (window.__aosVoice = window.__aosVoice || {});
V.send = (m) => { try { dioxus.send(m); } catch (_) {} };
V.cfg = V.cfg || { url: "", token: "" };

if (!V.installed) {
  V.installed = true;
  const PART_MS = 240000; // a part is at most 4 minutes (Codex takes well over that; short parts retry cheaply)
  const MIMES = ["audio/webm;codecs=opus", "audio/mp4", "audio/webm", "audio/ogg;codecs=opus"];
  const working = new Set();
  let cur = null;
  let ctx = null;
  let lastField = null;

  // ----- storage: IndexedDB, with an in-memory fallback so a locked-down webview still records ----
  const mem = { takes: new Map(), chunks: new Map() };
  let dbp = null;
  const db = () => (dbp ||= new Promise((res) => {
    try {
      const r = indexedDB.open("aos-voice", 1);
      r.onupgradeneeded = () => {
        r.result.createObjectStore("takes", { keyPath: "id" });
        r.result.createObjectStore("chunks", { keyPath: ["id", "p", "n"] });
      };
      r.onsuccess = () => res(r.result);
      r.onerror = () => res(null);
    } catch (_) { res(null); }
  }));
  const req = (r) => new Promise((res, rej) => { r.onsuccess = () => res(r.result); r.onerror = () => rej(r.error); });
  const S = {
    async putTake(t) {
      mem.takes.set(t.id, t);
      const d = await db(); if (!d) return;
      try { await req(d.transaction("takes", "readwrite").objectStore("takes").put(t)); } catch (_) {}
    },
    async getTake(id) {
      const d = await db();
      if (!d) return mem.takes.get(id) || null;
      try { return (await req(d.transaction("takes").objectStore("takes").get(id))) || null; } catch (_) { return mem.takes.get(id) || null; }
    },
    async allTakes() {
      const d = await db();
      if (!d) return [...mem.takes.values()];
      try { return await req(d.transaction("takes").objectStore("takes").getAll()); } catch (_) { return [...mem.takes.values()]; }
    },
    async delTake(id) {
      mem.takes.delete(id);
      const d = await db(); if (!d) return;
      try { await req(d.transaction("takes", "readwrite").objectStore("takes").delete(id)); } catch (_) {}
    },
    async putChunk(c) {
      mem.chunks.set(`${c.id}/${c.p}/${c.n}`, c);
      const d = await db(); if (!d) return;
      try { await req(d.transaction("chunks", "readwrite").objectStore("chunks").put(c)); } catch (_) {}
    },
    async chunksOf(id) {
      const d = await db();
      if (!d) return [...mem.chunks.values()].filter((c) => c.id === id).sort((a, b) => a.p - b.p || a.n - b.n);
      try { return await req(d.transaction("chunks").objectStore("chunks").getAll(IDBKeyRange.bound([id, 0, 0], [id, 1e6, 1e9]))); } catch (_) { return []; }
    },
    async delChunks(id) {
      for (const k of [...mem.chunks.keys()]) if (k.startsWith(id + "/")) mem.chunks.delete(k);
      const d = await db(); if (!d) return;
      try { await req(d.transaction("chunks", "readwrite").objectStore("chunks").delete(IDBKeyRange.bound([id, 0, 0], [id, 1e6, 1e9]))); } catch (_) {}
    },
  };

  // ----- chimes ------------------------------------------------------------------------------
  const audio = () => {
    try {
      ctx ||= new (window.AudioContext || window.webkitAudioContext)();
      if (ctx.state === "suspended") ctx.resume();
    } catch (_) {}
    return ctx;
  };
  const tone = (c, f, at, dur, type, vol) => {
    const o = c.createOscillator(), g = c.createGain(), t = c.currentTime + at;
    o.type = type; o.frequency.value = f;
    g.gain.setValueAtTime(0.0001, t);
    g.gain.exponentialRampToValueAtTime(vol, t + 0.015);
    g.gain.exponentialRampToValueAtTime(0.0001, t + dur);
    o.connect(g).connect(c.destination);
    o.start(t); o.stop(t + dur + 0.05);
  };
  V.chime = (kind) => {
    const c = audio(); if (!c) return;
    if (kind === "start") { tone(c, 660, 0, 0.1, "sine", 0.18); tone(c, 880, 0.09, 0.14, "sine", 0.18); }
    else if (kind === "stop") { tone(c, 880, 0, 0.1, "sine", 0.18); tone(c, 587, 0.09, 0.16, "sine", 0.18); }
    else { tone(c, 196, 0, 0.16, "square", 0.12); tone(c, 196, 0.22, 0.16, "square", 0.12); tone(c, 147, 0.44, 0.32, "square", 0.12); }
  };

  // ----- helpers -----------------------------------------------------------------------------
  const emitState = () => V.send({ type: "state", recording: cur ? cur.target : null, working: [...working] });
  const fail = (target, kind, message) => V.send({ type: "error", target, kind, message });
  const auth = () => ({ Authorization: "Bearer " + V.cfg.token });
  const base = () => (V.cfg.url || "").replace(/\/+$/, "");
  const mics = () => document.querySelectorAll("[data-aos-mic]");
  const fmt = (s) => `${Math.floor(s / 60)}:${String(Math.floor(s % 60)).padStart(2, "0")}`;
  const newId = () => "v" + Date.now().toString(36) + Math.random().toString(36).slice(2, 8);
  const wait = (ms) => new Promise((r) => setTimeout(r, ms));

  // ----- upload ------------------------------------------------------------------------------
  const pumping = new Set();
  async function pump(id) {
    if (pumping.has(id) || !V.cfg.token || V.authBad) return false;
    pumping.add(id);
    try {
      const take = await S.getTake(id);
      if (!take) return false;
      const todo = (await S.chunksOf(id)).filter((c) => !c.up);
      for (const c of todo) {
        let r;
        try {
          r = await fetch(`${base()}/api/voice/takes/${id}/parts/${c.p}/chunks/${c.n}`, {
            method: "PUT", headers: { ...auth(), "Content-Type": take.mime }, body: c.blob,
          });
        } catch (_) { return false; } // offline: leave it, retry later
        if (r.status === 401) { V.authBad = true; return false; }
        if (!r.ok) return false;
        c.up = 1;
        await S.putChunk(c);
      }
      return true;
    } finally { pumping.delete(id); }
  }

  // Joins what the server has and gets the text. Returns {text} or {error, kind}.
  async function finishTake(id) {
    const take = await S.getTake(id);
    if (!take) return { error: "That recording is gone.", kind: "missing" };
    if (take.text != null && take.state === "done") return { text: take.text };
    if (!V.cfg.token) return { error: "Pair this device to transcribe. The recording is saved here.", kind: "offline" };
    for (let i = 0; i < 4; i++) {
      if (await pump(id)) break;
      if (V.authBad) return { error: "The server refused this device's token. Re-pair in You. The recording is saved here.", kind: "auth" };
      if (i === 3) return { error: "No connection. The recording is saved on this phone and uploads when you are back online.", kind: "offline" };
      await wait(800 * (i + 1));
    }
    const parts = Object.entries(take.parts).map(([p, n]) => ({ part: Number(p), chunks: n }));
    let r;
    try {
      r = await fetch(`${base()}/api/voice/takes/${id}/finish`, {
        method: "POST", headers: { ...auth(), "Content-Type": "application/json" }, body: JSON.stringify({ parts }),
      });
    } catch (_) { return { error: "No connection. The recording is saved and will be sent when you are back online.", kind: "offline" }; }
    let body = null; try { body = await r.json(); } catch (_) {}
    if (r.status === 401) { V.authBad = true; return { error: "The server refused this device's token. Re-pair in You.", kind: "auth" }; }
    if (r.status === 409) { // the server lost a chunk: send everything again next time
      for (const c of await S.chunksOf(id)) { c.up = 0; await S.putChunk(c); }
      return { error: "The server missed some audio. Tap Recover to send it again.", kind: "missing_chunks" };
    }
    if (!r.ok) return { error: body?.error?.message || `Transcription failed (${r.status}). The recording is saved.`, kind: body?.error?.code || "server" };
    take.state = "done"; take.text = body.text || ""; take.doneAt = Date.now();
    await S.putTake(take);
    await S.delChunks(id); // the text is kept in the list; the server keeps its copy of the audio
    return { text: take.text };
  }

  // ----- recording ---------------------------------------------------------------------------
  function level() {
    if (!cur) return;
    const a = cur.analyser; let v = 0;
    if (a) {
      const buf = new Uint8Array(a.fftSize); a.getByteTimeDomainData(buf);
      let s = 0; for (const x of buf) { const d = (x - 128) / 128; s += d * d; }
      v = Math.min(1, Math.sqrt(s / buf.length) * 5);
    }
    const t = fmt((Date.now() - cur.startedAt) / 1000);
    mics().forEach((b) => {
      const on = b.dataset.aosMic === cur.target;
      b.classList.toggle("aos-mic-on", on);
      if (on) { b.style.setProperty("--lv", v.toFixed(2)); b.dataset.t = t; }
    });
  }

  function startPart(p) {
    const c = cur; c.part = p; c.parts[p] = c.parts[p] || 0;
    const rec = new MediaRecorder(c.stream, { ...(c.mimeFull ? { mimeType: c.mimeFull } : {}), audioBitsPerSecond: 32000 });
    c.rec = rec;
    rec.ondataavailable = async (e) => {
      if (!e.data || !e.data.size) return;
      const n = c.parts[p]++;
      await S.putChunk({ id: c.id, p, n, blob: e.data, up: 0 });
      const take = await S.getTake(c.id);
      if (take) { take.parts[p] = c.parts[p]; take.bytes = (take.bytes || 0) + e.data.size; take.updated = Date.now(); await S.putTake(take); }
      pump(c.id);
    };
    rec.onerror = () => accident("The recorder failed.");
    rec.onstop = () => {
      if (c.rotating) { c.rotating = false; if (cur === c && !c.stopping) startPart(p + 1); return; }
      if (!c.stopping) accident("Recording was stopped by the system.");
    };
    rec.start(1000);
    clearTimeout(c.partTimer);
    c.partTimer = setTimeout(() => { if (cur === c && rec.state === "recording") { c.rotating = true; rec.stop(); } }, PART_MS);
  }

  async function start(target) {
    if (cur) return;
    audio();
    if (!navigator.mediaDevices || !window.MediaRecorder) {
      V.chime("alert"); fail(target, "unsupported", "This webview cannot record audio."); return;
    }
    let stream;
    try {
      stream = await navigator.mediaDevices.getUserMedia({ audio: { channelCount: 1, echoCancellation: true, noiseSuppression: true } });
    } catch (e) {
      V.chime("alert");
      fail(target, "permission", e && e.name === "NotAllowedError" ? "Microphone permission is off. Turn it on in Settings." : "No microphone found.");
      return;
    }
    const mime = MIMES.find((m) => MediaRecorder.isTypeSupported(m)) || "";
    const id = newId();
    const c = (cur = { id, target, mime: (mime || "audio/webm").split(";")[0], mimeFull: mime || undefined, stream, startedAt: Date.now(), parts: {}, part: 0, stopping: false });
    await S.putTake({ id, target, mime: c.mime, createdAt: Date.now(), updated: Date.now(), state: "recording", parts: { 0: 0 }, bytes: 0 });
    try {
      const ac = audio();
      if (ac) { const src = ac.createMediaStreamSource(stream); c.analyser = ac.createAnalyser(); c.analyser.fftSize = 512; src.connect(c.analyser); c.src = src; }
    } catch (_) {}
    stream.getAudioTracks().forEach((t) => { t.onended = () => { if (cur === c && !c.stopping) accident("The microphone was disconnected."); }; });
    V.chime("start");
    c.meter = setInterval(level, 120);
    startPart(0);
    emitState();
  }

  // Ends the recording: stop the recorder, keep the audio, send it for transcription.
  async function end(kind, message) {
    const c = cur; if (!c) return;
    c.stopping = true;
    clearTimeout(c.partTimer); clearInterval(c.meter);
    V.chime(kind === "accident" ? "alert" : "stop");
    try { if (c.rec && c.rec.state !== "inactive") c.rec.stop(); } catch (_) {}
    await wait(250); // the last chunk arrives just before the recorder reports stopped
    try { c.stream.getTracks().forEach((t) => t.stop()); } catch (_) {}
    try { c.src && c.src.disconnect(); } catch (_) {}
    mics().forEach((b) => { b.classList.remove("aos-mic-on"); b.style.removeProperty("--lv"); delete b.dataset.t; });
    cur = null;
    const take = await S.getTake(c.id);
    if (take) { take.state = "stopped"; take.parts = { ...take.parts, ...c.parts }; take.seconds = Math.round((Date.now() - c.startedAt) / 1000); await S.putTake(take); }
    working.add(c.target); emitState();
    if (kind === "accident") fail(c.target, "accident", message + " What was recorded is saved.");
    const r = await finishTake(c.id);
    working.delete(c.target); emitState();
    if (r.error) { fail(c.target, r.kind, r.error); if (kind !== "accident") V.chime("alert"); return; }
    deliver(c.target, r.text, c.id);
  }
  const accident = (m) => end("accident", m);

  function deliver(target, text, id) {
    if (!text) { fail(target, "empty", "Heard nothing. The audio is kept in Recover recordings."); return; }
    if (target === "@focus") insertInto(lastField, text);
    V.send({ type: "text", target, text, id });
  }

  // ----- typing into the last field you touched (hotkey, and any field without a mic button) ---
  function insertInto(el, text) {
    try { navigator.clipboard && navigator.clipboard.writeText(text); } catch (_) {}
    if (!el || !document.contains(el)) return;
    if (el.isContentEditable) { el.focus(); document.execCommand("insertText", false, text); return; }
    const proto = el.tagName === "TEXTAREA" ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    const set = Object.getOwnPropertyDescriptor(proto, "value").set;
    const v = el.value, a = el.selectionStart ?? v.length, b = el.selectionEnd ?? v.length;
    const pre = v.slice(0, a), sp = pre && !/\s$/.test(pre) ? " " : "";
    set.call(el, pre + sp + text + v.slice(b));
    const at = (pre + sp + text).length;
    try { el.setSelectionRange(at, at); } catch (_) {}
    el.dispatchEvent(new Event("input", { bubbles: true }));
  }
  document.addEventListener("focusin", (e) => {
    const t = e.target;
    if (t && (t.tagName === "TEXTAREA" || (t.tagName === "INPUT" && /^(text|search|url|email|)$/.test(t.type)) || t.isContentEditable)) lastField = t;
  });

  // ----- the button --------------------------------------------------------------------------
  V.toggle = (target) => {
    audio();
    if (cur) { const same = cur.target === target; end("stop"); if (!same) { /* second tap on another field only stops */ } return; }
    start(target);
  };
  document.addEventListener("mousedown", (e) => { if (e.target.closest && e.target.closest("[data-aos-mic]")) e.preventDefault(); }, true);
  document.addEventListener("click", (e) => {
    const b = e.target.closest && e.target.closest("[data-aos-mic]");
    if (!b) return;
    e.preventDefault(); e.stopPropagation();
    V.toggle(b.dataset.aosMic);
  }, true);

  // The app was backgrounded or the phone locked: some webviews stop the recorder without telling us.
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState !== "visible" || !cur) return;
    if (cur.rec && cur.rec.state !== "recording") accident("The app was in the background.");
  });
  window.addEventListener("online", () => V.retry());

  // ----- recover -----------------------------------------------------------------------------
  V.config = (c) => { V.cfg = { ...V.cfg, ...c }; V.authBad = false; V.retry(); };
  V.retry = async () => {
    if (!V.cfg.token) return;
    let n = 0;
    for (const t of await S.allTakes()) {
      if (cur && cur.id === t.id) continue;
      if (t.state === "recording") { t.state = "interrupted"; await S.putTake(t); }
      if (t.state === "stopped" || t.state === "interrupted") {
        const r = await finishTake(t.id);
        if (r.text != null) { const u = await S.getTake(t.id); if (u) { u.unseen = true; await S.putTake(u); } n++; }
      }
    }
    if (n) V.send({ type: "recovered", n });
  };
  V.list = async () => (await S.allTakes()).sort((a, b) => b.createdAt - a.createdAt).slice(0, 60).map((t) => ({
    id: t.id, target: t.target, at: t.createdAt, state: t.state === "recording" && !(cur && cur.id === t.id) ? "interrupted" : t.state,
    text: t.text || "", seconds: t.seconds || 0, bytes: t.bytes || 0, unseen: !!t.unseen,
  }));
  V.recover = async (id) => {
    const t = await S.getTake(id); if (!t) return { error: "Gone." };
    if (t.state === "recording" || t.state === "interrupted") { t.state = "stopped"; await S.putTake(t); }
    working.add("@recover"); emitState();
    const r = await finishTake(id);
    working.delete("@recover"); emitState();
    if (r.text != null) { const u = await S.getTake(id); if (u) { u.unseen = false; await S.putTake(u); } }
    return r;
  };
  V.copy = async (id) => { const t = await S.getTake(id); if (t && t.text) { try { await navigator.clipboard.writeText(t.text); return true; } catch (_) {} } return false; };
  V.download = async (id) => {
    const t = await S.getTake(id); const cs = await S.chunksOf(id);
    if (!t || !cs.length) return false;
    // one file per part: each part has its own header
    const byPart = {}; cs.forEach((c) => (byPart[c.p] ||= []).push(c.blob));
    const ext = /mp4/.test(t.mime) ? "m4a" : /ogg/.test(t.mime) ? "ogg" : "webm";
    Object.entries(byPart).forEach(([p, bl], i) => setTimeout(() => {
      const a = document.createElement("a"); a.href = URL.createObjectURL(new Blob(bl, { type: t.mime }));
      a.download = `${id}-part${p}.${ext}`; document.body.appendChild(a); a.click(); a.remove();
    }, i * 300));
    return true;
  };
  V.discard = async (id) => { await S.delChunks(id); await S.delTake(id); return true; };
  V.status = async () => {
    try { const r = await fetch(`${base()}/api/voice/status`, { headers: auth() }); return r.ok ? await r.json() : { login: "unknown", message: `HTTP ${r.status}` }; }
    catch (_) { return { login: "unknown", message: "offline" }; }
  };

  // orphans from a crash or a closed app: mark them and tell the app
  (async () => {
    let n = 0;
    for (const t of await S.allTakes()) {
      if (t.state === "recording") { t.state = "interrupted"; await S.putTake(t); }
      if (t.state === "interrupted" || t.state === "stopped") n++;
    }
    if (n) V.send({ type: "orphans", n });
  })();
}

await new Promise(() => {});
