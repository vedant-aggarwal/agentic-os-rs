//! Real-time 3D hero (three.js). The JS module lives in `assets/hero3d/hero3d.js`
//! (built from `hero3d-src/`); personal models and presets load at runtime from
//! `assets/personal/` (gitignored) and the scene falls back to placeholders without them.
//!
//! ```ignore
//! Hero3D { level: 4, private_mode: false, focused: None, on_goal_tap: move |g: Goal| nav(g) }
//! ```

use dioxus::prelude::*;

/// Copied as-is (no hashing) so the module and demo page keep their names.
pub const HERO3D_DIR: Asset = asset!("/assets/hero3d", AssetOptions::folder());
/// Created empty by `build.rs` when missing, so a fresh clone still builds.
pub const PERSONAL_DIR: Asset = asset!("/assets/personal", AssetOptions::folder());

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Goal {
    Physical,
    Money,
    Mind,
}

impl Goal {
    pub fn as_str(self) -> &'static str {
        match self {
            Goal::Physical => "Physical",
            Goal::Money => "Money",
            Goal::Mind => "Mind",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "physical" => Some(Goal::Physical),
            "money" => Some(Goal::Money),
            "mind" => Some(Goal::Mind),
            _ => None,
        }
    }
}

fn js_goal(g: Option<Goal>) -> String {
    match g {
        Some(g) => format!("'{}'", g.as_str()),
        None => "null".into(),
    }
}

/// Renders `<div id="hero3d-root">` and mounts the 3D scene into it.
/// `on_goal_tap` fires when a goal object is tapped (or Enter on a focused goal).
/// `on_focus_change` fires when the user swipes / arrows between goals inside the scene.
#[component]
pub fn Hero3D(
    level: u32,
    private_mode: bool,
    focused: Option<Goal>,
    on_goal_tap: EventHandler<Goal>,
    on_focus_change: Option<EventHandler<Option<Goal>>>,
    #[props(default = "hero3d-root".to_string())] id: String,
    #[props(default)] class: String,
    #[props(default = "auto".to_string())] quality: String,
) -> Element {
    let mut mounted = use_signal(|| false);

    // Mount once the div exists; the eval stays alive and streams events back.
    let el_id = id.clone();
    let mount = move |_| {
        if mounted() {
            return;
        }
        mounted.set(true);
        let init = format!(
            r#"
            const el = document.getElementById({id:?});
            window.__hero3dState = window.__hero3dState || {{ level: {level}, privateMode: {private_mode}, focused: {focused} }};
            const mod = await import({module:?});
            window.__hero3dMod = mod;
            el.addEventListener('hero3d:goal', (e) => dioxus.send('goal:' + e.detail.goal));
            el.addEventListener('hero3d:focus', (e) => dioxus.send('focus:' + (e.detail.goal || '')));
            await mod.mount(el, {{ ...window.__hero3dState, personalBase: {personal:?}, quality: {quality:?} }});
            mod.update(window.__hero3dState);
            await new Promise(() => {{}});
            "#,
            id = el_id,
            module = format!("{HERO3D_DIR}/hero3d.js"),
            personal = format!("{PERSONAL_DIR}/"),
            focused = js_goal(focused),
            quality = quality,
        );
        let mut ev = document::eval(&init);
        spawn(async move {
            while let Ok(msg) = ev.recv::<String>().await {
                if let Some(g) = msg.strip_prefix("goal:") {
                    if let Some(goal) = Goal::parse(g) {
                        on_goal_tap.call(goal);
                    }
                } else if let Some(g) = msg.strip_prefix("focus:") {
                    if let Some(cb) = on_focus_change {
                        cb.call(Goal::parse(g));
                    }
                }
            }
        });
    };

    // Level and private mode always pass through.
    use_effect(use_reactive!(|(level, private_mode)| {
        document::eval(&format!(
            "window.__hero3dState = Object.assign(window.__hero3dState || {{}}, {{ level: {level}, privateMode: {private_mode} }});
             window.__hero3dMod && window.__hero3dMod.update({{ level: {level}, privateMode: {private_mode} }});"
        ));
    }));
    // Focus only when the prop changes, so in-scene swipes are not undone by other updates.
    use_effect(use_reactive!(|focused| {
        let f = js_goal(focused);
        document::eval(&format!(
            "window.__hero3dState = Object.assign(window.__hero3dState || {{}}, {{ focused: {f} }});
             window.__hero3dMod && window.__hero3dMod.update({{ focused: {f} }});"
        ));
    }));

    use_drop(|| {
        document::eval("window.__hero3dMod && window.__hero3dMod.unmount(); window.__hero3dMod = null; window.__hero3dState = null;");
    });

    rsx! {
        div {
            id: "{id}",
            class: "hero3d {class}",
            style: "position:relative;width:100%;height:100%;min-height:320px;overflow:hidden;background:#050507;",
            onmounted: mount,
        }
    }
}
