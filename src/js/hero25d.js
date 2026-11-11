// The 2.5D hero stage: a turntable of pedestals.
// Rust renders the items and owns the discrete state on #stage (data-n, data-spin,
// data-focus). This script owns the continuous state: it follows the finger, runs the
// springs, sets each item's transform, and reports a settled swipe back as "spin:<n>".
// Taps become DOM events on #hero-stage, the same events the three.js hero will send.
// Runs inside a Dioxus eval: `dioxus` is the channel to Rust.

const G = (window.__aosHero = window.__aosHero || { timeScale: 1 });
G.send = (msg) => { try { dioxus.send(msg); } catch (_) {} };

if (!G.installed) {
  G.installed = true;

  const reduce = window.matchMedia("(prefers-reduced-motion: reduce)");
  const K = 250, C = 29;   // spin spring (damping ratio ~0.92)
  const KF = 190, CF = 26; // focus spring (~0.94)

  const st = {
    el: null, n: 3, spin: 0, v: 0, target: 0,
    f: 0, fv: 0, ftarget: 0, focusKind: "",
    tx: 0, ty: 0, ttx: 0, tty: 0,
    drag: null, raf: 0, last: 0, front: "", hover: false,
  };

  const clamp = (v, a, b) => Math.max(a, Math.min(b, v));
  const lerp = (a, b, t) => {
    const r = {};
    for (const k in a) r[k] = a[k] + (b[k] - a[k]) * t;
    return r;
  };
  const wrap = (o, n) => {
    o = ((o % n) + n) % n;
    return o > n / 2 ? o - n : o;
  };
  const isFigure = (kind) => kind === "me" || kind === "mind";

  // Keep in step with `geom` in src/pages/home/hero.rs (first paint uses that copy).
  function geom(kind, o, n) {
    const a = Math.abs(o), sg = o < 0 ? -1 : 1;
    const F = { x: 0, y: 0, s: 1, o: 1, z: 10, p: 1, r: 0 };
    const x1 = n === 4 ? 0.335 : 0.31, s1 = n === 4 ? 0.56 : 0.6;
    const L = { x: sg * x1, y: -0.03, s: s1, o: 0.82, z: 5, p: 1, r: -sg * 12 };
    let B, end;
    if (n === 4) {
      end = 2;
      if (isFigure(kind)) {
        const side = kind === "me" ? -1 : 1;
        B = { x: side * 0.16, y: -0.02, s: 0.84, o: 0.9, z: 7, p: 0, r: -side * 10 };
      } else {
        B = { x: 0, y: -0.08, s: 0.42, o: 0, z: 0, p: 1, r: 0 };
      }
    } else {
      end = 1.5;
      B = { x: 0, y: -0.075, s: 0.42, o: 0, z: 0, p: 1, r: 0 };
    }
    return a <= 1 ? lerp(F, L, a) : lerp(L, B, (a - 1) / (end - 1));
  }

  // Focus: the chosen goal grows and lifts, the rest slide out and fade.
  const FOCUS = {
    physical: { s: 1.28, y: -0.235 },
    money: { s: 1.34, y: -0.235 },
    mind: { s: 0.88, y: -0.2 },
    me: { s: 1, y: 0 },
  };
  function focusMod(g, kind) {
    const f = st.f;
    if (f <= 0.0005 || !st.focusKind) return g;
    if (kind === st.focusKind) {
      const t = FOCUS[kind] || FOCUS.me;
      g.s *= 1 + (t.s - 1) * f;
      g.y += t.y * f;
      g.z = 12;
      g.r *= 1 - f;
    } else {
      const dir = g.x === 0 ? (kind === "me" ? -1 : 1) : Math.sign(g.x);
      g.x += dir * 0.6 * f;
      g.o *= 1 - f;
    }
    return g;
  }

  function stageEl() {
    const el = document.getElementById("stage");
    if (el !== st.el) {
      st.el = el;
      if (el) sync(true);
    }
    return el;
  }

  function sync(initial) {
    const el = st.el;
    if (!el) return;
    const n = +el.dataset.n || 3;
    const target = +el.dataset.spin || 0;
    const fk = el.dataset.focus || "";
    if (initial || n !== st.n) {
      if (!initial) {
        // The set of items changed (safe mode toggled): let CSS cross-fade the new layout.
        el.dataset.morph = "1";
        clearTimeout(G.morphTimer);
        G.morphTimer = setTimeout(() => { if (st.el) delete st.el.dataset.morph; }, 700);
      }
      st.n = n;
      st.spin = target;
      st.v = 0;
    }
    st.target = target;
    if (fk) st.focusKind = fk;
    st.ftarget = fk ? 1 : 0;
    if (initial) {
      st.f = st.ftarget;
      st.fv = 0;
    }
    if (reduce.matches) {
      st.spin = st.target; st.v = 0;
      st.f = st.ftarget; st.fv = 0;
    }
    kick();
  }

  function apply() {
    const el = st.el;
    if (!el) return;
    const n = st.n;
    let frontKind = "";
    let best = 9;
    for (const it of el.querySelectorAll(".st-item")) {
      const kind = it.dataset.kind;
      const o = wrap(+it.dataset.idx - st.spin, n);
      const g = focusMod(geom(kind, o, n), kind);
      const r = isFigure(kind) ? g.r : g.r * 0.35;
      it.style.transform =
        `translate3d(calc(-50% + ${(g.x * 100).toFixed(3)}cqw), ${(g.y * 100).toFixed(3)}cqh, 0) ` +
        `scale(${g.s.toFixed(4)}) rotateY(${r.toFixed(2)}deg)`;
      it.style.opacity = g.o.toFixed(3);
      it.style.zIndex = String(Math.round(g.z));
      it.style.pointerEvents = g.o < 0.2 ? "none" : "";
      const a = Math.abs(o);
      // Depth of field: blend in the pre-blurred copy away from the front.
      const focusSharp = kind === st.focusKind ? 1 - st.f : 1;
      const soft = Math.min(1, a) * 0.55 * focusSharp;
      const ped = it.querySelector(".st-ped");
      if (ped) ped.style.opacity = g.p.toFixed(3);
      const pedSoft = it.querySelector(".st-ped-soft");
      if (pedSoft) pedSoft.style.opacity = (soft * g.p).toFixed(3);
      const figSoft = it.querySelector(".st-soft");
      if (figSoft) figSoft.style.opacity = soft.toFixed(3);
      if (a < 0.5) it.dataset.front = "";
      else delete it.dataset.front;
      if (a < best) { best = a; frontKind = kind; }
    }
    if (frontKind && frontKind !== st.front) {
      st.front = frontKind;
      const root = document.getElementById("hero-stage");
      if (root) root.dispatchEvent(new CustomEvent("aos:front", { bubbles: true, detail: { item: frontKind } }));
    }
  }

  function parallax() {
    const el = st.el;
    if (!el) return;
    const far = el.querySelector(".st-far");
    const near = el.querySelector(".st-near");
    if (far) far.style.transform = `translate3d(${(-st.tx * 10).toFixed(2)}px, ${(-st.ty * 6).toFixed(2)}px, 0)`;
    if (near) near.style.transform = `translate3d(${(st.tx * 18).toFixed(2)}px, ${(st.ty * 10).toFixed(2)}px, 0)`;
  }

  function tick(now) {
    st.raf = 0;
    const dt = Math.min(0.05, st.last ? (now - st.last) / 1000 : 0) * (G.timeScale || 1);
    st.last = now;
    let moving = !!st.drag;
    if (!st.drag && dt > 0) {
      const steps = Math.max(1, Math.ceil(dt * 240));
      const h = dt / steps;
      for (let i = 0; i < steps; i++) {
        st.v += (-K * (st.spin - st.target) - C * st.v) * h;
        st.spin += st.v * h;
        st.fv += (-KF * (st.f - st.ftarget) - CF * st.fv) * h;
        st.f += st.fv * h;
      }
    }
    if (!st.drag) {
      if (Math.abs(st.spin - st.target) < 0.0004 && Math.abs(st.v) < 0.002) { st.spin = st.target; st.v = 0; }
      else moving = true;
      if (Math.abs(st.f - st.ftarget) < 0.0004 && Math.abs(st.fv) < 0.002) {
        st.f = st.ftarget; st.fv = 0;
        if (!st.f) st.focusKind = "";
      } else moving = true;
    }
    const ease = Math.min(1, dt * 7);
    st.tx += (st.ttx - st.tx) * ease;
    st.ty += (st.tty - st.ty) * ease;
    if (Math.abs(st.ttx - st.tx) > 0.002 || Math.abs(st.tty - st.ty) > 0.002) moving = true;
    apply();
    parallax();
    if (moving || dt === 0) st.raf = requestAnimationFrame(tick);
    else st.last = 0;
  }

  function kick() {
    if (!st.raf) st.raf = requestAnimationFrame(tick);
  }
  G.kick = kick;

  function emit(kind) {
    const root = document.getElementById("hero-stage");
    if (!root) return;
    if (kind === "me") root.dispatchEvent(new CustomEvent("aos:self-tap", { bubbles: true }));
    else root.dispatchEvent(new CustomEvent("aos:goal-tap", { bubbles: true, detail: { goal: kind } }));
  }

  // ---- input -----------------------------------------------------------
  const slotPx = (w) => w * (st.n === 4 ? 0.335 : 0.31);

  document.addEventListener("pointerdown", (e) => {
    if (e.button !== 0) return;
    const el = stageEl();
    if (!el || !el.contains(e.target)) return;
    const app = el.closest(".app");
    if (app && app.dataset.lock !== "open") return;
    const now = performance.now();
    st.drag = {
      id: e.pointerId, x0: e.clientX, y0: e.clientY, t0: now,
      spin0: st.spin, axis: null, w: el.clientWidth || 390,
      lastX: e.clientX, lastT: now, vx: 0, moved: false,
      target: e.target,
    };
  }, { passive: true });

  document.addEventListener("pointermove", (e) => {
    const d = st.drag;
    if (d && e.pointerId === d.id) {
      const dx = e.clientX - d.x0, dy = e.clientY - d.y0;
      if (!d.axis) {
        if (Math.abs(dx) < 7 && Math.abs(dy) < 7) return;
        d.axis = Math.abs(dx) > Math.abs(dy) * 1.1 ? "x" : "y";
      }
      if (d.axis !== "x" || st.ftarget) { st.drag = null; st.ttx = 0; kick(); return; }
      d.moved = true;
      const now = performance.now();
      const dtm = Math.max(1, now - d.lastT);
      d.vx = 0.75 * (((e.clientX - d.lastX) / dtm) * 1000) + 0.25 * d.vx;
      d.lastX = e.clientX;
      d.lastT = now;
      st.spin = d.spin0 - dx / slotPx(d.w);
      st.v = 0;
      st.ttx = clamp(dx / d.w, -1, 1) * 0.7;
      kick();
      return;
    }
    // Mouse hover tilts the scene a little on desktop.
    if (e.pointerType === "mouse" && st.el && !st.drag) {
      const r = st.el.getBoundingClientRect();
      const inside = e.clientX >= r.left && e.clientX <= r.right && e.clientY >= r.top && e.clientY <= r.bottom;
      if (inside) {
        st.ttx = clamp(((e.clientX - r.left) / r.width - 0.5) * 2, -1, 1) * 0.6;
        st.tty = clamp(((e.clientY - r.top) / r.height - 0.5) * 2, -1, 1) * 0.6;
        st.hover = true;
        kick();
      } else if (st.hover) {
        st.hover = false;
        st.ttx = 0; st.tty = 0;
        kick();
      }
    }
  }, { passive: true });

  function end(e) {
    const d = st.drag;
    if (!d || e.pointerId !== d.id) return;
    st.drag = null;
    if (d.moved) {
      const slot = slotPx(d.w);
      const vs = -d.vx / slot;                    // slots per second
      const proj = st.spin + vs * 0.16;
      const base = Math.round(d.spin0);
      const t = clamp(Math.round(proj), base - 1, base + 1);
      st.target = t;
      st.v = clamp(vs, -8, 8);
      st.ttx = 0;
      G.send("spin:" + t);
      kick();
    } else if (e.type === "pointerup" && performance.now() - d.t0 < 700) {
      const item = d.target && d.target.closest ? d.target.closest(".st-item") : null;
      if (item && parseFloat(item.style.opacity || "1") > 0.2) {
        const kind = item.dataset.kind;
        if (item.dataset.front === undefined && kind === "me") {
          // A tap on the character at a flank turns him back to the front.
          const idx = +item.dataset.idx;
          const t = st.target + wrap(idx - st.target, st.n);
          st.target = Math.round(t);
          G.send("spin:" + st.target);
          kick();
        } else {
          emit(kind);
        }
      }
      kick();
    }
  }
  document.addEventListener("pointerup", end, { passive: true });
  document.addEventListener("pointercancel", end, { passive: true });

  document.addEventListener("keydown", (e) => {
    const item = e.target && e.target.closest ? e.target.closest(".st-item") : null;
    if (!item) return;
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      emit(item.dataset.kind);
    } else if (e.key === "ArrowRight" || e.key === "ArrowLeft") {
      e.preventDefault();
      st.target += e.key === "ArrowRight" ? 1 : -1;
      G.send("spin:" + st.target);
      kick();
    }
  });

  // Phone tilt. iOS asks for permission, which needs a tap: the unlock button carries
  // data-motion-permission so the request rides on that gesture.
  window.addEventListener("deviceorientation", (e) => {
    if (e.gamma == null || reduce.matches) return;
    if (G.baseBeta == null) G.baseBeta = e.beta;
    st.ttx = clamp(e.gamma / 30, -1, 1) * 0.8;
    st.tty = clamp((e.beta - G.baseBeta) / 30, -1, 1) * 0.6;
    kick();
  });
  document.addEventListener("click", (e) => {
    const t = e.target && e.target.closest ? e.target.closest("[data-motion-permission]") : null;
    const DOE = window.DeviceOrientationEvent;
    if (t && DOE && typeof DOE.requestPermission === "function") DOE.requestPermission().catch(() => {});
  }, true);

  // Rust changes data-* on #stage (focus, safe mode); a remount brings a new #stage.
  new MutationObserver((records) => {
    const el = document.getElementById("stage");
    if (el !== st.el) { stageEl(); return; }
    if (!el) return;
    for (const r of records) {
      if (r.type === "attributes" && r.target === el) { sync(false); return; }
      if (r.type === "childList" && el.contains(r.target)) { kick(); return; }
    }
  }).observe(document.body, {
    subtree: true, childList: true, attributes: true,
    attributeFilter: ["data-spin", "data-n", "data-focus"],
  });

  stageEl();
}

G.kick && G.kick();
// Keep this eval (and its channel to Rust) alive.
await new Promise(() => {});
