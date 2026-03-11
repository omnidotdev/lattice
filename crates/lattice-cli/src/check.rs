use std::path::Path;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum CheckError {
    #[error("failed to load config: {0}")]
    Config(#[from] lattice_config::ConfigError),
    #[error("no subgraphs defined in config")]
    NoSubgraphs,
    #[error("supergraph SDL not found at: {0}")]
    SupergraphNotFound(String),
    #[error("invalid supergraph SDL: {0}")]
    InvalidSupergraph(String),
}

/// Validate a Lattice configuration file and its referenced resources.
///
/// # Errors
///
/// Returns [`CheckError`] if the config is invalid or references are broken.
pub fn run(config_path: &Path) -> Result<(), CheckError> {
    let config = lattice_config::load(config_path)?;

    tracing::info!(path = %config_path.display(), "config loaded successfully");

    check_subgraphs(&config)?;
    check_supergraph(&config)?;
    check_plugins(&config);

    tracing::info!("all checks passed");

    Ok(())
}

fn check_subgraphs(config: &lattice_config::Config) -> Result<(), CheckError> {
    if let Some(ref subgraphs) = config.subgraphs {
        if subgraphs.is_empty() {
            return Err(CheckError::NoSubgraphs);
        }
        tracing::info!(count = subgraphs.len(), "subgraphs configured");
    }
    Ok(())
}

fn check_supergraph(config: &lattice_config::Config) -> Result<(), CheckError> {
    if let Some(ref path) = config.supergraph.path {
        let sdl = std::fs::read_to_string(path).map_err(|_| CheckError::SupergraphNotFound(path.clone()))?;

        if sdl.trim().is_empty() {
            return Err(CheckError::InvalidSupergraph("supergraph SDL is empty".into()));
        }

        tracing::info!(path = %path, "supergraph SDL is valid");
    }
    Ok(())
}

fn check_plugins(config: &lattice_config::Config) {
    let Some(ref plugins) = config.plugins else {
        return;
    };
    for plugin in plugins {
        if Path::new(&plugin.path).exists() {
            tracing::info!(name = %plugin.name, "plugin found");
        } else {
            tracing::warn!(name = %plugin.name, path = %plugin.path, "plugin not found");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_missing_config() {
        let result = run(Path::new("nonexistent.yaml"));
        assert!(result.is_err());
    }
}
