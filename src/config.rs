use crate::algorithm::Algo;
use crate::LeechMethod;
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Deserialize, Clone, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub storage: StorageConfig,
    pub scan: ScanConfig,
    pub revise: ReviseConfig,
}

#[derive(Debug, Default, Deserialize, Clone, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct StorageConfig {
    pub state_dir: Option<PathBuf>,
}

#[derive(Debug, Default, Deserialize, Clone, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct ScanConfig {
    pub file_types: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize, Clone, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct ReviseConfig {
    pub maximum_cards_per_session: Option<usize>,
    pub maximum_duration_of_session: Option<usize>,
    pub leech_failure_threshold: Option<usize>,
    pub leech_method: Option<LeechMethod>,
    pub algorithm: Option<Algo>,
    pub reverse_probability: Option<f64>,
}

pub fn project_config_path(vault_dir: &Path) -> PathBuf {
    vault_dir.join("config.toml")
}

pub fn load_config(vault_dir: &Path) -> Result<Config, String> {
    let path = project_config_path(vault_dir);
    let content = match std::fs::read_to_string(&path) {
        Ok(content) => content,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Config::default()),
        Err(e) => return Err(format!("Failed to read {}: {}", path.display(), e)),
    };

    toml::from_str(&content).map_err(|e| format!("Failed to parse {}: {}", path.display(), e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_full_config() {
        let config: Config = toml::from_str(
            r#"
            [storage]
            state_dir = "/tmp/carddown-state"

            [scan]
            file_types = ["md", "txt"]

            [revise]
            maximum_cards_per_session = 10
            maximum_duration_of_session = 15
            leech_failure_threshold = 7
            leech_method = "warn"
            algorithm = "sm2"
            reverse_probability = 0.25
        "#,
        )
        .unwrap();

        assert_eq!(
            config.storage.state_dir,
            Some(PathBuf::from("/tmp/carddown-state"))
        );
        assert_eq!(
            config.scan.file_types,
            Some(vec!["md".to_string(), "txt".to_string()])
        );
        assert_eq!(config.revise.maximum_cards_per_session, Some(10));
        assert_eq!(config.revise.maximum_duration_of_session, Some(15));
        assert_eq!(config.revise.leech_failure_threshold, Some(7));
        assert_eq!(config.revise.leech_method, Some(LeechMethod::Warn));
        assert_eq!(config.revise.algorithm, Some(Algo::SM2));
        assert_eq!(config.revise.reverse_probability, Some(0.25));
    }

    #[test]
    fn test_unknown_fields_rejected() {
        let result: Result<Config, _> = toml::from_str(
            r#"
            [storage]
            nope = "x"
        "#,
        );

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("nope"));
    }
}
