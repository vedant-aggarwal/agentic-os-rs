// Gestures for the layers above the home: drag a sheet or the goal page down by its
// handle, swipe a page back from the left edge, Escape on desktop. The panel follows
// the finger; past the threshold it finishes the exit itself and tells Rust
// "close:<panel>", which removes the `open` class. Inline styles are cleared once the
// class has caught up, so CSS owns the resting states.
// Runs inside a Dioxus eval: `dioxus` is the channel to Rust.

const L = (window.__aosLayers = window.__aosLayers || {});
L.send = (msg) => { try { dioxus.send(msg); } catch (_) {} };

if (!L.installed) {
  L.installed = true;
  let g = null;

  const isOpen = (p) => !!p.closest(".open");
  const openPanels = () => Array.from(document.querySelectorAll("[data-panel]")).filter(isOpen);

  document.addEventListener("pointerdown", (e) => {
    if (e.button !== 0) return;
    const handle = e.target.closest ? e.target.closest("[data-handle]") : null;
    let panel = null, axis = null;
    if (handle) {
      panel = handle.closest("[data-panel]");
      axis = "y";
    } else if (e.clientX < 26) {
      // Edge swipe: the topmost open page that allows it.
      const pages = openPanels().filter((p) => p.dataset.edge === "1");
      panel = pages[pages.length - 1] || null;
      axis = "x";
    }
    if (!panel || !isOpen(panel)) return;
    g = { id: e.pointerId, panel, axis, x0: e.clientX, y0: e.clientY, t: performance.now(), last: 0, v: 0, d: 0, live: false };
  }, { passive: true });

  document.addEventListener("pointermove", (e) => {
    if (!g || e.pointerId !== g.id) return;
    const dx = e.clientX - g.x0, dy = e.clientY - g.y0;
    if (!g.live) {
      const along = g.axis === "y" ? dy : dx;
      const across = g.axis === "y" ? dx : dy;
      if (Math.abs(along) < 6 && Math.abs(across) < 6) return;
      if (along <= 0 || Math.abs(across) > Math.abs(along)) { g = null; return; }
      g.live = true;
      g.panel.style.transition = "none";
    }
    const d = Math.max(0, g.axis === "y" ? dy : dx);
    const now = performance.now();
    g.v = ((d - g.d) / Math.max(1, now - g.t)) * 1000;
    g.t = now;
    g.d = d;
    g.panel.style.transform = g.axis === "y" ? `translate3d(0, ${d}px, 0)` : `translate3d(${d}px, 0, 0)`;
  }, { passive: true });

  function release(e) {
    if (!g || e.pointerId !== g.id) return;
    const { panel, axis, d, v, live } = g;
    g = null;
    if (!live) return;
    const size = axis === "y" ? panel.offsetHeight : window.innerWidth;
    const close = d > Math.min(140, size * 0.3) || v > 650;
    if (close) {
      panel.style.transition = "transform 280ms cubic-bezier(0.32, 0.72, 0, 1)";
      panel.style.transform = axis === "y" ? `translate3d(0, ${size + 40}px, 0)` : `translate3d(${size}px, 0, 0)`;
      L.send("close:" + panel.dataset.panel);
      setTimeout(() => { panel.style.transition = ""; panel.style.transform = ""; }, 520);
    } else {
      panel.style.transition = "transform 420ms cubic-bezier(0.2, 0.9, 0.25, 1)";
      panel.style.transform = "";
      setTimeout(() => { panel.style.transition = ""; }, 440);
    }
  }
  document.addEventListener("pointerup", release, { passive: true });
  document.addEventListener("pointercancel", release, { passive: true });

  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape") L.send("escape");
  });
}

await new Promise(() => {});
