// Mind OS: paste, drop and photo upload into the library, and "/" to search.
// Runs inside a Dioxus eval: `dioxus` is the channel to Rust. Active only while .mind-page is on
// screen. Messages to Rust:
//   {type:"uploaded", asset, w, h}       an image reached the server (POST /api/media)
//   {type:"paste-url", url} / {type:"paste-text", text}
//   {type:"busy", on} / {type:"error", message}

const M = (window.__aosMind = window.__aosMind || {});
M.send = (m) => { try { dioxus.send(m); } catch (_) {} };
M.cfg = M.cfg || { url: "", token: "" };
M.config = (c) => { M.cfg = { ...M.cfg, ...c }; };

if (!M.installed) {
  M.installed = true;
  const MAXPX = 2000;
  const here = () => !!document.querySelector(".mind-page") && !document.querySelector(".mc-layer");

  const shrink = async (file) => {
    if (file.type === "image/gif") return { blob: file, w: 0, h: 0 };
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

  M.upload = async (file) => {
    if (!file || !/^image\//.test(file.type || "image/")) { M.send({ type: "error", message: "That isn't an image." }); return; }
    M.send({ type: "busy", on: true });
    try {
      const s = await shrink(file);
      const base = (M.cfg.url || "").replace(/\/+$/, "");
      const r = await fetch(`${base}/api/media`, {
        method: "POST",
        headers: { Authorization: "Bearer " + M.cfg.token, "Content-Type": s.blob.type || "image/jpeg" },
        body: s.blob,
      });
      const j = await r.json().catch(() => ({}));
      if (r.status === 401) M.send({ type: "error", message: "The server refused this device. Re-pair in You." });
      else if (!r.ok) M.send({ type: "error", message: (j.error && j.error.message) || `Upload failed (HTTP ${r.status})` });
      else M.send({ type: "uploaded", asset: j, w: s.w || 0, h: s.h || 0 });
    } catch (_) {
      M.send({ type: "error", message: "Could not reach the server. Try again." });
    }
    M.send({ type: "busy", on: false });
  };

  const take = (dt, allowText) => {
    const files = Array.from(dt.files || []).filter((f) => /^image\//.test(f.type));
    if (files.length) { files.slice(0, 12).forEach((f) => M.upload(f)); return true; }
    if (!allowText) return false;
    const text = ((dt.getData && (dt.getData("text/uri-list") || dt.getData("text/plain"))) || "").trim();
    if (/^https?:\/\/\S+$/.test(text)) { M.send({ type: "paste-url", url: text }); return true; }
    if (text) { M.send({ type: "paste-text", text }); return true; }
    return false;
  };

  document.addEventListener("change", (e) => {
    const input = e.target;
    if (!input || !input.matches || !input.matches("input[data-mind-image]")) return;
    const files = Array.from(input.files || []);
    input.value = "";
    files.slice(0, 12).forEach((f) => M.upload(f));
  }, true);

  // Paste anywhere on Mind. In a text field only pictures are taken; text pastes as usual.
  document.addEventListener("paste", (e) => {
    if (!here() || !e.clipboardData) return;
    const t = document.activeElement && document.activeElement.tagName;
    const inField = /INPUT|TEXTAREA/.test(t || "");
    if (take(e.clipboardData, !inField)) e.preventDefault();
  });
  document.addEventListener("dragover", (e) => {
    if (!here()) return;
    e.preventDefault();
    document.body.classList.add("mind-dragging");
  });
  document.addEventListener("dragleave", (e) => { if (!e.relatedTarget) document.body.classList.remove("mind-dragging"); });
  document.addEventListener("drop", (e) => {
    document.body.classList.remove("mind-dragging");
    if (!here() || !e.dataTransfer) return;
    e.preventDefault();
    take(e.dataTransfer, true);
  });

  // "/" focuses search on desktop.
  document.addEventListener("keydown", (e) => {
    if (e.key !== "/" || e.ctrlKey || e.metaKey || e.altKey) return;
    const t = document.activeElement && document.activeElement.tagName;
    if (/INPUT|TEXTAREA|SELECT/.test(t || "") || !here()) return;
    const s = document.getElementById("mind-search");
    if (s) { e.preventDefault(); s.focus(); }
  });

  // The Paste button (phones have no Ctrl+V).
  M.pasteButton = async () => {
    try {
      if (navigator.clipboard && navigator.clipboard.read) {
        const list = await navigator.clipboard.read();
        for (const it of list) {
          const type = it.types.find((t) => t.startsWith("image/"));
          if (type) { const blob = await it.getType(type); M.upload(new File([blob], "paste", { type })); return; }
        }
      }
      const text = navigator.clipboard ? (await navigator.clipboard.readText()).trim() : "";
      if (/^https?:\/\/\S+$/.test(text)) M.send({ type: "paste-url", url: text });
      else if (text) M.send({ type: "paste-text", text });
      else M.send({ type: "error", message: "The clipboard is empty." });
    } catch (_) {
      M.send({ type: "error", message: "Paste needs clipboard permission. Long-press the field and paste instead." });
    }
  };
}
