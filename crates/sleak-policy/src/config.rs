use serde::Deserialize;
use sleak_core::SleakError;
use std::collections::BTreeMap;
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

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(default)]
pub struct PolicyConfig {
    pub max_fan_in: u32,
    pub max_fan_out: u32,
    pub cycles: CyclePolicy,
    /// Raw PageRank score (0.0-1.0) above which a node's influence is
    /// flagged (SLK006). Default matches the directive's own example
    /// (§15).
    pub max_centrality: f64,
    /// Fraction (0.0-1.0) of the rest of the architecture that may
    /// become unreachable before a node's blast radius is flagged
    /// (SLK007). Default matches the directive's own example (§15).
    pub max_blast_radius: f64,
}

impl Default for PolicyConfig {
    fn default() -> Self {
        Self {
            max_fan_in: 25,
            max_fan_out: 20,
            cycles: CyclePolicy::default(),
            max_centrality: 0.20,
            max_blast_radius: 0.30,
        }
    }
}

/// One disallowed edge direction between two declared layers, e.g.
/// `domain -> infrastructure`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ForbiddenRule {
    pub from: String,
    pub to: String,
}

/// Declared architectural boundaries (directive §4.3). Disabled by
/// default and inert with no `forbidden` rules — Sleak never invents a
/// paradigm (layered, hexagonal, ...) to check against; the project
/// declares its own layers and which crossings are disallowed.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct BoundariesConfig {
    pub enabled: bool,
    /// Layer name -> crate-relative path prefix (e.g.
    /// `"domain" -> "src/domain"`), matched against each module's
    /// `src`-prefixed path.
    pub layers: BTreeMap<String, String>,
    pub forbidden: Vec<ForbiddenRule>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    pub policy: PolicyConfig,
    pub boundaries: BoundariesConfig,
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
        assert_eq!(cfg.policy.max_centrality, 0.20);
        assert_eq!(cfg.policy.max_blast_radius, 0.30);
        assert!(!cfg.boundaries.enabled);
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
    fn boundaries_config_parses_layers_and_forbidden_rules() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(CONFIG_FILE_NAME),
            r#"
[boundaries]
enabled = true

[boundaries.layers]
domain = "src/domain"
infrastructure = "src/infrastructure"

[[boundaries.forbidden]]
from = "domain"
to = "infrastructure"
"#,
        )
        .unwrap();

        let cfg = Config::load(dir.path()).unwrap();
        assert!(cfg.boundaries.enabled);
        assert_eq!(
            cfg.boundaries.layers.get("domain").map(String::as_str),
            Some("src/domain")
        );
        assert_eq!(cfg.boundaries.forbidden.len(), 1);
        assert_eq!(cfg.boundaries.forbidden[0].from, "domain");
        assert_eq!(cfg.boundaries.forbidden[0].to, "infrastructure");
    }

    #[test]
    fn malformed_config_is_a_hard_error() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(CONFIG_FILE_NAME), "not valid toml =====").unwrap();

        let err = Config::load(dir.path()).unwrap_err();
        assert!(matches!(err, SleakError::Configuration { .. }));
    }
}
