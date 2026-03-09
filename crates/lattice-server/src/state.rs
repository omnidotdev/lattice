use lattice_config::Config;

/// Shared application state for the axum server.
pub struct AppState {
    pub config: Option<Config>,
}

impl AppState {
    /// Create state from a loaded config.
    #[must_use]
    pub const fn from_config(config: Config) -> Self {
        Self { config: Some(config) }
    }

    /// Create a stub state for testing.
    #[cfg(test)]
    pub fn new_stub() -> Self {
        Self { config: None }
    }
}
