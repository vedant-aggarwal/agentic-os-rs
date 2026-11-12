// Visualization board: the infinite canvas and image capture (design/specs/boards.md section 4).
// Runs inside a Dioxus eval: `dioxus` is the channel to Rust.
//
// Rust renders the cards (.mc-item with data-x/y/w/h) inside .mc-world; this module owns the
// camera and the gestures and never re-renders cards. It tells Rust only about commits:
//   {type:"open", id}                 a tap on a card
//   {type:"moves", moves:[{id,x,y,w,h,z}]}  a drag or resize ended
//   {type:"uploaded", asset, board}   an image reached the server (POST /api/media)
//   {type:"paste-url", url} / {type:"paste-text", text}
//   {type:"busy", on} / {type:"error", message}
// Camera per board is kept in localStorage. Off-screen cards get display:none (culling).

const B = (window.__aosBoard = window.__aosBoard || {});
B.send = (m) => { try { dioxus.send(m); } catch (_) {} };
B.cfg = B.cfg || { url: "", token: "" };
B.config = (c) => { B.cfg = { ...B.cfg, ...c }; };
B.board = B.board || null;

if (!B.installed) {
  B.installed = true;
  const MAXPX = 2000;
  const cam = { x: 0, y: 0, z: 1 };
  let vp = null, world = null, raf = 0, saveT = 0;
  const pts = new Map();
  let drag = null, pinch = null, pan = null;

  const key = () => `aos.cam.${B.board}`;
  const items = () => (world ? Array.from(world.querySelectorAll(".mc-item")) : []);
  const num = (el, k) => parseFloat(el.dataset[k] || "0");

  const apply = () => {
    raf = 0;
    if (!world || !vp) return;
    world.style.transform = `translate(${cam.x}px, ${cam.y}px) scale(${cam.z})`;
    const g = 28 * cam.z;
    vp.style.backgroundSize = `${g}px ${g}px`;
    vp.style.backgroundPosition = `${cam.x}px ${cam.y}px`;
    vp.style.setProperty("--grid-a", cam.z < 0.35 ? "0" : String(Math.min(1, (cam.z - 0.35) * 2)));
    vp.dataset.lod = cam.z < 0.45 ? "far" : "near";
    // Cull: only cards that touch the view (plus a margin) stay in the layout.
    const r = vp.getBoundingClientRect();
    const m = 300 / cam.z;
    const x0 = -cam.x / cam.z - m, y0 = -cam.y / cam.z - m;
    const x1 = (r.width - cam.x) / cam.z + m, y1 = (r.height - cam.y) / cam.z + m;
    for (const el of items()) {
      const x = num(el, "x"), y = num(el, "y"), w = num(el, "w"), h = num(el, "h");
      const vis = x < x1 && x + w > x0 && y < y1 && y + h > y0;
      el.style.display = vis || el === (drag && drag.el) ? "" : "none";
    }
  };
  const schedule = () => { if (!raf) raf = requestAnimationFrame(apply); };
  const save = () => {
    clearTimeout(saveT);
    saveT = setTimeout(() => { try { localStorage.setItem(key(), JSON.stringify(cam)); } catch (_) {} }, 300);
  };
  const zoomAt = (px, py, z2) => {
    z2 = Math.max(0.12, Math.min(4, z2));
    cam.x = px - (px - cam.x) * (z2 / cam.z);
    cam.y = py - (py - cam.y) * (z2 / cam.z);
    cam.z = z2;
    schedule();
    save();
  };

  B.fit = () => {
    if (!vp) return;
    const els = items();
    const r = vp.getBoundingClientRect();
    if (!els.length || !r.width) { cam.x = r.width / 2 - 130; cam.y = 80; cam.z = 1; schedule(); return; }
    let x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity;
    for (const el of els) {
      const x = num(el, "x"), y = num(el, "y");
      x0 = Math.min(x0, x); y0 = Math.min(y0, y);
      x1 = Math.max(x1, x + num(el, "w")); y1 = Math.max(y1, y + num(el, "h"));
    }
    const pad = 36;
    const z = Math.max(0.12, Math.min(1.4, Math.min((r.width - pad * 2) / (x1 - x0), (r.height - pad * 2) / (y1 - y0))));
    cam.z = z;
    cam.x = (r.width - (x1 - x0) * z) / 2 - x0 * z;
    cam.y = (r.height - (y1 - y0) * z) / 2 - y0 * z;
    schedule();
    save();
  };
  B.zoom = (k) => { if (!vp) return; const r = vp.getBoundingClientRect(); zoomAt(r.width / 2, r.height / 2, cam.z * k); };

  // Find the viewport for the open board (Rust calls this after rendering it).
  B.mount = (board) => {
    B.board = board;
    let n = 0;
    const tryMount = () => {
      const v = document.querySelector(`.mc-viewport[data-board="${board}"]`);
      if (!v) { if (n++ < 60) setTimeout(tryMount, 50); return; }
      if (v === vp) { schedule(); return; }
      vp = v; world = v.querySelector(".mc-world");
      let saved = null;
      try { saved = JSON.parse(localStorage.getItem(key()) || "null"); } catch (_) {}
      if (saved && isFinite(saved.z)) Object.assign(cam, saved); else B.fit();
      new MutationObserver(schedule).observe(world, { childList: true, subtree: true, attributes: true, attributeFilter: ["data-x", "data-y", "data-w", "data-h"] });
      schedule();
    };
    tryMount();
  };
  B.unmount = () => { vp = null; world = null; };

  const local = (e) => { const r = vp.getBoundingClientRect(); return { x: e.clientX - r.left, y: e.clientY - r.top }; };
  const inVp = (e) => vp && vp.contains(e.target);

  document.addEventListener("pointerdown", (e) => {
    if (!inVp(e) || e.target.closest(".mc-canvas-tools")) return;
    vp.setPointerCapture && vp.setPointerCapture(e.pointerId);
    pts.set(e.pointerId, local(e));
    if (pts.size === 2) {
      // A second finger cancels a drag and starts a pinch (which also pans).
      if (drag) { drag.el.style.transform = drag.orig; drag = null; }
      const [a, b] = Array.from(pts.values());
      pinch = { d: Math.hypot(a.x - b.x, a.y - b.y) || 1, z: cam.z, mx: (a.x + b.x) / 2, my: (a.y + b.y) / 2, cx: cam.x, cy: cam.y };
      pan = null;
      world.style.willChange = "transform";
      return;
    }
    if (pts.size > 2) return;
    const item = e.target.closest(".mc-item");
    const p = local(e);
    if (item) {
      drag = {
        el: item, id: item.dataset.id, sx: p.x, sy: p.y, t: Date.now(), moved: false,
        x: num(item, "x"), y: num(item, "y"), w: num(item, "w"), h: num(item, "h"),
        resize: !!e.target.closest(".mc-rz"), orig: item.style.transform,
      };
    } else {
      pan = { sx: p.x, sy: p.y, cx: cam.x, cy: cam.y };
      world.style.willChange = "transform";
    }
  }, true);

  document.addEventListener("pointermove", (e) => {
    if (!pts.has(e.pointerId) || !vp) return;
    const p = local(e);
    pts.set(e.pointerId, p);
    if (pinch && pts.size >= 2) {
      const [a, b] = Array.from(pts.values());
      const d = Math.hypot(a.x - b.x, a.y - b.y) || 1;
      const mx = (a.x + b.x) / 2, my = (a.y + b.y) / 2;
      const z2 = Math.max(0.12, Math.min(4, pinch.z * (d / pinch.d)));
      cam.x = mx - (pinch.mx - pinch.cx) * (z2 / pinch.z);
      cam.y = my - (pinch.my - pinch.cy) * (z2 / pinch.z);
      cam.z = z2;
      schedule();
      return;
    }
    if (pan) { cam.x = pan.cx + (p.x - pan.sx); cam.y = pan.cy + (p.y - pan.sy); schedule(); return; }
    if (drag) {
      const dx = (p.x - drag.sx) / cam.z, dy = (p.y - drag.sy) / cam.z;
      if (!drag.moved && Math.hypot(p.x - drag.sx, p.y - drag.sy) < 6) return;
      drag.moved = true;
      drag.el.classList.add("is-dragging");
      if (drag.resize) {
        const ratio = drag.h / drag.w;
        const w = Math.max(80, drag.w + dx);
        const h = e.shiftKey || drag.el.dataset.kind === "image" ? w * ratio : Math.max(60, drag.h + dy);
        drag.nw = w; drag.nh = h;
        drag.el.style.width = `${w}px`; drag.el.style.height = `${h}px`;
      } else {
        drag.nx = drag.x + dx; drag.ny = drag.y + dy;
        drag.el.style.transform = `translate(${drag.nx}px, ${drag.ny}px)`;
      }
    }
  }, true);

  const end = (e) => {
    if (!pts.has(e.pointerId)) return;
    pts.delete(e.pointerId);
    if (pinch) { if (pts.size < 2) { pinch = null; world.style.willChange = ""; save(); } return; }
    if (pan) { pan = null; world.style.willChange = ""; save(); return; }
    if (drag) {
      const d = drag;
      drag = null;
      d.el.classList.remove("is-dragging");
      if (!d.moved) {
        if (e.type === "pointerup") B.send({ type: "open", id: d.id });
        return;
      }
      const maxZ = Math.max(0, ...items().map((el) => parseInt(el.dataset.z || "0", 10)));
      const m = { id: d.id, z: maxZ + 1 };
      if (d.resize) { m.w = Math.round(d.nw); m.h = Math.round(d.nh); d.el.dataset.w = m.w; d.el.dataset.h = m.h; }
      else { m.x = Math.round(d.nx); m.y = Math.round(d.ny); d.el.dataset.x = m.x; d.el.dataset.y = m.y; }
      d.el.dataset.z = m.z; d.el.style.zIndex = m.z;
      B.send({ type: "moves", moves: [m] });
    }
  };
  document.addEventListener("pointerup", end, true);
  document.addEventListener("pointercancel", end, true);

  // Wheel: Ctrl+wheel (and a trackpad pinch, which arrives as Ctrl+wheel) zooms; plain wheel pans.
  document.addEventListener("wheel", (e) => {
    if (!inVp(e)) return;
    e.preventDefault();
    const p = local(e);
    if (e.ctrlKey || e.metaKey) zoomAt(p.x, p.y, cam.z * Math.exp(-e.deltaY * 0.01));
    else { cam.x -= e.deltaX; cam.y -= e.deltaY; schedule(); save(); }
  }, { passive: false, capture: true });

  document.addEventListener("keydown", (e) => {
    if (!vp || !document.body.contains(vp) || /INPUT|TEXTAREA/.test(document.activeElement && document.activeElement.tagName)) return;
    if ((e.ctrlKey || e.metaKey) && e.key === "0") { e.preventDefault(); B.fit(); }
    else if (e.key === "+" || e.key === "=") B.zoom(1.2);
    else if (e.key === "-") B.zoom(1 / 1.2);
  });

  // ------------------------------------------------------------------------------------------
  // Images: pick, paste or drop -> shrink -> POST /api/media.

  const shrink = async (file) => {
    if (file.type === "image/gif") return file;
    try {
      const bmp = await createImageBitmap(file);
      const k = Math.min(1, MAXPX / Math.max(bmp.width, bmp.height));
      const c = document.createElement("canvas");
      c.width = Math.round(bmp.width * k);
      c.height = Math.round(bmp.height * k);
      c.getContext("2d").drawImage(bmp, 0, 0, c.width, c.height);
      const blob = await new Promise((res) => c.toBlob(res, "image/jpeg", 0.86));
      if (blob && blob.size > 0) return { blob, w: c.width, h: c.height };
    } catch (_) {}
    return { blob: file, w: 0, h: 0 };
  };

  B.upload = async (file, board) => {
    if (!file || !/^image\//.test(file.type || "image/")) { B.send({ type: "error", message: "That isn't an image." }); return; }
    B.send({ type: "busy", on: true });
    try {
      const s = await shrink(file);
      const blob = s.blob || s;
      const base = (B.cfg.url || "").replace(/\/+$/, "");
      const r = await fetch(`${base}/api/media`, {
        method: "POST",
        headers: { Authorization: "Bearer " + B.cfg.token, "Content-Type": blob.type || "image/jpeg" },
        body: blob,
      });
      const j = await r.json().catch(() => ({}));
      if (r.status === 401) B.send({ type: "error", message: "The server refused this device. Re-pair in You." });
      else if (!r.ok) B.send({ type: "error", message: (j.error && j.error.message) || `Upload failed (HTTP ${r.status})` });
      else B.send({ type: "uploaded", asset: j, board: board || B.board, w: s.w || 0, h: s.h || 0 });
    } catch (_) {
      B.send({ type: "error", message: "Could not reach the server. Try again." });
    }
    B.send({ type: "busy", on: false });
  };

  document.addEventListener("change", (e) => {
    const input = e.target;
    if (!input || !input.matches || !input.matches("input[data-mc-image]")) return;
    const files = Array.from(input.files || []);
    const board = input.dataset.mcImage;
    input.value = "";
    files.slice(0, 12).forEach((f) => B.upload(f, board));
  }, true);

  const pasteInto = (dt) => {
    if (!B.board || !document.querySelector(".mc-layer")) return false;
    const files = Array.from(dt.files || []).filter((f) => /^image\//.test(f.type));
    if (files.length) { files.forEach((f) => B.upload(f, B.board)); return true; }
    const text = (dt.getData && dt.getData("text/plain")) || "";
    if (/^https?:\/\/\S+$/.test(text.trim())) { B.send({ type: "paste-url", url: text.trim() }); return true; }
    if (text.trim()) { B.send({ type: "paste-text", text: text.trim() }); return true; }
    return false;
  };
  document.addEventListener("paste", (e) => {
    const t = document.activeElement && document.activeElement.tagName;
    if (/INPUT|TEXTAREA/.test(t) || !e.clipboardData) return;
    if (pasteInto(e.clipboardData)) e.preventDefault();
  });
  document.addEventListener("dragover", (e) => { if (document.querySelector(".mc-layer")) e.preventDefault(); });
  document.addEventListener("drop", (e) => {
    if (!document.querySelector(".mc-layer") || !e.dataTransfer) return;
    e.preventDefault();
    pasteInto(e.dataTransfer);
  });

  // The Paste button (phones have no Ctrl+V): read the clipboard with permission.
  B.pasteButton = async () => {
    try {
      if (navigator.clipboard && navigator.clipboard.read) {
        const list = await navigator.clipboard.read();
        for (const it of list) {
          const type = it.types.find((t) => t.startsWith("image/"));
          if (type) { const blob = await it.getType(type); B.upload(new File([blob], "paste", { type }), B.board); return; }
        }
      }
      const text = navigator.clipboard ? await navigator.clipboard.readText() : "";
      if (/^https?:\/\/\S+$/.test(text.trim())) B.send({ type: "paste-url", url: text.trim() });
      else if (text.trim()) B.send({ type: "paste-text", text: text.trim() });
      else B.send({ type: "error", message: "The clipboard is empty." });
    } catch (_) {
      B.send({ type: "error", message: "Paste needs clipboard permission. Long-press and paste into the link field instead." });
    }
  };
}
