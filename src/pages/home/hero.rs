//! The hero: his character on a stone pedestal, his goals on pedestals either side.
//!
//! `HeroStage` holds both heroes: the three.js `Hero3D` (from main) in a slot that
//! fades in once its first frame is ready, and the 2.5D image stage, which shows while
//! the 3D module loads, while the app is locked, and for good if WebGL fails.

use dioxus::prelude::*;

use crate::components::hero3d::{Goal as Goal3d, Hero3D};
use crate::data::goal::Goal;
use crate::data::levels::{companion_vivid, Look};

const IMG_CAVE: Asset = asset!("/assets/img/cave.webp");
const IMG_PEDESTAL: Asset = asset!("/assets/img/pedestal.webp");
const IMG_PEDESTAL_SOFT: Asset = asset!("/assets/img/pedestal-soft.webp");
const IMG_BIKE: Asset = asset!("/assets/img/bike.webp");
const IMG_MACBOOK: Asset = asset!("/assets/img/macbook.webp");
const IMG_COMPANION: Asset = asset!("/assets/img/companion.webp");
const IMG_L1: Asset = asset!("/assets/img/me-l1.webp");
const IMG_L5: Asset = asset!("/assets/img/me-l5.webp");
const IMG_L10: Asset = asset!("/assets/img/me-l10.webp");
const IMG_L20: Asset = asset!("/assets/img/me-l20.webp");

/// Pre-blurred copies (offline, see HOME-V2-BUILD.md): `soft` for depth of field at
/// the flanks, `halo` for the glow behind each item. (sharp, soft, halo)
fn layers(kind: Kind, look: Look) -> (Asset, Asset, Asset) {
    match kind {
        Kind::Me => match look {
            Look::L1 => (IMG_L1, asset!("/assets/img/me-l1-soft.webp"), asset!("/assets/img/me-l1-halo.webp")),
            Look::L5 => (IMG_L5, asset!("/assets/img/me-l5-soft.webp"), asset!("/assets/img/me-l5-halo.webp")),
            Look::L10 => (IMG_L10, asset!("/assets/img/me-l10-soft.webp"), asset!("/assets/img/me-l10-halo.webp")),
            Look::L20 => (IMG_L20, asset!("/assets/img/me-l20-soft.webp"), asset!("/assets/img/me-l20-halo.webp")),
        },
        Kind::Goal(Goal::Mind) => (IMG_COMPANION, asset!("/assets/img/companion-soft.webp"), asset!("/assets/img/companion-halo.webp")),
        Kind::Goal(Goal::Physical) => (IMG_BIKE, asset!("/assets/img/bike-soft.webp"), asset!("/assets/img/bike-halo.webp")),
        Kind::Goal(Goal::Money) => (IMG_MACBOOK, asset!("/assets/img/macbook-soft.webp"), asset!("/assets/img/macbook-halo.webp")),
    }
}

/// How brightly an item glows: more progress, more light. Not shown as a number.
fn glow(kind: Kind, level: u32) -> f32 {
    let lvl = match kind {
        Kind::Me => level as f32,
        Kind::Goal(g) => g.stat_level() as f32,
    };
    (0.3 + lvl * 0.12).min(1.0)
}
pub const FACE_L1: Asset = asset!("/assets/img/me-l1-face.webp");
pub const FACE_L5: Asset = asset!("/assets/img/me-l5-face.webp");
pub const FACE_L10: Asset = asset!("/assets/img/me-l10-face.webp");
pub const FACE_L20: Asset = asset!("/assets/img/me-l20-face.webp");
pub const FACE_COMPANION: Asset = asset!("/assets/img/companion-face.webp");

const HERO25D_JS: &str = include_str!("../../js/hero25d.js");
const HERO_BRIDGE_JS: &str = include_str!("../../js/hero_bridge.js");

pub fn look_face(look: Look) -> Asset {
    match look {
        Look::L1 => FACE_L1,
        Look::L5 => FACE_L5,
        Look::L10 => FACE_L10,
        Look::L20 => FACE_L20,
    }
}

pub fn goal_img(goal: Goal) -> Asset {
    match goal {
        Goal::Physical => IMG_BIKE,
        Goal::Money => IMG_MACBOOK,
        Goal::Mind => IMG_COMPANION,
    }
}

fn to_3d(goal: Goal) -> Goal3d {
    match goal {
        Goal::Physical => Goal3d::Physical,
        Goal::Money => Goal3d::Money,
        Goal::Mind => Goal3d::Mind,
    }
}

fn from_3d(goal: Goal3d) -> Goal {
    match goal {
        Goal3d::Physical => Goal::Physical,
        Goal3d::Money => Goal::Money,
        Goal3d::Mind => Goal::Mind,
    }
}

/// Both heroes, one interface. Taps from the 2.5D stage arrive as DOM events on
/// `#hero-stage`; taps in the 3D scene come through `Hero3D`'s own channel.
#[component]
pub fn HeroStage(
    level: u32,
    private_mode: bool,
    focused: Option<Goal>,
    on_goal_tap: EventHandler<Goal>,
    /// His character was tapped (2.5D stage only).
    on_self_tap: EventHandler<()>,
) -> Element {
    use_effect(move || {
        spawn(async move {
            let mut bridge = document::eval(HERO_BRIDGE_JS);
            while let Ok(msg) = bridge.recv::<String>().await {
                if msg == "self" {
                    on_self_tap.call(());
                } else if let Some(goal) = msg.strip_prefix("goal:").and_then(Goal::from_slug) {
                    on_goal_tap.call(goal);
                }
            }
        });
    });

    rsx! {
        div { id: "hero-stage", class: "hero-stage",
            div { class: "hero3d-slot",
                Hero3D {
                    level,
                    private_mode,
                    focused: focused.map(to_3d),
                    on_goal_tap: move |g: Goal3d| on_goal_tap.call(from_3d(g)),
                }
            }
            Hero25D { level, private_mode, focused }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Me,
    Goal(Goal),
}

impl Kind {
    pub fn slug(self) -> &'static str {
        match self {
            Kind::Me => "me",
            Kind::Goal(g) => g.slug(),
        }
    }
}

/// Ring order. Bullet left and MacBook right at rest; in private mode the companion
/// stands beside him (the back slot of a four-item ring is drawn beside the front one).
fn stage_items(private_mode: bool) -> &'static [Kind] {
    const PRIVATE: [Kind; 4] = [Kind::Me, Kind::Goal(Goal::Money), Kind::Goal(Goal::Mind), Kind::Goal(Goal::Physical)];
    const SAFE: [Kind; 3] = [Kind::Me, Kind::Goal(Goal::Money), Kind::Goal(Goal::Physical)];
    if private_mode {
        &PRIVATE
    } else {
        &SAFE
    }
}

/// Nearest ring position that puts item `idx` in front.
fn nearest_spin(cur: i32, idx: usize, n: usize) -> i32 {
    let n = n as i32;
    let d = (idx as i32 - cur).rem_euclid(n);
    cur + if d * 2 > n { d - n } else { d }
}

#[component]
fn Hero25D(level: u32, private_mode: bool, focused: Option<Goal>) -> Element {
    let items = stage_items(private_mode);
    let mut spin = use_signal(|| 0i32);
    let mut shown = use_signal(|| items);

    // Keep the same item in front when safe mode adds or removes the companion,
    // and bring a focused goal to the front.
    use_effect(use_reactive!(|private_mode, focused| {
        let items = stage_items(private_mode);
        let before = *shown.peek();
        let mut s = *spin.peek();
        if before.len() != items.len() {
            let front = before[s.rem_euclid(before.len() as i32) as usize];
            let idx = items.iter().position(|k| *k == front).unwrap_or(0);
            s = nearest_spin(s, idx, items.len());
            shown.set(items);
        }
        if let Some(goal) = focused {
            if let Some(idx) = items.iter().position(|k| *k == Kind::Goal(goal)) {
                s = nearest_spin(s, idx, items.len());
            }
        }
        if s != *spin.peek() {
            spin.set(s);
        }
    }));

    // The stage script follows the finger and reports where a swipe settled.
    use_effect(move || {
        spawn(async move {
            let mut stage = document::eval(HERO25D_JS);
            while let Ok(msg) = stage.recv::<String>().await {
                if let Some(v) = msg.strip_prefix("spin:").and_then(|v| v.parse::<i32>().ok()) {
                    spin.set(v);
                }
            }
        });
    });

    let n = items.len();
    let s = spin();
    let vivid = companion_vivid(level as u8);

    rsx! {
        section {
            id: "stage",
            class: "stage",
            "data-n": "{n}",
            "data-spin": "{s}",
            "data-focus": focused.map(Goal::slug).unwrap_or(""),
            aria_label: "Your character and goals. Swipe to turn the stage, tap a goal to open it.",
            div { class: "st-far", aria_hidden: "true",
                img { class: "st-backdrop", src: IMG_CAVE, alt: "", draggable: "false" }
                for k in ["me", "physical", "money", "mind"] {
                    span { key: "{k}", class: "st-tint st-tint-{k}" }
                }
                span { class: "st-backdrop-fade" }
            }
            div { class: "st-items",
                for (idx, kind) in items.iter().copied().enumerate() {
                    StageItem { key: "{kind.slug()}", idx, kind, n, spin: s, level, vivid }
                }
            }
            div { class: "st-near", aria_hidden: "true",
                for i in 0..9 {
                    span { key: "{i}", class: "mote", style: "--i: {i}" }
                }
            }
        }
    }
}

#[component]
fn StageItem(idx: usize, kind: Kind, n: usize, spin: i32, level: u32, vivid: f32) -> Element {
    // Written once for the first paint. After mount the stage script owns these styles,
    // so they must not change between renders (Dioxus only writes changed attributes).
    let (style, ped) = use_hook(|| rest_style(kind, idx, n, spin));
    let look = Look::at(level as u8);

    let label = match kind {
        Kind::Me => format!("You, level {level}. Open the level roadmap"),
        Kind::Goal(Goal::Mind) => "Mind goal. Open".to_string(),
        Kind::Goal(g) => format!("{} goal: {}. Open", g.pillar(), g.spec().name),
    };

    let figure = matches!(kind, Kind::Me | Kind::Goal(Goal::Mind));
    let (sharp, soft, halo) = layers(kind, look);
    let fig_style = if kind == Kind::Goal(Goal::Mind) {
        format!("--src: url({sharp}); --vivid: {vivid:.2}")
    } else {
        format!("--src: url({sharp})")
    };
    let g = glow(kind, level);

    rsx! {
        div {
            class: "st-item st-{kind.slug()}",
            "data-kind": kind.slug(),
            "data-hue": kind.slug(),
            "data-idx": "{idx}",
            role: "button",
            tabindex: "0",
            aria_label: "{label}",
            style: "{style}",
            div { class: "st-reveal", style: "--glow: {g:.2}",
                if figure {
                    img { class: "st-ped", src: IMG_PEDESTAL, alt: "", draggable: "false", style: "{ped}" }
                    img { class: "st-ped-soft", src: IMG_PEDESTAL_SOFT, alt: "", draggable: "false" }
                    span { class: "st-glow", aria_hidden: "true" }
                }
                div { class: "st-fig", style: "{fig_style}",
                    img { class: "st-halo", src: halo, alt: "", draggable: "false" }
                    img { class: "st-sharp", src: sharp, alt: "", draggable: "false" }
                    img { class: "st-soft", src: soft, alt: "", draggable: "false" }
                    span { class: "st-sheen" }
                }
                if !figure {
                    span { class: "st-glow", aria_hidden: "true" }
                }
            }
        }
    }
}

#[derive(Clone, Copy)]
struct Geo {
    x: f32,
    y: f32,
    s: f32,
    o: f32,
    z: f32,
    p: f32,
    r: f32,
}

fn lerp(a: Geo, b: Geo, t: f32) -> Geo {
    let m = |p: f32, q: f32| p + (q - p) * t;
    Geo { x: m(a.x, b.x), y: m(a.y, b.y), s: m(a.s, b.s), o: m(a.o, b.o), z: m(a.z, b.z), p: m(a.p, b.p), r: m(a.r, b.r) }
}

fn wrap(o: f32, n: usize) -> f32 {
    let n = n as f32;
    let o = o.rem_euclid(n);
    if o > n / 2.0 {
        o - n
    } else {
        o
    }
}

/// Slot geometry for an item `o` slots from the front. Keep in step with `geom` in
/// src/js/hero25d.js. x is a share of the stage width, y of its height.
fn geom(kind: Kind, o: f32, n: usize) -> Geo {
    let a = o.abs();
    let sg = if o < 0.0 { -1.0 } else { 1.0 };
    let front = Geo { x: 0.0, y: 0.0, s: 1.0, o: 1.0, z: 10.0, p: 1.0, r: 0.0 };
    let (x1, s1) = if n == 4 { (0.335, 0.56) } else { (0.31, 0.6) };
    let flank = Geo { x: sg * x1, y: -0.03, s: s1, o: 0.82, z: 5.0, p: 1.0, r: -sg * 12.0 };
    let figure = matches!(kind, Kind::Me | Kind::Goal(Goal::Mind));
    let (back, end) = if n == 4 && figure {
        let side = if kind == Kind::Me { -1.0 } else { 1.0 };
        (Geo { x: side * 0.16, y: -0.02, s: 0.84, o: 0.9, z: 7.0, p: 0.0, r: -side * 10.0 }, 2.0)
    } else if n == 4 {
        (Geo { x: 0.0, y: -0.08, s: 0.42, o: 0.0, z: 0.0, p: 1.0, r: 0.0 }, 2.0)
    } else {
        (Geo { x: 0.0, y: -0.075, s: 0.42, o: 0.0, z: 0.0, p: 1.0, r: 0.0 }, 1.5)
    };
    if a <= 1.0 {
        lerp(front, flank, a)
    } else {
        lerp(flank, back, (a - 1.0) / (end - 1.0))
    }
}

fn rest_style(kind: Kind, idx: usize, n: usize, spin: i32) -> (String, String) {
    let g = geom(kind, wrap(idx as f32 - spin as f32, n), n);
    let figure = matches!(kind, Kind::Me | Kind::Goal(Goal::Mind));
    let r = if figure { g.r } else { g.r * 0.35 };
    let item = format!(
        "transform: translate3d(calc(-50% + {:.3}cqw), {:.3}cqh, 0) scale({:.4}) rotateY({:.2}deg); opacity: {:.3}; z-index: {}",
        g.x * 100.0,
        g.y * 100.0,
        g.s,
        r,
        g.o,
        g.z.round() as i32
    );
    (item, format!("opacity: {:.3}", g.p))
}
