//! Design tokens live in `assets/main.css` as CSS custom properties (light and dark).
//! This module owns the theme preference and the document head: stylesheet, font, title.

use dioxus::prelude::*;
use std::{fmt, str::FromStr};

pub const MAIN_CSS: Asset = asset!("/assets/main.css");

/// Hanken Grotesk (OFL) is self-hosted so phones with no network on first launch
/// and headless screenshots render the same face as the web build.
pub const FONT: Asset = asset!("/assets/fonts/HankenGrotesk-Variable.ttf");

/// `?theme=dark|light` overrides the OS setting; `System` follows it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ThemePref {
    #[default]
    System,
    Light,
    Dark,
}

impl ThemePref {
    /// Class for the shell element: tokens on it win over the `:root` media query.
    pub fn class(self) -> &'static str {
        match self {
            ThemePref::System => "",
            ThemePref::Light => "theme-light",
            ThemePref::Dark => "theme-dark",
        }
    }

    /// Mirrors the choice onto `<html>` so the page background and form controls follow.
    fn script(self) -> &'static str {
        match self {
            ThemePref::System => "delete document.documentElement.dataset.theme;",
            ThemePref::Light => "document.documentElement.dataset.theme = 'light';",
            ThemePref::Dark => "document.documentElement.dataset.theme = 'dark';",
        }
    }
}

impl FromStr for ThemePref {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "system" => Ok(ThemePref::System),
            "light" => Ok(ThemePref::Light),
            "dark" => Ok(ThemePref::Dark),
            other => Err(format!("unknown theme '{other}'")),
        }
    }
}

impl fmt::Display for ThemePref {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            ThemePref::System => "system",
            ThemePref::Light => "light",
            ThemePref::Dark => "dark",
        })
    }
}

#[component]
pub fn ThemeHead(theme: ReadSignal<ThemePref>) -> Element {
    // Runs on the client only; the server render never touches the DOM.
    use_effect(move || {
        let _ = document::eval(theme().script());
    });

    rsx! {
        document::Title { "Agentic OS" }
        document::Meta { name: "viewport", content: "width=device-width, initial-scale=1, viewport-fit=cover" }
        document::Meta { name: "color-scheme", content: "light dark" }
        document::Stylesheet { href: MAIN_CSS }
        document::Style {
            r#"@font-face {{ font-family: "Hanken Grotesk"; src: url("{FONT}") format("truetype"); font-weight: 100 900; font-display: swap; }}"#
        }
    }
}
