use thiserror::Error;

/// Errors that can occur during plugin operations.
#[derive(Debug, Error)]
pub enum PluginError {
    /// Failed to load a WASM plugin from disk.
    #[error("failed to load plugin: {0}")]
    Load(String),
    /// Plugin hook call failed.
    #[error("plugin hook error: {0}")]
    Hook(String),
    /// Plugin returned invalid JSON.
    #[error("failed to deserialize plugin response: {0}")]
    Deserialize(String),
}
