use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use std::path::Path;
use std::path::PathBuf;

const CONFIG_FILE: &str = "config.toml";

/// Commented config written on first run when no global config exists.
pub const DEFAULT_CONFIG: &str = "\
#:schema https://raw.githubusercontent.com/byteowlz/schemas/refs/heads/main/mmry/mmry.config.schema.json

# Bounded directories searched by cross-repository commands (`--all`, `--root`).
# Discovery never searches the home directory unless it is listed here.
#
# [[roots]]
# path = \"~/byteowlz\"
# max_depth = 2
";

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Bounded directories searched by cross-repository commands.
    pub roots: Vec<DiscoveryRoot>,
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

    fn load_file(path: &Path) -> crate::Result<Self> {
        let content = std::fs::read_to_string(path).map_err(|error| {
            crate::Error::Config(format!("cannot read {}: {error}", path.display()))
        })?;
        let mut config: Self = toml::from_str(&content)
            .map_err(|error| crate::Error::Config(format!("{}: {error}", path.display())))?;
        for root in &mut config.roots {
            root.path = expand_tilde(&root.path)?;
        }
        Ok(config)
    }

    pub fn schema_json() -> crate::Result<String> {
        Ok(serde_json::to_string_pretty(&schemars::schema_for!(Self))?)
    }
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
