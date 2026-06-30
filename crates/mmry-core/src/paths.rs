//! Zero-dependency base-directory resolution (option-B semantics).
//!
//! Resolution rules, applied per kind (config/data/state/cache):
//! 1. An explicit, *absolute* `XDG_*` env var always wins, on any OS.
//! 2. Otherwise on unix (incl. macOS) use the XDG-style `~/.config`,
//!    `~/.local/share`, `~/.local/state`, `~/.cache` layout — never
//!    `~/Library/Application Support`, which is undesirable for a CLI.
//! 3. Otherwise on Windows use `%APPDATA%` (config/data) or
//!    `%LOCALAPPDATA%` (state/cache).
//!
//! The app name is then joined by the caller.

use std::path::PathBuf;

/// Core resolution logic, kept pure for testing.
fn resolve_base(
    xdg: Option<PathBuf>,
    home: Option<PathBuf>,
    win_dir: Option<PathBuf>,
    is_windows: bool,
    unix_rel: &str,
) -> Option<PathBuf> {
    if let Some(p) = xdg.filter(|p| p.is_absolute()) {
        return Some(p);
    }
    if is_windows {
        win_dir
    } else {
        home.map(|h| h.join(unix_rel))
    }
}

/// Resolve a base directory for the given XDG var / unix-relative path / Windows var.
fn base_dir(xdg_var: &str, unix_rel: &str, win_var: &str) -> anyhow::Result<PathBuf> {
    resolve_base(
        std::env::var_os(xdg_var).map(PathBuf::from),
        std::env::var_os("HOME").map(PathBuf::from),
        std::env::var_os(win_var).map(PathBuf::from),
        cfg!(windows),
        unix_rel,
    )
    .ok_or_else(|| anyhow::anyhow!("unable to determine base directory ({xdg_var})"))
}

/// Base config directory (e.g. `~/.config` on unix).
pub fn config_base() -> anyhow::Result<PathBuf> {
    base_dir("XDG_CONFIG_HOME", ".config", "APPDATA")
}

/// Base data directory (e.g. `~/.local/share` on unix).
pub fn data_base() -> anyhow::Result<PathBuf> {
    base_dir("XDG_DATA_HOME", ".local/share", "APPDATA")
}

/// Base state directory (e.g. `~/.local/state` on unix).
pub fn state_base() -> anyhow::Result<PathBuf> {
    base_dir("XDG_STATE_HOME", ".local/state", "LOCALAPPDATA")
}

/// Base cache directory (e.g. `~/.cache` on unix).
pub fn cache_base() -> anyhow::Result<PathBuf> {
    base_dir("XDG_CACHE_HOME", ".cache", "LOCALAPPDATA")
}

/// Resolve the user's home directory from `$HOME` (no platform fallbacks).
pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn absolute_xdg_wins_on_unix() {
        let got = resolve_base(
            Some(PathBuf::from("/explicit/cfg")),
            Some(PathBuf::from("/home/user")),
            Some(PathBuf::from("C:/Users/u/AppData/Roaming")),
            false,
            ".config",
        );
        assert_eq!(got, Some(PathBuf::from("/explicit/cfg")));
    }

    #[test]
    fn absolute_xdg_wins_on_windows() {
        let got = resolve_base(
            Some(PathBuf::from("/explicit/cfg")),
            Some(PathBuf::from("/home/user")),
            Some(PathBuf::from("C:/Users/u/AppData/Roaming")),
            true,
            ".config",
        );
        assert_eq!(got, Some(PathBuf::from("/explicit/cfg")));
    }

    #[test]
    fn relative_xdg_is_ignored() {
        // A non-absolute XDG value must not be honored; fall back to home.
        let got = resolve_base(
            Some(PathBuf::from("relative/cfg")),
            Some(PathBuf::from("/home/user")),
            None,
            false,
            ".config",
        );
        assert_eq!(got, Some(PathBuf::from("/home/user/.config")));
    }

    #[test]
    fn unix_uses_home_join_rel_not_library() {
        let got = resolve_base(
            None,
            Some(PathBuf::from("/home/user")),
            None,
            false,
            ".local/share",
        );
        assert_eq!(got, Some(PathBuf::from("/home/user/.local/share")));
    }

    #[test]
    fn windows_uses_win_dir() {
        let got = resolve_base(
            None,
            Some(PathBuf::from("C:/Users/u")),
            Some(PathBuf::from("C:/Users/u/AppData/Local")),
            true,
            ".cache",
        );
        assert_eq!(got, Some(PathBuf::from("C:/Users/u/AppData/Local")));
    }

    #[test]
    fn unix_without_home_is_none() {
        let got = resolve_base(None, None, None, false, ".config");
        assert_eq!(got, None);
    }

    #[test]
    fn windows_without_win_dir_is_none() {
        let got = resolve_base(
            None,
            Some(PathBuf::from("C:/Users/u")),
            None,
            true,
            ".cache",
        );
        assert_eq!(got, None);
    }
}
