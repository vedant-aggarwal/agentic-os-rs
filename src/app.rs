use std::{fmt, str::FromStr};

use dioxus::prelude::*;

use crate::components::{SheetHost, Sidebar, TabBar, Toast};
use crate::pages::{Hero3dDemo, Inbox, Life, Mission, NoraPage, PairLayer, Today, UnlockLayer, Work};
use crate::state::AppState;
use crate::theme::Head;

const LAYERS_JS: &str = include_str!("js/layers.js");

#[derive(Routable, Clone, PartialEq, Debug)]
#[rustfmt::skip]
pub enum Route {
    #[layout(Shell)]
        // Dev switches for screenshots: `?lock=off` skips the unlock screen,
        // `?mode=safe|private` opens in that mode.
        #[route("/?:lock&:mode")]
        Today { lock: Option<Switch>, mode: Option<Mode> },
        #[route("/inbox")]
        Inbox {},
        #[route("/life")]
        Life {},
        #[route("/work")]
        Work {},
        #[route("/mission")]
        Mission {},
    #[end_layout]
    // Standalone 3D hero for screenshots: /hero3d?level=12&private=true&focus=money
    #[route("/hero3d?:level&:private&:focus")]
    Hero3dDemo { level: Option<u32>, private: Option<bool>, focus: Option<String> },
}

#[component]
pub fn App() -> Element {
    rsx! {
        Router::<Route> {}
    }
}

/// Navigation, the layers above every page, and the unlock screen.
#[component]
fn Shell() -> Element {
    let route = use_route::<Route>();
    let (lock_param, mode_param) = match &route {
        Route::Today { lock, mode } => (*lock, *mode),
        _ => (None, None),
    };
    let state = use_context_provider(|| {
        AppState::new(lock_param != Some(Switch::Off), mode_param == Some(Mode::Safe))
    });
    let _live = crate::live::use_live_provider();
    let _voice = crate::components::voice::use_voice_provider(state);
    let _photo = crate::components::photo::use_photo_provider(state);
    use_live_toasts(state);
    use_layer_gestures(state);
    use_safe_mode_store(state, lock_param == Some(Switch::Off) && mode_param.is_none());

    let lock = (state.lock)();
    let safe = (state.safe)();

    rsx! {
        Head {}
        div { class: "app", class: if safe { "is-safe" }, "data-lock": lock.slug(),
            Sidebar {}
            div { class: "view", Outlet::<Route> {} }
            TabBar {}
            NoraPage {}
            SheetHost {}
            UnlockLayer {}
            PairLayer {}
            crate::call::CallRoot {}
            Toast {}
            crate::components::voice::VoiceHost {}
        }
    }
}

/// Toasts raised by the data layer (a write the server refused).
fn use_live_toasts(mut state: AppState) {
    use_effect(move || {
        if let Some(t) = crate::live::TOAST.read().clone() {
            state.toast(t);
        }
    });
}

/// Messages from the layer gestures (drag a sheet down, swipe back, Escape).
fn use_layer_gestures(mut state: AppState) {
    use_effect(move || {
        spawn(async move {
            let mut eval = document::eval(LAYERS_JS);
            while let Ok(msg) = eval.recv::<String>().await {
                match msg.as_str() {
                    "close:sheet" => state.close_sheet(),
                    "close:nora" => state.nora.set(false),
                    "close:goal" => state.goal.set(None),
                    "close:pillar" => state.pillar.set(None),
                    "escape" => state.escape(),
                    _ => {}
                }
            }
        });
    });
}

/// Safe mode survives a restart. The unlock screen asks again each launch; the stored
/// value matters when that screen is skipped and for the native pieces that come later.
fn use_safe_mode_store(mut state: AppState, restore: bool) {
    use_effect(move || {
        if restore {
            // Created before the first write below, so it reads the old value.
            let read = document::eval("try { return localStorage.getItem('aos.safe') || ''; } catch (e) { return ''; }");
            spawn(async move {
                if let Ok(v) = read.join::<String>().await {
                    if v == "1" {
                        state.set_safe(true);
                    }
                }
            });
        }
    });
    use_effect(move || {
        let safe = (state.safe)();
        let _ = document::eval(&format!(
            "try {{ localStorage.setItem('aos.safe', '{}'); }} catch (e) {{}}",
            if safe { 1 } else { 0 }
        ));
    });
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Switch {
    #[default]
    On,
    Off,
}

impl FromStr for Switch {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "on" => Ok(Switch::On),
            "off" => Ok(Switch::Off),
            other => Err(format!("unknown switch '{other}'")),
        }
    }
}

impl fmt::Display for Switch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Switch::On => "on",
            Switch::Off => "off",
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Mode {
    Safe,
    #[default]
    Private,
}

impl FromStr for Mode {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "safe" => Ok(Mode::Safe),
            "private" => Ok(Mode::Private),
            other => Err(format!("unknown mode '{other}'")),
        }
    }
}

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Mode::Safe => "safe",
            Mode::Private => "private",
        })
    }
}
