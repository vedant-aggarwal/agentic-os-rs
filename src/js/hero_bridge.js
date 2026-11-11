// Forwards 2.5D hero events to Rust and wires the three.js hero's DOM events.
// The 2.5D stage dispatches these on #hero-stage:
//   aos:goal-tap  detail { goal: "physical" | "money" | "mind" }
//   aos:self-tap  (his character)
//   aos:front     detail { item: "me" | "physical" | "money" | "mind" }
// `aos:front` sets data-front on the nearest [data-front-scope], which picks the
// caption and the ambient colour in CSS.
// Runs inside a Dioxus eval: `dioxus` is the channel to Rust.

const root = document.getElementById("hero-stage");
if (root) {
  root.addEventListener("aos:goal-tap", (e) => {
    const goal = e.detail && e.detail.goal;
    if (goal) dioxus.send("goal:" + goal);
  });
  root.addEventListener("aos:self-tap", () => dioxus.send("self"));
  root.addEventListener("aos:front", (e) => {
    const item = e.detail && e.detail.item;
    const scope = root.closest("[data-front-scope]");
    if (scope && item) scope.dataset.front = item;
  });
  // The three.js hero (components/hero3d.rs): show it once it has drawn, keep the
  // 2.5D stage if WebGL fails, and follow its in-scene swipes for the stats row.
  const slot = root.querySelector(".hero3d-slot");
  // Without his presets (assets/personal/, gitignored) the 3D scene is a faceless
  // mannequin; the 2.5D stage with his real art is the better stand-in then.
  root.addEventListener("hero3d:ready", (e) => {
    if (!slot) return;
    const personal = !!(e.detail && e.detail.stats && e.detail.stats.personal);
    if (personal) slot.dataset.ready = "1";
    else slot.dataset.placeholder = "1";
  });
  root.addEventListener("hero3d:error", () => { if (slot) { delete slot.dataset.ready; slot.dataset.failed = "1"; } });
  root.addEventListener("hero3d:focus", (e) => {
    const g = e.detail && e.detail.goal;
    const scope = root.closest("[data-front-scope]");
    if (scope) scope.dataset.front = g ? String(g).toLowerCase() : "me";
  });
  await new Promise(() => {});
}
