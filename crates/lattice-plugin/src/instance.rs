use std::collections::BTreeMap;
use std::sync::Mutex;

use extism::{Manifest, Plugin, Wasm};

use crate::error::PluginError;
use crate::hooks::{HookResult, OnRequestInput, OnResponseInput};

/// A loaded WASI plugin instance.
pub struct PluginInstance {
    name: String,
    plugin: Mutex<Plugin>,
    hooks: Vec<String>,
}

impl PluginInstance {
    /// Load a plugin from a `.wasm` file path.
    ///
    /// # Errors
    ///
    /// Returns [`PluginError::Load`] if the file cannot be loaded or is invalid.
    pub fn from_file(
        name: &str,
        path: &str,
        hooks: Vec<String>,
        config: BTreeMap<String, String>,
    ) -> Result<Self, PluginError> {
        let wasm = Wasm::file(path);
        let manifest = Manifest::new([wasm]).with_config(config.into_iter());
        let plugin = Plugin::new(manifest, [], true).map_err(|e| PluginError::Load(format!("{e}")))?;

        Ok(Self {
            name: name.to_owned(),
            plugin: Mutex::new(plugin),
            hooks,
        })
    }

    /// The plugin name (from config).
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Whether this plugin registered for a given hook.
    #[must_use]
    pub fn has_hook(&self, hook: &str) -> bool {
        self.hooks.iter().any(|h| h == hook)
    }

    /// Call a named hook with JSON input, return deserialized `HookResult`.
    fn call_hook(&self, hook: &str, input: &[u8]) -> Result<HookResult, PluginError> {
        let mut plugin = self
            .plugin
            .lock()
            .map_err(|e| PluginError::Hook(format!("lock poisoned: {e}")))?;

        if !plugin.function_exists(hook) {
            return Ok(HookResult::Continue);
        }

        let output: Vec<u8> = plugin
            .call(hook, input)
            .map_err(|e| PluginError::Hook(format!("{e}")))?;
        drop(plugin);

        serde_json::from_slice(&output).map_err(|e| PluginError::Deserialize(format!("{e}")))
    }

    /// Call the `on_request` hook.
    ///
    /// # Errors
    ///
    /// Returns [`PluginError::Hook`] if the plugin call fails.
    pub fn on_request(&self, input: &OnRequestInput) -> Result<HookResult, PluginError> {
        let bytes = serde_json::to_vec(input).map_err(|e| PluginError::Hook(format!("serialize: {e}")))?;
        self.call_hook("on_request", &bytes)
    }

    /// Call the `on_response` hook.
    ///
    /// # Errors
    ///
    /// Returns [`PluginError::Hook`] if the plugin call fails.
    pub fn on_response(&self, input: &OnResponseInput) -> Result<HookResult, PluginError> {
        let bytes = serde_json::to_vec(input).map_err(|e| PluginError::Hook(format!("serialize: {e}")))?;
        self.call_hook("on_response", &bytes)
    }
}

impl std::fmt::Debug for PluginInstance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginInstance")
            .field("name", &self.name)
            .field("hooks", &self.hooks)
            .finish_non_exhaustive()
    }
}
