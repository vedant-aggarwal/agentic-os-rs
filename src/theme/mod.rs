//! Document head: the Aura stylesheet, self-hosted Geist, and the phone meta tags.
//! Tokens live in `assets/aura.css` as CSS custom properties. Dark only for now.

use dioxus::prelude::*;

pub const AURA_CSS: Asset = asset!("/assets/aura.css");
pub const LIVE_CSS: Asset = asset!("/assets/live.css");
pub const CALL_CSS: Asset = asset!("/assets/call.css");
pub const MISSION_CSS: Asset = asset!("/assets/mission.css");
pub const WAVE_CSS: Asset = asset!("/assets/wave.css");
pub const MIND_CSS: Asset = asset!("/assets/mind.css");

/// Geist and Geist Mono (OFL) are bundled so a phone with no network on first launch
/// renders the same face as the web build.
pub const GEIST: Asset = asset!("/assets/fonts/Geist-Variable.woff2");
pub const GEIST_MONO: Asset = asset!("/assets/fonts/GeistMono-Variable.woff2");

#[component]
pub fn Head() -> Element {
    rsx! {
        document::Title { "Agentic OS" }
        document::Meta { name: "viewport", content: "width=device-width, initial-scale=1, viewport-fit=cover" }
        document::Meta { name: "color-scheme", content: "dark" }
        document::Meta { name: "theme-color", content: "#050507" }
        document::Meta { name: "apple-mobile-web-app-capable", content: "yes" }
        document::Meta { name: "apple-mobile-web-app-status-bar-style", content: "black-translucent" }
        document::Style {
            r#"@font-face {{ font-family: "Geist"; src: url("{GEIST}") format("woff2"); font-weight: 100 900; font-display: swap; }}
@font-face {{ font-family: "Geist Mono"; src: url("{GEIST_MONO}") format("woff2"); font-weight: 100 900; font-display: swap; }}"#
        }
        document::Stylesheet { href: AURA_CSS }
        document::Stylesheet { href: LIVE_CSS }
        document::Stylesheet { href: CALL_CSS }
        document::Stylesheet { href: MISSION_CSS }
        document::Stylesheet { href: WAVE_CSS }
        document::Stylesheet { href: MIND_CSS }
    }
}
