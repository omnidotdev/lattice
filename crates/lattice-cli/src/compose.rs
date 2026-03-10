use std::{
    io,
    path::{Path, PathBuf},
    process::Command,
};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ComposeError {
    #[error("failed to load config: {0}")]
    Config(#[from] lattice_config::ConfigError),
    #[error("no subgraphs defined in config")]
    NoSubgraphs,
    #[error("no JavaScript runtime found (tried `bun`, then `node`)")]
    RuntimeNotFound,
    #[error("composition failed: {0}")]
    Composition(String),
    #[error("failed to write output: {0}")]
    Write(#[source] io::Error),
}

/// Run federation composition by shelling out to a JS script.
///
/// # Errors
///
/// Returns [`ComposeError`] on config, runtime, composition, or I/O failures.
pub fn run(config_path: &Path, output_path: &Path) -> Result<(), ComposeError> {
    let config = lattice_config::load(config_path)?;

    let subgraphs = config.subgraphs.ok_or(ComposeError::NoSubgraphs)?;
    if subgraphs.is_empty() {
        return Err(ComposeError::NoSubgraphs);
    }

    let input: Vec<serde_json::Value> = subgraphs
        .into_iter()
        .map(|(name, sg)| {
            let mut obj = serde_json::json!({ "name": name });
            if let Some(url) = sg.url {
                obj["url"] = serde_json::Value::String(url);
            }
            if let Some(schema) = sg.schema {
                obj["schema_path"] = serde_json::Value::String(schema);
            }
            obj
        })
        .collect();

    let input_json = serde_json::to_string(&input).expect("serializing subgraph input should not fail");

    let runtime = find_runtime().ok_or(ComposeError::RuntimeNotFound)?;
    let script = find_script()?;

    tracing::info!(
        runtime = %runtime,
        script = %script.display(),
        subgraphs = input.len(),
        "composing supergraph",
    );

    let output = Command::new(&runtime)
        .arg(&script)
        .arg(&input_json)
        .output()
        .map_err(|e| ComposeError::Composition(format!("failed to spawn {runtime}: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ComposeError::Composition(stderr.into_owned()));
    }

    std::fs::write(output_path, &output.stdout).map_err(ComposeError::Write)?;

    tracing::info!(output = %output_path.display(), "supergraph SDL written");

    Ok(())
}

/// Try `bun` first, then fall back to `node`.
fn find_runtime() -> Option<String> {
    for cmd in &["bun", "node"] {
        if Command::new(cmd)
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success())
        {
            return Some((*cmd).to_owned());
        }
    }
    None
}

/// Locate `scripts/compose.mjs` relative to the current working directory,
/// falling back to the directory containing the executable.
///
/// # Errors
///
/// Returns [`ComposeError::Composition`] if the script cannot be found.
fn find_script() -> Result<PathBuf, ComposeError> {
    let cwd_script = PathBuf::from("scripts/compose.mjs");
    if cwd_script.exists() {
        return Ok(cwd_script);
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let exe_script = dir.join("scripts/compose.mjs");
            if exe_script.exists() {
                return Ok(exe_script);
            }
        }
    }

    Err(ComposeError::Composition(
        "scripts/compose.mjs not found in CWD or next to executable".into(),
    ))
}
