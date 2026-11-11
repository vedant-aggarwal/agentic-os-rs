//! The Swift side: `src/ios/push` (a SwiftPM package). `dx bundle` finds it through the
//! `#[manganis::ffi]` metadata below, builds it into `Frameworks/DioxusSwiftPlugins.framework`,
//! and the generated `AosPushPlugin::new()` loads it with dlopen. If it is missing, push is off and
//! the app runs as before.

#![allow(non_snake_case)]

use std::sync::OnceLock;

#[manganis::ffi("src/ios/push")]
extern "Swift" {
    pub type AosPushPlugin;
    pub fn startJson(this: &AosPushPlugin, optionsJson: String) -> String;
    pub fn pollJson(this: &AosPushPlugin) -> String;
    pub fn syncAlarmsJson(this: &AosPushPlugin, alarmsJson: String) -> String;
}

fn plugin() -> Option<&'static AosPushPlugin> {
    static P: OnceLock<Option<AosPushPlugin>> = OnceLock::new();
    P.get_or_init(|| match AosPushPlugin::new() {
        Ok(p) => Some(p),
        Err(e) => {
            eprintln!("[aos-push] native plugin unavailable: {e}");
            None
        }
    })
    .as_ref()
}

pub fn available() -> bool {
    plugin().is_some()
}

/// `start` / `poll` / `sync`, returning the plugin's JSON.
pub fn call(what: &str, arg: &str) -> Option<String> {
    let p = plugin()?;
    let r = match what {
        "start" => startJson(p, arg.to_string()),
        "poll" => pollJson(p),
        "sync" => syncAlarmsJson(p, arg.to_string()),
        _ => return None,
    };
    r.map_err(|e| eprintln!("[aos-push] {what}: {e}")).ok()
}
