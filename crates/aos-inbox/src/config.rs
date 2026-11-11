//! Inbox configuration: `~/.aos/inbox.toml` (or `AOS_INBOX_CONFIG`), every field optional.
//!
//! ```toml
//! [whatsapp]
//! enabled = true
//! account = "main"            # label stored on each item
//! wacli_bin = "D:/aos-ml/bin/wacli.exe"
//! store_dir = "D:/aos-ml/wacli-store"
//!
//! [[gmail]]                   # one block per Gmail account
//! name = "main"
//! config_dir = ""             # GOOGLE_WORKSPACE_CLI_CONFIG_DIR for this account; empty = gws default
//! query = "in:inbox newer_than:2d"
//!
//! [triage]
//! threshold = 0.35            # score at or above this means "Nora should read it"
//!
//! [laya]
//! python = "D:/aos-ml/laya-venv/Scripts/python.exe"
//! hf_home = "D:/hf-cache"
//! port = 8799
//! ```
//!
//! Nothing personal is hardcoded: accounts and numbers come from this file, never from the repo.

use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct WhatsappConfig {
    pub enabled: bool,
    pub account: String,
    pub wacli_bin: Option<PathBuf>,
    pub store_dir: Option<PathBuf>,
    /// How often queued, confirmed replies are looked at.
    pub outbox_secs: u64,
}

impl Default for WhatsappConfig {
    fn default() -> Self {
        WhatsappConfig {
            enabled: true,
            account: "main".into(),
            wacli_bin: None,
            store_dir: None,
            outbox_secs: 2,
        }
    }
}

impl WhatsappConfig {
    pub fn bin(&self) -> PathBuf {
        self.wacli_bin
            .clone()
            .or_else(|| env_path("AOS_WACLI_BIN"))
            .or_else(|| ml_home().map(|d| d.join("bin").join(exe("wacli"))))
            .filter(|p| p.exists())
            .unwrap_or_else(|| PathBuf::from(exe("wacli")))
    }

    pub fn store(&self) -> PathBuf {
        self.store_dir
            .clone()
            .or_else(|| env_path("AOS_WACLI_STORE"))
            .or_else(|| ml_home().map(|d| d.join("wacli-store")))
            .unwrap_or_else(|| aos_core::paths::aos_home().join("wacli-store"))
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct GmailAccount {
    pub name: String,
    pub config_dir: Option<PathBuf>,
    pub query: String,
    /// Newest messages looked at per poll.
    pub max: u32,
    pub interval_secs: u64,
}

impl Default for GmailAccount {
    fn default() -> Self {
        GmailAccount {
            name: "main".into(),
            config_dir: None,
            query: "in:inbox newer_than:2d".into(),
            max: 15,
            interval_secs: 90,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct TriageConfig {
    pub enabled: bool,
    pub host: String,
    pub port: u16,
    pub timeout_ms: u64,
    pub threshold: f64,
}

impl Default for TriageConfig {
    fn default() -> Self {
        TriageConfig {
            enabled: true,
            host: "127.0.0.1".into(),
            port: 8799,
            timeout_ms: 4000,
            threshold: 0.35,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct LayaConfig {
    /// Start and supervise the Laya service as a child process.
    pub spawn: bool,
    pub python: Option<PathBuf>,
    pub hf_home: Option<PathBuf>,
    pub script: Option<PathBuf>,
}

impl Default for LayaConfig {
    fn default() -> Self {
        LayaConfig {
            spawn: true,
            python: None,
            hf_home: None,
            script: None,
        }
    }
}

impl LayaConfig {
    pub fn python_bin(&self) -> PathBuf {
        self.python
            .clone()
            .or_else(|| env_path("AOS_LAYA_PYTHON"))
            .or_else(|| {
                ml_home().map(|d| {
                    d.join("laya-venv").join(if cfg!(windows) {
                        "Scripts/python.exe"
                    } else {
                        "bin/python"
                    })
                })
            })
            .unwrap_or_else(|| PathBuf::from("python"))
    }

    pub fn hf(&self) -> Option<PathBuf> {
        self.hf_home
            .clone()
            .or_else(|| env_path("HF_HOME"))
            .or_else(|| {
                let d = PathBuf::from("D:/hf-cache");
                (cfg!(windows) && d.exists()).then_some(d)
            })
    }

    pub fn script_path(&self) -> PathBuf {
        self.script
            .clone()
            .or_else(|| env_path("AOS_LAYA_SERVICE"))
            .unwrap_or_else(|| {
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("laya-service")
                    .join("laya_service.py")
            })
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct InboxConfig {
    pub whatsapp: WhatsappConfig,
    pub gmail: Vec<GmailAccount>,
    pub triage: TriageConfig,
    pub laya: LayaConfig,
}

impl InboxConfig {
    pub fn path() -> PathBuf {
        env_path("AOS_INBOX_CONFIG").unwrap_or_else(|| aos_core::paths::aos_home().join("inbox.toml"))
    }

    /// Load the config file; a missing file gives the defaults (WhatsApp on, Gmail `main`).
    pub fn load() -> InboxConfig {
        Self::load_from(&Self::path())
    }

    pub fn load_from(path: &Path) -> InboxConfig {
        let mut cfg: InboxConfig = match std::fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text).unwrap_or_else(|e| {
                tracing::warn!("{}: {e}; using defaults", path.display());
                InboxConfig::default()
            }),
            Err(_) => InboxConfig::default(),
        };
        if cfg.gmail.is_empty() && !path.exists() {
            cfg.gmail.push(GmailAccount::default());
        }
        cfg
    }
}

/// The `gws` executable. The npm shim `gws.cmd` cannot take JSON arguments safely, so look for
/// the real binary inside the package.
pub fn find_gws() -> PathBuf {
    if let Some(p) = env_path("AOS_GWS_BIN") {
        return p;
    }
    if let Some(appdata) = std::env::var_os("APPDATA") {
        let p = PathBuf::from(appdata)
            .join("npm/node_modules/@googleworkspace/cli/node_modules/.bin_real")
            .join(exe("gws"));
        if p.exists() {
            return p;
        }
    }
    PathBuf::from(exe("gws"))
}

fn ml_home() -> Option<PathBuf> {
    env_path("AOS_ML_HOME").or_else(|| {
        let d = PathBuf::from("D:/aos-ml");
        (cfg!(windows) && d.exists()).then_some(d)
    })
}

fn env_path(k: &str) -> Option<PathBuf> {
    std::env::var_os(k)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

fn exe(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_partial_toml() {
        let c: InboxConfig = toml::from_str(
            "[[gmail]]\nname = \"second\"\nconfig_dir = \"/tmp/x\"\n[triage]\nthreshold = 0.5\n",
        )
        .unwrap();
        assert_eq!(c.gmail[0].name, "second");
        assert_eq!(c.gmail[0].interval_secs, 90);
        assert_eq!(c.triage.threshold, 0.5);
        assert!(c.whatsapp.enabled);
    }
}
