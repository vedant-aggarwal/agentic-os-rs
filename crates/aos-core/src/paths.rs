//! Default file locations. All of them live outside the repo.

use std::path::PathBuf;

/// `%USERPROFILE%\.aos` (or `~/.aos`).
pub fn aos_home() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".aos")
}

/// The database, unless `AOS_DB` says otherwise.
pub fn default_db_path() -> PathBuf {
    std::env::var_os("AOS_DB")
        .map(PathBuf::from)
        .unwrap_or_else(|| aos_home().join("aos.db"))
}

/// Personal seed data (habits, areas). Never committed; loaded when present.
pub fn local_seed_path() -> PathBuf {
    std::env::var_os("AOS_SEED")
        .map(PathBuf::from)
        .unwrap_or_else(|| aos_home().join("seed.local.toml"))
}

/// Images for boards (Mission Control), unless `AOS_MEDIA` says otherwise. Never in the repo.
pub fn media_dir() -> PathBuf {
    std::env::var_os("AOS_MEDIA")
        .map(PathBuf::from)
        .unwrap_or_else(|| aos_home().join("media"))
}
