use serde::Deserialize;
use sleak_core::SleakError;
use std::path::Path;

pub const CONFIG_FILE_NAME: &str = ".sleak.toml";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CyclePolicy {
    #[default]
    Error,
    Warn,
    Ignore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct PolicyConfig {
    pub max_fan_in: u32,
    pub max_fan_out: u32,
    pub cycles: CyclePolicy,
}

impl Default for PolicyConfig {
    fn default() -> Self {
        Self {
            max_fan_in: 25,
            max_fan_out: 20,
            cycles: CyclePolicy::default(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    pub policy: PolicyConfig,
}

impl Config {
    /// Loads `.sleak.toml` from `root`. A missing file yields defaults;
    /// a present-but-unparseable file is a hard error — Sleak never
    /// silently falls back to defaults on a *malformed* config, only on
    /// an *absent* one.
    pub fn load(root: &Path) -> Result<Config, SleakError> {
        let path = root.join(CONFIG_FILE_NAME);
        match std::fs::read_to_string(&path) {
            Ok(contents) => toml::from_str(&contents).map_err(|e| SleakError::Configuration {
                path,
                reason: e.to_string(),
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
            Err(source) => Err(SleakError::Io { path, source }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_config_file_yields_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = Config::load(dir.path()).unwrap();
        assert_eq!(cfg.policy.max_fan_in, 25);
        assert_eq!(cfg.policy.max_fan_out, 20);
        assert_eq!(cfg.policy.cycles, CyclePolicy::Error);
    }

    #[test]
    fn present_config_overrides_defaults() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(CONFIG_FILE_NAME),
            "[policy]\nmax_fan_in = 3\ncycles = \"warn\"\n",
        )
        .unwrap();

        let cfg = Config::load(dir.path()).unwrap();
        assert_eq!(cfg.policy.max_fan_in, 3);
        assert_eq!(cfg.policy.max_fan_out, 20);
        assert_eq!(cfg.policy.cycles, CyclePolicy::Warn);
    }

    #[test]
    fn malformed_config_is_a_hard_error() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(CONFIG_FILE_NAME), "not valid toml =====").unwrap();

        let err = Config::load(dir.path()).unwrap_err();
        assert!(matches!(err, SleakError::Configuration { .. }));
    }
}
