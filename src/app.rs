use dioxus::prelude::*;

use crate::components::nav::{Dock, Rail};
use crate::components::overlay::OverlayHost;
use crate::data::DayState;
use crate::pages::{Brain, Nora, Today, Work};
use crate::state::ShellState;
use crate::theme::{ThemeHead, ThemePref};

#[derive(Routable, Clone, PartialEq, Debug)]
#[rustfmt::skip]
pub enum Route {
    #[layout(Shell)]
        // `?state=morning|day|night` and `?theme=light|dark` are dev switches for screenshots.
        #[route("/?:state&:theme")]
        Today { state: Option<DayState>, theme: Option<ThemePref> },
        #[route("/work")]
        Work {},
        #[route("/nora")]
        Nora {},
        #[route("/brain")]
        Brain {},
}

#[component]
pub fn App() -> Element {
    rsx! {
        Router::<Route> {}
    }
}

/// Rail or tab bar, the pinned capture row, and the overlays. Wraps every page.
#[component]
fn Shell() -> Element {
    let route = use_route::<Route>();
    let shell = use_context_provider(ShellState::new);

    // Other tabs keep the theme of the last Today view.
    let theme = match &route {
        Route::Today { theme, .. } => theme.unwrap_or_default(),
        _ => (shell.today_params)().1.unwrap_or_default(),
    };

    rsx! {
        ThemeHead { theme }
        RememberToday { route: route.clone() }
        div { class: "shell {theme.class()}",
            Rail {}
            div { class: "content", Outlet::<Route> {} }
            Dock {}
            OverlayHost {}
        }
    }
}

/// Stores the Today dev params after each navigation, so the Today tab returns to the same view.
#[component]
fn RememberToday(route: ReadSignal<Route>) -> Element {
    let mut shell = use_context::<ShellState>();
    use_effect(move || {
        if let Route::Today { state, theme } = route() {
            shell.today_params.set((state, theme));
        }
    });
    rsx! {}
}
