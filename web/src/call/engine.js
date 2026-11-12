// Nora's call session and orb, ported from the Dioxus build (src/js/call.js). Imported once by the
// app root, so the call outlives the call screen: the screen and the pill are two views of this
// session (design/specs/orb-and-call.md §1.2). Events go out as `aos-call` CustomEvents on window.
//
// window.aosCall: start({base, token, reason, ringId}), end(), setMuted(b), toggleSpeaker(),
//   mountOrb(canvas), unmountOrb(), setOrb({color, thinking, muted, stress}).
// window.AosOrb.mount(canvas) -> {set(o), level(v), destroy()}: WebGL2 points, motion in the shader.

(function () {
  const W = window;
  // The latest eval's channel: a re-mounted root takes over the events of a call already running.
  W.__aosCallSend = (o) => { try { W.dispatchEvent(new CustomEvent("aos-call", { detail: o })); } catch (_) {} };
  const emit = (o) => W.__aosCallSend(o);
  if (W.aosCall) { emit({ t: "hello", phase: W.aosCall.state.phase, callId: W.aosCall.state.callId, startedAt: W.aosCall.state.startedAt }); return; }

  // ---------------------------------------------------------------------------------------
  // Colour: blend in OKLab so a calm blue never passes through grey on its way to red.
  const lin = (c) => (c <= 0.04045 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4));
  const gam = (c) => (c <= 0.0031308 ? 12.92 * c : 1.055 * Math.pow(c, 1 / 2.4) - 0.055);
  function hexRgb(h) { const n = parseInt(String(h || "#6DAEFF").replace("#", ""), 16); return [(n >> 16 & 255) / 255, (n >> 8 & 255) / 255, (n & 255) / 255]; }
  function toLab([r, g, b]) {
    r = lin(r); g = lin(g); b = lin(b);
    const l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b);
    const m = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b);
    const s = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b);
    return [0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s, 1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s, 0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s];
  }
  function toRgb([L, a, b]) {
    const l = (L + 0.3963377774 * a + 0.2158037573 * b) ** 3, m = (L - 0.1055613458 * a - 0.0638541728 * b) ** 3, s = (L - 0.0894841775 * a - 1.291485548 * b) ** 3;
    const c = [4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s, -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s, -0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s];
    return c.map((v) => Math.min(1, Math.max(0, gam(v))));
  }

  // ---------------------------------------------------------------------------------------
  // The orb.
  const VS = `#version 300 es
  precision highp float;
  in vec3 aSphere; in vec3 aTri; in float aSeed;
  uniform float uTime, uLevel, uMorph, uRot, uAspect, uPx, uJitter, uFrozen;
  out float vBright; out float vSeed; out float vEdge;
  vec3 mod289(vec3 x){return x-floor(x*(1./289.))*289.;} vec4 mod289(vec4 x){return x-floor(x*(1./289.))*289.;}
  vec4 perm(vec4 x){return mod289(((x*34.)+1.)*x);} vec4 tis(vec4 r){return 1.79284291400159-0.85373472095314*r;}
  float snoise(vec3 v){const vec2 C=vec2(1./6.,1./3.);const vec4 D=vec4(0.,.5,1.,2.);
    vec3 i=floor(v+dot(v,C.yyy));vec3 x0=v-i+dot(i,C.xxx);vec3 g=step(x0.yzx,x0.xyz);vec3 l=1.-g;
    vec3 i1=min(g.xyz,l.zxy);vec3 i2=max(g.xyz,l.zxy);vec3 x1=x0-i1+C.xxx;vec3 x2=x0-i2+C.yyy;vec3 x3=x0-D.yyy;
    i=mod289(i);vec4 p=perm(perm(perm(i.z+vec4(0.,i1.z,i2.z,1.))+i.y+vec4(0.,i1.y,i2.y,1.))+i.x+vec4(0.,i1.x,i2.x,1.));
    float n_=.142857142857;vec3 ns=n_*D.wyz-D.xzx;vec4 j=p-49.*floor(p*ns.z*ns.z);vec4 x_=floor(j*ns.z);vec4 y_=floor(j-7.*x_);
    vec4 x=x_*ns.x+ns.yyyy;vec4 y=y_*ns.x+ns.yyyy;vec4 h=1.-abs(x)-abs(y);vec4 b0=vec4(x.xy,y.xy);vec4 b1=vec4(x.zw,y.zw);
    vec4 s0=floor(b0)*2.+1.;vec4 s1=floor(b1)*2.+1.;vec4 sh=-step(h,vec4(0.));vec4 a0=b0.xzyw+s0.xzyw*sh.xxyy;vec4 a1=b1.xzyw+s1.xzyw*sh.zzww;
    vec3 p0=vec3(a0.xy,h.x);vec3 p1=vec3(a0.zw,h.y);vec3 p2=vec3(a1.xy,h.z);vec3 p3=vec3(a1.zw,h.w);
    vec4 norm=tis(vec4(dot(p0,p0),dot(p1,p1),dot(p2,p2),dot(p3,p3)));p0*=norm.x;p1*=norm.y;p2*=norm.z;p3*=norm.w;
    vec4 m=max(.6-vec4(dot(x0,x0),dot(x1,x1),dot(x2,x2),dot(x3,x3)),0.);m=m*m;
    return 42.*dot(m*m,vec4(dot(p0,x0),dot(p1,x1),dot(p2,x2),dot(p3,x3)));}
  mat3 rotY(float a){float c=cos(a),s=sin(a);return mat3(c,0.,-s,0.,1.,0.,s,0.,c);}
  mat3 rotX(float a){float c=cos(a),s=sin(a);return mat3(1.,0.,0.,0.,c,s,0.,-s,c);}
  mat3 rotZ(float a){float c=cos(a),s=sin(a);return mat3(c,s,0.,-s,c,0.,0.,0.,1.);}
  void main(){
    float t = uTime * (1.0 - uFrozen);
    // Sphere: a shell that breathes, swells with the voice and ripples with low-frequency noise.
    float n = snoise(aSphere * 0.9 + vec3(0., 0., t * (0.2 + uJitter * 0.35)));
    float fine = snoise(aSphere * 3.1 + vec3(t * 0.4)) * (0.006 + uJitter * 0.012);
    float breathe = sin(uTime * 0.9) * 0.018;
    float r = 1.0 + breathe + uLevel * 0.2 + n * (0.035 + uLevel * 0.17) + fine;
    vec3 sph = rotX(0.38) * rotY(uRot) * (aSphere * r);
    // Triangle: the same particles stream along three nested, slowly turning triangles.
    vec3 tri = rotZ(uTime * 0.35) * aTri;
    tri.z += sin(aSeed * 40.0 + uTime * 2.0) * 0.03;
    vec3 p = mix(sph, tri, smoothstep(0.0, 1.0, uMorph));
    float z = p.z + 3.4;
    gl_Position = vec4(p.x * 2.35 / z / uAspect, p.y * 2.35 / z, 0.0, 1.0);
    float depth = clamp((1.6 - p.z) / 2.4, 0.0, 1.0);          // front points are brighter
    // A travelling wave along the triangle's perimeter (loader feel) while thinking.
    float wave = 0.55 + 0.45 * sin(aSeed * 6.2831 * 3.0 - uTime * 3.2);
    float spark = step(0.95, fract(aSeed * 57.3));                  // a few bright sparkles
    float b = 0.3 + 0.7 * pow(fract(aSeed * 91.7), 1.6) + spark * 0.8;
    vBright = mix(0.45 + 0.55 * (1.0 - depth), wave, uMorph) * b;
    vSeed = aSeed;
    vEdge = length(sph.xy);
    gl_PointSize = (1.1 + fract(aSeed * 13.1) * 2.0 + spark * 1.4 + uLevel * 1.2) * uPx * (3.4 / z);
  }`;
  const FS = `#version 300 es
  precision mediump float;
  in float vBright; in float vSeed; in float vEdge;
  uniform vec3 uColor; uniform float uIntensity, uDim;
  out vec4 o;
  void main(){
    vec2 d = gl_PointCoord - 0.5; float r = length(d);
    float a = smoothstep(0.5, 0.0, r);
    // Mostly white sparkles tinted by his colour, with a faint iridescent shift at the rim.
    vec3 iri = 0.5 + 0.5 * cos(6.2831 * (vec3(0.0, 0.33, 0.67) + vSeed + vEdge * 0.4));
    vec3 c = mix(uColor, vec3(1.0), 0.42 + 0.3 * fract(vSeed * 37.0));
    c = mix(c, iri, 0.12 * smoothstep(0.8, 1.15, vEdge));
    float k = a * vBright * uIntensity * (1.0 - 0.55 * uDim);
    vec3 g = mix(c, vec3(dot(c, vec3(0.3, 0.59, 0.11))), 0.6 * uDim);
    o = vec4(g * k, k);
  }`;

  // Random points in a thin shell (a lattice shows moire patterns at this density).
  function shellPoint() {
    const z = Math.random() * 2 - 1, ph = Math.random() * 2 * Math.PI, rad = Math.sqrt(1 - z * z), k = 1 + (Math.random() - 0.5) * 0.05;
    return [Math.cos(ph) * rad * k, z * k, Math.sin(ph) * rad * k];
  }
  function triPoint(i, n, seed) {
    // Three nested equilateral triangles; points spread along the edges by index.
    const ring = i % 3, scale = [1.05, 0.72, 0.4][ring];
    const u = ((i / 3) / (n / 3) + seed * 0.002) % 1, edge = Math.floor(u * 3), f = u * 3 - edge;
    const ang = (k) => -Math.PI / 2 + k * 2 * Math.PI / 3 + (ring === 1 ? Math.PI : 0);
    const a = ang(edge), b = ang(edge + 1);
    const j = (Math.random() - 0.5) * 0.05;
    return [(Math.cos(a) * (1 - f) + Math.cos(b) * f) * scale + j, -(Math.sin(a) * (1 - f) + Math.sin(b) * f) * scale + j, (Math.random() - 0.5) * 0.06];
  }

  const TIERS = [4000, 9000, 16000];  // starting ladder; adaptive quality moves between them

  function mountOrb(canvas) {
    const reduce = W.matchMedia && W.matchMedia("(prefers-reduced-motion: reduce)").matches;
    const gl = canvas.getContext("webgl2", { premultipliedAlpha: true, antialias: false, alpha: true });
    if (!gl) { canvas.dataset.fallback = "1"; return null; }
    let prog, buf, vao, count = 0, tier = W.innerWidth < 700 ? 1 : 2, raf = 0, dead = false;
    const U = {};
    const st = { level: 0, lvlTarget: 0, morph: 0, morphTarget: 0, rot: 0, color: toLab(hexRgb("#6DAEFF")), colorTarget: toLab(hexRgb("#6DAEFF")), dim: 0, dimTarget: 0, stress: 0, frozen: 0 };
    const sh = (type, src) => { const s = gl.createShader(type); gl.shaderSource(s, src); gl.compileShader(s); if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(s)); return s; };
    function build() {
      prog = gl.createProgram();
      gl.attachShader(prog, sh(gl.VERTEX_SHADER, VS)); gl.attachShader(prog, sh(gl.FRAGMENT_SHADER, FS));
      gl.linkProgram(prog);
      if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(prog));
      for (const k of ["uTime", "uLevel", "uMorph", "uRot", "uAspect", "uPx", "uJitter", "uFrozen", "uColor", "uIntensity", "uDim"]) U[k] = gl.getUniformLocation(prog, k);
      upload();
    }
    function upload() {
      count = TIERS[tier];
      const data = new Float32Array(count * 7);
      for (let i = 0; i < count; i++) {
        const s = shellPoint(), seed = Math.random(), t = triPoint(i, count, seed);
        data.set([s[0], s[1], s[2], t[0], t[1], t[2], seed], i * 7);
      }
      if (vao) gl.deleteVertexArray(vao); if (buf) gl.deleteBuffer(buf);
      vao = gl.createVertexArray(); gl.bindVertexArray(vao);
      buf = gl.createBuffer(); gl.bindBuffer(gl.ARRAY_BUFFER, buf); gl.bufferData(gl.ARRAY_BUFFER, data, gl.STATIC_DRAW);
      const at = (name, size, off) => { const l = gl.getAttribLocation(prog, name); gl.enableVertexAttribArray(l); gl.vertexAttribPointer(l, size, gl.FLOAT, false, 28, off); };
      at("aSphere", 3, 0); at("aTri", 3, 12); at("aSeed", 1, 24);
      canvas.dataset.tier = String(tier); canvas.dataset.count = String(count);
    }
    function size() {
      const dpr = Math.min(W.devicePixelRatio || 1, 2), w = canvas.clientWidth || 300, h = canvas.clientHeight || 300;
      if (canvas.width !== Math.round(w * dpr) || canvas.height !== Math.round(h * dpr)) { canvas.width = Math.round(w * dpr); canvas.height = Math.round(h * dpr); }
      return dpr;
    }
    let last = performance.now(), slow = 0, fast = 0;
    function frame(now) {
      if (dead) return;
      raf = requestAnimationFrame(frame);
      if (document.hidden) return;
      const dt = Math.min(0.1, (now - last) / 1000); last = now;
      // Adaptive quality: drop a tier when frames run long, climb back after a long calm stretch.
      if (dt > 0.034) { slow++; fast = 0; } else { fast++; slow = Math.max(0, slow - 1); }
      if (slow > 45 && tier > 0) { tier--; slow = 0; upload(); }
      if (fast > 900 && tier < 2 && st.lvlTarget < 0.05) { tier++; fast = 0; upload(); }
      const dpr = size();
      // Fast attack, slow release.
      const k = st.lvlTarget > st.level ? 1 - Math.exp(-dt * 22) : 1 - Math.exp(-dt * 4);
      st.level += (st.lvlTarget - st.level) * k;
      st.morph += (st.morphTarget - st.morph) * (1 - Math.exp(-dt * (st.morphTarget > st.morph ? 2.2 : 5)));
      st.dim += (st.dimTarget - st.dim) * (1 - Math.exp(-dt * 4));
      const ck = 1 - Math.exp(-dt * 2.2);
      for (let i = 0; i < 3; i++) st.color[i] += (st.colorTarget[i] - st.color[i]) * ck;
      st.rot += dt * (reduce ? 0 : 0.16) * (1 - 0.7 * st.morph);
      gl.viewport(0, 0, canvas.width, canvas.height);
      gl.clearColor(0, 0, 0, 0); gl.clear(gl.COLOR_BUFFER_BIT);
      gl.enable(gl.BLEND); gl.blendFunc(gl.ONE, gl.ONE); gl.disable(gl.DEPTH_TEST);
      gl.useProgram(prog); gl.bindVertexArray(vao);
      const rgb = toRgb(st.color);
      gl.uniform1f(U.uTime, reduce ? 0 : now / 1000); gl.uniform1f(U.uLevel, reduce ? 0 : st.level); gl.uniform1f(U.uMorph, st.morph);
      gl.uniform1f(U.uRot, st.rot); gl.uniform1f(U.uAspect, canvas.width / canvas.height); gl.uniform1f(U.uPx, dpr * Math.max(1, canvas.clientWidth / 300));
      gl.uniform1f(U.uJitter, st.stress); gl.uniform1f(U.uFrozen, st.frozen); gl.uniform3f(U.uColor, rgb[0], rgb[1], rgb[2]);
      gl.uniform1f(U.uIntensity, 1.0); gl.uniform1f(U.uDim, st.dim);
      gl.drawArrays(gl.POINTS, 0, count);
    }
    function start() { try { build(); } catch (e) { console.error("orb", e); canvas.dataset.fallback = "1"; return; } last = performance.now(); raf = requestAnimationFrame(frame); }
    canvas.addEventListener("webglcontextlost", (e) => { e.preventDefault(); cancelAnimationFrame(raf); vao = buf = null; });
    canvas.addEventListener("webglcontextrestored", start);
    start();
    return {
      level(v) { st.lvlTarget = Math.max(0, Math.min(1, v)); },
      set(o) {
        if (o.color) st.colorTarget = toLab(hexRgb(o.color));
        if (o.thinking != null) st.morphTarget = o.thinking ? 1 : 0;
        if (o.muted != null) { st.dimTarget = o.muted ? 1 : 0; st.frozen = o.muted ? 1 : 0; }
        if (o.stress != null) st.stress = o.stress ? 1 : 0;
        if (o.snap) { st.color = st.colorTarget.slice(); st.morph = st.morphTarget; }
      },
      destroy() { dead = true; cancelAnimationFrame(raf); },
    };
  }
  W.AosOrb = { mount: mountOrb };

  // ---------------------------------------------------------------------------------------
  // The call session.
  const S = { phase: "idle", callId: null, base: "", token: "", muted: false, speaker: true, who: "none", startedAt: 0 };
  let pc = null, mic = null, audio = null, ctx = null, anMic = null, anOut = null, loop = 0, orb = null, orbOpts = {}, holdUntil = 0, readySent = false;
  const api = async (path, body) => {
    const r = await fetch(S.base.replace(/\/$/, "") + path, { method: "POST", headers: { "Content-Type": "application/json", Authorization: "Bearer " + S.token }, body: JSON.stringify(body || {}) });
    const j = await r.json().catch(() => ({}));
    if (!r.ok) throw new Error((j.error && j.error.message) || ("HTTP " + r.status));
    return j;
  };
  const phase = (p, extra) => { S.phase = p; emit(Object.assign({ t: "phase", phase: p, callId: S.callId, startedAt: S.startedAt }, extra || {})); };
  function rms(an, buf) { an.getFloatTimeDomainData(buf); let s = 0; for (let i = 0; i < buf.length; i++) s += buf[i] * buf[i]; return Math.sqrt(s / buf.length); }
  function levels() {
    const bm = new Float32Array(512), bo = new Float32Array(512);
    let lvl = 0;
    const tick = () => {
      loop = requestAnimationFrame(tick);
      const you = anMic && !S.muted ? rms(anMic, bm) : 0, her = anOut ? rms(anOut, bo) : 0;
      // Voice gate: a little above the room's noise floor.
      const now = performance.now();
      let who = S.who;
      if (her > 0.012) { who = "nora"; holdUntil = now + 350; }
      else if (you > 0.02) { if (now > holdUntil) who = "you"; }
      else if (now > holdUntil + 250) who = "none";
      if (who !== S.who) { S.who = who; emit({ t: "who", who }); }
      const target = who === "nora" ? Math.min(1, her * 9) : who === "you" ? Math.min(0.55, you * 5) : 0;
      lvl += (target - lvl) * (target > lvl ? 0.5 : 0.08);
      if (orb) orb.level(target);
      document.documentElement.style.setProperty("--call-lvl", lvl.toFixed(3));
    };
    tick();
  }
  async function start(o) {
    if (S.phase === "connecting" || S.phase === "live") return;
    S.base = o.base || location.origin; S.token = o.token || ""; S.callId = null; S.muted = false; S.who = "none"; readySent = false;
    phase("connecting");
    try {
      ctx = ctx || new (W.AudioContext || W.webkitAudioContext)();
      if (ctx.state === "suspended") await ctx.resume();
      mic = await navigator.mediaDevices.getUserMedia({ audio: { echoCancellation: true, noiseSuppression: true, autoGainControl: true } });
      pc = new RTCPeerConnection();
      mic.getTracks().forEach((t) => pc.addTrack(t, mic));
      pc.createDataChannel("oai-events");
      audio = audio || Object.assign(document.createElement("audio"), { autoplay: true });
      audio.setAttribute("playsinline", ""); if (!audio.isConnected) document.body.appendChild(audio);
      pc.ontrack = (e) => {
        const stream = e.streams[0] || new MediaStream([e.track]);
        audio.srcObject = stream; audio.play().catch(() => {});
        try { anOut = ctx.createAnalyser(); anOut.fftSize = 1024; ctx.createMediaStreamSource(stream).connect(anOut); } catch (_) {}
        e.track.onunmute = ready;
      };
      pc.onconnectionstatechange = () => {
        const s = pc && pc.connectionState;
        if (s === "connected") ready();
        if ((s === "failed" || s === "closed") && S.phase === "live") end("connection " + s);
      };
      anMic = ctx.createAnalyser(); anMic.fftSize = 1024; ctx.createMediaStreamSource(mic).connect(anMic);
      const offer = await pc.createOffer();
      await pc.setLocalDescription(offer);
      await new Promise((res) => { if (pc.iceGatheringState === "complete") return res(); const t = setTimeout(res, 1500); pc.onicegatheringstatechange = () => { if (pc.iceGatheringState === "complete") { clearTimeout(t); res(); } }; });
      const r = await api("/api/nora/call/start", { sdp: pc.localDescription.sdp, reason: o.reason || null, ring_id: o.ringId || null });
      S.callId = r.call_id; S.startedAt = Date.now();
      await pc.setRemoteDescription({ type: "answer", sdp: r.sdp });
      phase("live", { kind: r.kind });
      levels();
    } catch (e) {
      cleanup();
      phase("failed", { error: String((e && e.message) || e) });
    }
  }
  function ready() {
    if (readySent || !S.callId) return;
    readySent = true;
    api("/api/nora/call/" + encodeURIComponent(S.callId) + "/ready").catch(() => {});
  }
  function cleanup() {
    cancelAnimationFrame(loop);
    try { pc && pc.close(); } catch (_) {}
    try { mic && mic.getTracks().forEach((t) => t.stop()); } catch (_) {}
    pc = null; mic = null; anMic = null; anOut = null;
    if (audio) audio.srcObject = null;
    document.documentElement.style.setProperty("--call-lvl", "0");
    if (orb) orb.level(0);
  }
  async function end(why) {
    if (S.phase !== "live" && S.phase !== "connecting") return;
    const id = S.callId;
    cleanup();
    phase("ended", { why: why || "hung up" });
    if (id) api("/api/nora/call/" + encodeURIComponent(id) + "/end").catch(() => {});
  }
  function setMuted(m) {
    S.muted = !!m;
    if (mic) mic.getAudioTracks().forEach((t) => (t.enabled = !S.muted));
    if (orb) orb.set({ muted: S.muted });
    emit({ t: "muted", muted: S.muted });
  }
  async function toggleSpeaker() {
    // Desktop browsers: switch between the default output and the next one. Phones need a native
    // audio-session plugin (spec §1.1); until then the button reports what it could do.
    S.speaker = !S.speaker;
    let routed = false;
    try {
      if (audio && audio.setSinkId && navigator.mediaDevices.enumerateDevices) {
        const outs = (await navigator.mediaDevices.enumerateDevices()).filter((d) => d.kind === "audiooutput");
        if (outs.length > 1) { await audio.setSinkId(S.speaker ? "" : outs[1].deviceId); routed = true; }
      }
    } catch (_) {}
    emit({ t: "speaker", speaker: S.speaker, routed });
  }
  W.aosCall = {
    state: S, start, end, setMuted, toggleSpeaker,
    mountOrb(canvas) { if (orb) orb.destroy(); orb = mountOrb(canvas); if (orb) orb.set(Object.assign({ snap: true }, orbOpts, { muted: S.muted })); return !!orb; },
    unmountOrb() { if (orb) orb.destroy(); orb = null; },
    setOrb(o) { orbOpts = Object.assign(orbOpts, o); if (orb) orb.set(o); },
  };
  emit({ t: "hello", phase: S.phase });
})();
