use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use std::path::Path;
use std::path::PathBuf;

const CONFIG_FILE: &str = "config.toml";

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
    pub fn load(path: Option<&Path>) -> crate::Result<Self> {
        let path = path.map_or(config_path()?, Path::to_path_buf);
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(&path)?;
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

    #[test]
    fn roots_deserialize_and_appear_in_schema() {
        let config: Config =
            toml::from_str("[[roots]]\npath = '/tmp/code'\nmax_depth = 3").unwrap();
        assert_eq!(config.roots[0].max_depth, 3);
        let schema = Config::schema_json().unwrap();
        assert!(schema.contains("max_depth"));
        assert!(schema.contains("path"));
    }
}
