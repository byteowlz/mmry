use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use std::path::Path;
use std::path::PathBuf;

const CONFIG_FILE: &str = "config.toml";

/// Environment override for [`Config::state_root`].
pub const ENV_STATE_ROOT: &str = "MMRY_STATE_ROOT";
/// Environment override for [`Config::migrate`] (`auto`, `prompt`, `off`).
pub const ENV_MIGRATE: &str = "MMRY_MIGRATE";

/// Commented config written on first run when no global config exists.
pub const DEFAULT_CONFIG: &str = "\
#:schema https://raw.githubusercontent.com/byteowlz/schemas/refs/heads/main/mmry/mmry.config.schema.json

# Central per-user store: general/ and repos/<name>/ ledgers.
# state_root = \"~/.local/state/mmry\"

# Repo-local .mmry/mmry.jsonl ledgers that belong in the central store:
# \"prompt\" (default: ask on a terminal, otherwise warn and continue),
# \"auto\" (migrate on first use; `mmry setup` sets this) or \"off\" (warn only).
# Repos with .mmry/tracked or a git-committed ledger stay repo-local.
# migrate = \"prompt\"

# Git sync of the state root; set up with `mmry sync init --remote URL`.
# [sync]
# auto_pull = false      # pull at session start (mmry preview)
# auto_commit = false    # commit after every write
# auto_push = false      # push after an automatic commit
# timeout_secs = 10

# Bounded directories searched by cross-repository commands (`--all`, `--repo`).
# Discovery never searches the home directory unless it is listed here.
#
# [[roots]]
# path = \"~/byteowlz\"
# max_depth = 2
";

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Per-user central store (general + per-repo ledgers). Defaults to
    /// `$XDG_STATE_HOME/mmry`.
    pub state_root: Option<PathBuf>,
    /// What to do when a repository still has a repo-local ledger that belongs
    /// in the central store.
    pub migrate: MigrateMode,
    /// Bounded directories searched by cross-repository commands.
    pub roots: Vec<DiscoveryRoot>,
    /// Git sync of the state root (`mmry sync init` sets it up).
    pub sync: SyncConfig,
}

/// Automatic git sync; all off by default. Failures only warn.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct SyncConfig {
    /// Pull at session start (`mmry preview`).
    pub auto_pull: bool,
    /// Commit the store after every write.
    pub auto_commit: bool,
    /// Push after an automatic commit.
    pub auto_push: bool,
    /// Upper bound for each git network operation, in seconds.
    pub timeout_secs: u64,
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            auto_pull: false,
            auto_commit: false,
            auto_push: false,
            timeout_secs: 10,
        }
    }
}

/// Handling of repo-local ledgers in central mode.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MigrateMode {
    /// Migrate automatically on first use in that repository.
    Auto,
    /// Ask on a terminal; otherwise warn and continue without migrating.
    #[default]
    Prompt,
    /// Never migrate automatically; warn and use the central store only.
    Off,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DiscoveryRoot {
    pub path: PathBuf,
    #[serde(default = "default_max_depth")]
    pub max_depth: usize,
}

const fn default_max_depth() -> usize {
    2
}

impl Config {
    /// Load an explicitly selected config file, or the global one.
    ///
    /// An explicit path must exist. When the global config is missing, a
    /// commented default is created so users can discover the settings.
    pub fn load(path: Option<&Path>) -> crate::Result<Self> {
        if let Some(path) = path {
            return Self::load_file(path);
        }
        let path = config_path()?;
        ensure_default_config(&path)?;
        Self::load_file(&path)
    }

    /// Path of the config file `load` reads: `explicit`, else the global one.
    pub fn resolve_path(explicit: Option<&Path>) -> crate::Result<PathBuf> {
        explicit.map_or_else(config_path, |path| Ok(path.to_path_buf()))
    }

    /// Apply `MMRY_STATE_ROOT` / `MMRY_MIGRATE` from `lookup` (normally
    /// `std::env::var`). Environment overrides the config file; empty values
    /// are ignored. Command-line flags are applied by the caller afterwards.
    pub fn apply_env(&mut self, lookup: impl Fn(&str) -> Option<String>) -> crate::Result<()> {
        if let Some(root) = lookup(ENV_STATE_ROOT).filter(|value| !value.is_empty()) {
            self.state_root = Some(expand_tilde(Path::new(&root))?);
        }
        if let Some(mode) = lookup(ENV_MIGRATE).filter(|value| !value.is_empty()) {
            self.migrate = mode.parse()?;
        }
        Ok(())
    }

    fn load_file(path: &Path) -> crate::Result<Self> {
        let content = std::fs::read_to_string(path).map_err(|error| {
            crate::Error::Config(format!("cannot read {}: {error}", path.display()))
        })?;
        let mut config: Self = toml::from_str(&content)
            .map_err(|error| crate::Error::Config(format!("{}: {error}", path.display())))?;
        if let Some(state_root) = &config.state_root {
            config.state_root = Some(expand_tilde(state_root)?);
        }
        for root in &mut config.roots {
            root.path = expand_tilde(&root.path)?;
        }
        Ok(config)
    }

    /// The central store root: `state_root` or `$XDG_STATE_HOME/mmry`.
    pub fn state_root(&self) -> crate::Result<PathBuf> {
        match &self.state_root {
            Some(root) => Ok(root.clone()),
            None => Ok(crate::paths::state_base()?.join("mmry")),
        }
    }

    pub fn schema_json() -> crate::Result<String> {
        Ok(serde_json::to_string_pretty(&schemars::schema_for!(Self))?)
    }
}

impl std::str::FromStr for MigrateMode {
    type Err = crate::Error;

    fn from_str(text: &str) -> crate::Result<Self> {
        match text {
            "auto" => Ok(Self::Auto),
            "prompt" => Ok(Self::Prompt),
            "off" => Ok(Self::Off),
            other => Err(crate::Error::Config(format!(
                "invalid migrate mode '{other}': use auto, prompt or off"
            ))),
        }
    }
}

impl MigrateMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Prompt => "prompt",
            Self::Off => "off",
        }
    }
}

/// Set a top-level string `key` in the TOML file at `path`.
///
/// Comments and formatting are preserved. Creates the file (from [`DEFAULT_CONFIG`]) if
/// missing. The result is validated against [`Config`] before writing.
pub fn set_config_value(path: &Path, key: &str, value: &str) -> crate::Result<()> {
    ensure_default_config(path)?;
    let text = std::fs::read_to_string(path)?;
    let document: toml_edit::DocumentMut = text
        .parse()
        .map_err(|error| crate::Error::Config(format!("{}: {error}", path.display())))?;
    let updated = if document.contains_key(key) {
        let mut document = document;
        document[key] = toml_edit::value(value);
        document.to_string()
    } else {
        insert_top_level(&text, key, &toml_edit::value(value).to_string())
    };
    toml::from_str::<Config>(&updated)
        .map_err(|error| crate::Error::Config(format!("{}: {error}", path.display())))?;
    let temp = path.with_extension("toml.tmp");
    std::fs::write(&temp, &updated)?;
    std::fs::rename(&temp, path)?;
    Ok(())
}

/// Insert `key = value` next to its commented default (`# key = ...`), else
/// before the first table header, so leading comments (`#:schema`) stay first.
fn insert_top_level(text: &str, key: &str, value: &str) -> String {
    let line = format!("{key} = {}", value.trim());
    let lines: Vec<&str> = text.lines().collect();
    let commented = format!("# {key} =");
    let position = lines
        .iter()
        .position(|l| l.trim_start().starts_with(&commented))
        .map(|index| index + 1)
        .or_else(|| lines.iter().position(|l| l.trim_start().starts_with('[')))
        .unwrap_or(lines.len());
    let mut out: Vec<&str> = lines[..position].to_vec();
    out.push(&line);
    out.extend_from_slice(&lines[position..]);
    out.join("\n") + "\n"
}

/// Write [`DEFAULT_CONFIG`] to `path` unless a file already exists there.
pub fn ensure_default_config(path: &Path) -> crate::Result<()> {
    if path.exists() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, DEFAULT_CONFIG)?;
    Ok(())
}

pub fn config_path() -> crate::Result<PathBuf> {
    Ok(crate::paths::config_base()?.join("mmry").join(CONFIG_FILE))
}

pub fn expand_tilde(path: &Path) -> crate::Result<PathBuf> {
    let text = path.to_string_lossy();
    if text == "~" {
        return crate::paths::home_dir()
            .ok_or_else(|| crate::Error::Config("cannot expand ~: home is unavailable".into()));
    }
    if let Some(rest) = text.strip_prefix("~/") {
        return crate::paths::home_dir()
            .map(|home| home.join(rest))
            .ok_or_else(|| crate::Error::Config("cannot expand ~: home is unavailable".into()));
    }
    Ok(path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples");

    #[test]
    fn roots_deserialize_with_default_depth() {
        let config: Config = toml::from_str("[[roots]]\npath = '/tmp/code'").unwrap();
        assert_eq!(
            config,
            Config {
                roots: vec![DiscoveryRoot {
                    path: "/tmp/code".into(),
                    max_depth: 2,
                }],
                ..Config::default()
            }
        );
    }

    #[test]
    fn application_visible_schema_key_is_rejected() {
        let result = toml::from_str::<Config>("\"$schema\" = 'https://example.test/s.json'");
        assert!(result.is_err());
    }

    #[test]
    fn explicit_missing_config_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing.toml");
        let error = Config::load(Some(&missing)).unwrap_err();
        assert!(error.to_string().contains("missing.toml"), "{error}");
        assert!(!missing.exists());
    }

    #[test]
    fn default_config_is_created_once_and_parses_to_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mmry").join(CONFIG_FILE);
        ensure_default_config(&path).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), DEFAULT_CONFIG);
        assert_eq!(Config::load(Some(&path)).unwrap(), Config::default());

        std::fs::write(&path, "[[roots]]\npath = '/srv'\n").unwrap();
        ensure_default_config(&path).unwrap();
        assert_eq!(Config::load(Some(&path)).unwrap().roots.len(), 1);
    }

    #[test]
    fn env_overrides_config_file() {
        let mut config: Config = toml::from_str("state_root = '/file'\nmigrate = 'auto'").unwrap();
        config.apply_env(|_| None).unwrap();
        assert_eq!(
            (config.state_root.clone(), config.migrate),
            (Some("/file".into()), MigrateMode::Auto)
        );
        config
            .apply_env(|key| match key {
                ENV_STATE_ROOT => Some("/env".into()),
                ENV_MIGRATE => Some("off".into()),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            (config.state_root.clone(), config.migrate),
            (Some("/env".into()), MigrateMode::Off)
        );
        config.apply_env(|_| Some(String::new())).unwrap();
        assert_eq!(config.migrate, MigrateMode::Off);
        let error = config
            .apply_env(|key| (key == ENV_MIGRATE).then(|| "sometimes".into()))
            .unwrap_err();
        assert!(error.to_string().contains("auto, prompt or off"), "{error}");
    }

    #[test]
    fn set_config_value_preserves_comments_and_validates() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "# keep me\n[[roots]]\npath = '/srv' # inline\n").unwrap();
        set_config_value(&path, "migrate", "auto").unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.contains("# keep me") && text.contains("# inline"),
            "{text}"
        );
        let config = Config::load(Some(&path)).unwrap();
        assert_eq!((config.migrate, config.roots.len()), (MigrateMode::Auto, 1));
        assert!(set_config_value(&path, "migrate", "never").is_err());
        assert_eq!(
            Config::load(Some(&path)).unwrap().migrate,
            MigrateMode::Auto
        );

        let fresh = dir.path().join("new/config.toml");
        set_config_value(&fresh, "migrate", "auto").unwrap();
        let fresh_text = std::fs::read_to_string(&fresh).unwrap();
        assert!(fresh_text.starts_with("#:schema"), "{fresh_text}");
        assert!(
            fresh_text.contains("# migrate = \"prompt\"\nmigrate = \"auto\"\n"),
            "{fresh_text}"
        );
        assert_eq!(
            Config::load(Some(&fresh)).unwrap().migrate,
            MigrateMode::Auto
        );
    }

    #[test]
    fn example_schema_is_current() {
        let example = std::fs::read_to_string(format!("{EXAMPLES}/config.schema.json")).unwrap();
        assert_eq!(
            example.trim_end(),
            Config::schema_json().unwrap(),
            "run `just generate-config`"
        );
    }

    #[test]
    fn example_config_parses_and_points_to_central_schema() {
        let example = std::fs::read_to_string(format!("{EXAMPLES}/config.toml")).unwrap();
        assert!(example.starts_with(DEFAULT_CONFIG.lines().next().unwrap()));
        let config: Config = toml::from_str(&example).unwrap();
        assert!(!config.roots.is_empty());
    }
}
