// Photo of the marked-up morning sheet: pick or take a picture, shrink it, send it to the server.
// Runs inside a Dioxus eval: `dioxus` is the channel to Rust.
//
// A label wraps <input type="file" data-aos-photo>, so the tap itself opens the camera (iOS and
// Safari only allow that from a real tap). One document-level listener handles every such input.
// The image is cut to 2000 px on the long side and sent as JPEG: POST /api/nora/photo with the
// device token. The server reads it with Claude, applies the marks and posts a summary in the
// Nora thread. Messages to Rust: {type:"state", busy}, {type:"done", summary}, {type:"error", message}.

const P = (window.__aosPhoto = window.__aosPhoto || {});
P.send = (m) => { try { dioxus.send(m); } catch (_) {} };
P.cfg = P.cfg || { url: "", token: "" };
P.config = (c) => { P.cfg = { ...P.cfg, ...c }; };

if (!P.installed) {
  P.installed = true;
  const MAX = 2000;
  const shrink = async (file) => {
    try {
      const bmp = await createImageBitmap(file);
      const k = Math.min(1, MAX / Math.max(bmp.width, bmp.height));
      const c = document.createElement("canvas");
      c.width = Math.round(bmp.width * k);
      c.height = Math.round(bmp.height * k);
      c.getContext("2d").drawImage(bmp, 0, 0, c.width, c.height);
      const blob = await new Promise((res) => c.toBlob(res, "image/jpeg", 0.85));
      if (blob && blob.size > 0) return blob;
    } catch (_) {}
    return file; // the server accepts JPEG, PNG and WebP as they are
  };
  document.addEventListener("change", async (e) => {
    const input = e.target;
    if (!input || !input.matches || !input.matches("input[data-aos-photo]")) return;
    const file = input.files && input.files[0];
    input.value = "";
    if (!file) return;
    P.send({ type: "state", busy: true });
    try {
      const blob = await shrink(file);
      const base = (P.cfg.url || "").replace(/\/+$/, "");
      const r = await fetch(`${base}/api/nora/photo`, {
        method: "POST",
        headers: { Authorization: "Bearer " + P.cfg.token, "Content-Type": blob.type || "image/jpeg" },
        body: blob,
      });
      const j = await r.json().catch(() => ({}));
      if (r.status === 401) P.send({ type: "error", message: "The server refused this device. Re-pair in You." });
      else if (!r.ok) P.send({ type: "error", message: (j.error && j.error.message) || `HTTP ${r.status}` });
      else P.send({ type: "done", summary: j.summary || "" });
    } catch (err) {
      P.send({ type: "error", message: "Could not reach the server. Try again." });
    }
    P.send({ type: "state", busy: false });
  }, true);
}
