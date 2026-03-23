use lattice_config::PluginConfig;

use crate::error::PluginError;
use crate::hooks::{
    HookResult, OnGraphqlParseInput, OnGraphqlValidationInput, OnHttpResponseInput, OnOperationResponseInput,
    OnQueryPlanInput, OnRequestInput, OnResponseInput, OnSubgraphRequestInput, OnSubgraphResponseInput,
};
use crate::instance::PluginInstance;

/// Manage loaded plugin instances and dispatch hooks.
#[derive(Debug)]
pub struct PluginManager {
    plugins: Vec<PluginInstance>,
}

impl PluginManager {
    /// Create a new manager with the given plugin instances.
    #[must_use]
    pub const fn new(plugins: Vec<PluginInstance>) -> Self {
        Self { plugins }
    }

    /// Run `on_request` across all plugins that registered for it.
    ///
    /// Returns the first `Reject` or `Modify` result, or `Continue` if all
    /// plugins continue. Individual plugin errors are logged and skipped
    /// (fail-open) so one broken plugin doesn't block later plugins.
    pub fn on_request(&self, input: &OnRequestInput) -> HookResult {
        for plugin in &self.plugins {
            if !plugin.has_hook("on_request") {
                continue;
            }

            match plugin.on_request(input) {
                Ok(HookResult::Continue) => {}
                Ok(result @ (HookResult::Modify { .. } | HookResult::Reject { .. })) => {
                    tracing::info!(plugin = plugin.name(), "on_request hook intercepted");
                    return result;
                }
                Err(e) => {
                    tracing::warn!(plugin = plugin.name(), error = %e, "on_request plugin error, skipping");
                }
            }
        }

        HookResult::Continue
    }

    /// Run `on_response` across all plugins that registered for it.
    ///
    /// Returns the first `Modify` result, or `Continue` if all continue.
    /// `Reject` is treated as `Continue` for response hooks (too late to reject).
    /// Individual plugin errors are logged and skipped (fail-open).
    pub fn on_response(&self, input: &OnResponseInput) -> HookResult {
        for plugin in &self.plugins {
            if !plugin.has_hook("on_response") {
                continue;
            }

            match plugin.on_response(input) {
                Ok(HookResult::Continue | HookResult::Reject { .. }) => {}
                Ok(result @ HookResult::Modify { .. }) => {
                    tracing::info!(plugin = plugin.name(), "on_response hook modified response");
                    return result;
                }
                Err(e) => {
                    tracing::warn!(plugin = plugin.name(), error = %e, "on_response plugin error, skipping");
                }
            }
        }

        HookResult::Continue
    }

    /// Run `on_gateway_request` across all plugins (backward-compatible with `on_request`).
    ///
    /// Returns the first `Reject` or `Modify` result, or `Continue` if all
    /// plugins continue.
    pub fn on_gateway_request(&self, input: &OnRequestInput) -> HookResult {
        for plugin in &self.plugins {
            if !plugin.has_hook("on_gateway_request") && !plugin.has_hook("on_request") {
                continue;
            }

            match plugin.on_gateway_request(input) {
                Ok(HookResult::Continue) => {}
                Ok(result @ (HookResult::Modify { .. } | HookResult::Reject { .. })) => {
                    tracing::info!(plugin = plugin.name(), "on_gateway_request hook intercepted");
                    return result;
                }
                Err(e) => {
                    tracing::warn!(plugin = plugin.name(), error = %e, "on_gateway_request plugin error, skipping");
                }
            }
        }

        HookResult::Continue
    }

    /// Run `on_gateway_response` across all plugins (backward-compatible with `on_response`).
    ///
    /// Returns the first `Modify` result, or `Continue` if all continue.
    pub fn on_gateway_response(&self, input: &OnResponseInput) -> HookResult {
        for plugin in &self.plugins {
            if !plugin.has_hook("on_gateway_response") && !plugin.has_hook("on_response") {
                continue;
            }

            match plugin.on_gateway_response(input) {
                Ok(HookResult::Continue | HookResult::Reject { .. }) => {}
                Ok(result @ HookResult::Modify { .. }) => {
                    tracing::info!(plugin = plugin.name(), "on_gateway_response hook modified response");
                    return result;
                }
                Err(e) => {
                    tracing::warn!(plugin = plugin.name(), error = %e, "on_gateway_response plugin error, skipping");
                }
            }
        }

        HookResult::Continue
    }

    /// Run `on_subgraph_request` across all plugins.
    ///
    /// Returns the first `Reject` or `Modify` result, or `Continue` if all
    /// plugins continue.
    pub fn on_subgraph_request(&self, input: &OnSubgraphRequestInput) -> HookResult {
        for plugin in &self.plugins {
            if !plugin.has_hook("on_subgraph_request") {
                continue;
            }

            match plugin.on_subgraph_request(input) {
                Ok(HookResult::Continue) => {}
                Ok(result @ (HookResult::Modify { .. } | HookResult::Reject { .. })) => {
                    tracing::info!(plugin = plugin.name(), "on_subgraph_request hook intercepted");
                    return result;
                }
                Err(e) => {
                    tracing::warn!(plugin = plugin.name(), error = %e, "on_subgraph_request plugin error, skipping");
                }
            }
        }

        HookResult::Continue
    }

    /// Run `on_subgraph_response` across all plugins.
    ///
    /// Returns the first `Modify` result, or `Continue` if all continue.
    /// Cannot reject (modify only).
    pub fn on_subgraph_response(&self, input: &OnSubgraphResponseInput) -> HookResult {
        for plugin in &self.plugins {
            if !plugin.has_hook("on_subgraph_response") {
                continue;
            }

            match plugin.on_subgraph_response(input) {
                Ok(HookResult::Continue | HookResult::Reject { .. }) => {}
                Ok(result @ HookResult::Modify { .. }) => {
                    tracing::info!(plugin = plugin.name(), "on_subgraph_response hook modified");
                    return result;
                }
                Err(e) => {
                    tracing::warn!(plugin = plugin.name(), error = %e, "on_subgraph_response plugin error, skipping");
                }
            }
        }

        HookResult::Continue
    }

    /// Run `on_operation_response` across all plugins.
    ///
    /// Returns the first `Modify` result, or `Continue` if all continue.
    /// Cannot reject (modify only).
    pub fn on_operation_response(&self, input: &OnOperationResponseInput) -> HookResult {
        for plugin in &self.plugins {
            if !plugin.has_hook("on_operation_response") {
                continue;
            }

            match plugin.on_operation_response(input) {
                Ok(HookResult::Continue | HookResult::Reject { .. }) => {}
                Ok(result @ HookResult::Modify { .. }) => {
                    tracing::info!(plugin = plugin.name(), "on_operation_response hook modified");
                    return result;
                }
                Err(e) => {
                    tracing::warn!(plugin = plugin.name(), error = %e, "on_operation_response plugin error, skipping");
                }
            }
        }

        HookResult::Continue
    }

    /// Run `on_http_response` across all plugins.
    ///
    /// Returns the first `Modify` result, or `Continue` if all continue.
    /// Cannot reject (modify only).
    pub fn on_http_response(&self, input: &OnHttpResponseInput) -> HookResult {
        for plugin in &self.plugins {
            if !plugin.has_hook("on_http_response") {
                continue;
            }

            match plugin.on_http_response(input) {
                Ok(HookResult::Continue | HookResult::Reject { .. }) => {}
                Ok(result @ HookResult::Modify { .. }) => {
                    tracing::info!(plugin = plugin.name(), "on_http_response hook modified");
                    return result;
                }
                Err(e) => {
                    tracing::warn!(plugin = plugin.name(), error = %e, "on_http_response plugin error, skipping");
                }
            }
        }

        HookResult::Continue
    }

    /// Run `on_graphql_parse` across all plugins that registered for it.
    ///
    /// Returns the first `Reject` or `Modify` result, or `Continue` if all
    /// plugins continue.
    pub fn on_graphql_parse(&self, input: &OnGraphqlParseInput) -> HookResult {
        for plugin in &self.plugins {
            if !plugin.has_hook("on_graphql_parse") {
                continue;
            }

            match plugin.on_graphql_parse(input) {
                Ok(HookResult::Continue) => {}
                Ok(result @ (HookResult::Modify { .. } | HookResult::Reject { .. })) => {
                    tracing::info!(plugin = plugin.name(), "on_graphql_parse hook intercepted");
                    return result;
                }
                Err(e) => {
                    tracing::warn!(plugin = plugin.name(), error = %e, "on_graphql_parse plugin error, skipping");
                }
            }
        }

        HookResult::Continue
    }

    /// Run `on_graphql_validation` across all plugins that registered for it.
    ///
    /// Returns the first `Reject` or `Modify` result, or `Continue` if all
    /// plugins continue.
    pub fn on_graphql_validation(&self, input: &OnGraphqlValidationInput) -> HookResult {
        for plugin in &self.plugins {
            if !plugin.has_hook("on_graphql_validation") {
                continue;
            }

            match plugin.on_graphql_validation(input) {
                Ok(HookResult::Continue) => {}
                Ok(result @ (HookResult::Modify { .. } | HookResult::Reject { .. })) => {
                    tracing::info!(plugin = plugin.name(), "on_graphql_validation hook intercepted");
                    return result;
                }
                Err(e) => {
                    tracing::warn!(plugin = plugin.name(), error = %e, "on_graphql_validation plugin error, skipping");
                }
            }
        }

        HookResult::Continue
    }

    /// Run `on_query_plan` across all plugins that registered for it.
    ///
    /// Returns the first `Reject` or `Modify` result, or `Continue` if all
    /// plugins continue.
    pub fn on_query_plan(&self, input: &OnQueryPlanInput) -> HookResult {
        for plugin in &self.plugins {
            if !plugin.has_hook("on_query_plan") {
                continue;
            }

            match plugin.on_query_plan(input) {
                Ok(HookResult::Continue) => {}
                Ok(result @ (HookResult::Modify { .. } | HookResult::Reject { .. })) => {
                    tracing::info!(plugin = plugin.name(), "on_query_plan hook intercepted");
                    return result;
                }
                Err(e) => {
                    tracing::warn!(plugin = plugin.name(), error = %e, "on_query_plan plugin error, skipping");
                }
            }
        }

        HookResult::Continue
    }

    /// Number of loaded plugins.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.plugins.len()
    }

    /// Whether any plugins are loaded.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.plugins.is_empty()
    }
}

/// Convert a YAML value to a plain string for plugin config.
fn yaml_value_to_string(v: &serde_yaml::Value) -> String {
    match v {
        serde_yaml::Value::String(s) => s.clone(),
        serde_yaml::Value::Bool(b) => b.to_string(),
        serde_yaml::Value::Number(n) => n.to_string(),
        serde_yaml::Value::Null => String::new(),
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

/// Warn about hook names that aren't recognized.
fn warn_unrecognized_hooks(name: &str, hooks: &[String]) {
    for hook in hooks {
        if !matches!(
            hook.as_str(),
            "on_request"
                | "on_response"
                | "on_gateway_request"
                | "on_gateway_response"
                | "on_subgraph_request"
                | "on_subgraph_response"
                | "on_operation_response"
                | "on_http_response"
                | "on_graphql_parse"
                | "on_graphql_validation"
                | "on_query_plan"
        ) {
            tracing::warn!(plugin = %name, hook = %hook, "unrecognized hook name");
        }
    }
}

/// Load all plugins from config.
///
/// # Errors
///
/// Returns [`PluginError::Load`] if any `.wasm` file fails to load.
pub fn load_plugins(configs: &[PluginConfig]) -> Result<PluginManager, PluginError> {
    let mut plugins = Vec::with_capacity(configs.len());

    for cfg in configs {
        let config_map: std::collections::BTreeMap<String, String> = cfg
            .config
            .iter()
            .map(|(k, v)| (k.clone(), yaml_value_to_string(v)))
            .collect();

        warn_unrecognized_hooks(&cfg.name, &cfg.hooks);
        tracing::info!(name = %cfg.name, path = %cfg.path, "loading plugin");

        let instance = PluginInstance::from_file(&cfg.name, &cfg.path, cfg.hooks.clone(), config_map)?;
        plugins.push(instance);
    }

    tracing::info!(count = plugins.len(), "loaded plugins");
    Ok(PluginManager::new(plugins))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_manager_returns_continue_for_on_request() {
        let mgr = PluginManager::new(vec![]);
        let input = OnRequestInput {
            query: "{ me }".into(),
            operation_name: None,
            variables: None,
            client_ip: None,
        };
        let result = mgr.on_request(&input);
        assert!(matches!(result, HookResult::Continue));
    }

    #[test]
    fn empty_manager_returns_continue_for_on_response() {
        let mgr = PluginManager::new(vec![]);
        let input = OnResponseInput {
            query: "{ me }".into(),
            data: None,
            errors: vec![],
        };
        let result = mgr.on_response(&input);
        assert!(matches!(result, HookResult::Continue));
    }

    #[test]
    fn empty_manager_returns_continue_for_on_graphql_parse() {
        let mgr = PluginManager::new(vec![]);
        let input = OnGraphqlParseInput {
            query: "{ me }".into(),
            operation_name: None,
        };
        let result = mgr.on_graphql_parse(&input);
        assert!(matches!(result, HookResult::Continue));
    }

    #[test]
    fn empty_manager_returns_continue_for_on_graphql_validation() {
        let mgr = PluginManager::new(vec![]);
        let input = OnGraphqlValidationInput {
            query: "{ me }".into(),
            operation_name: None,
            is_valid: true,
            validation_errors: vec![],
        };
        let result = mgr.on_graphql_validation(&input);
        assert!(matches!(result, HookResult::Continue));
    }

    #[test]
    fn empty_manager_returns_continue_for_on_query_plan() {
        let mgr = PluginManager::new(vec![]);
        let input = OnQueryPlanInput {
            query: "{ me }".into(),
            operation_name: None,
            plan_json: serde_json::json!({"node": null}),
        };
        let result = mgr.on_query_plan(&input);
        assert!(matches!(result, HookResult::Continue));
    }

    #[test]
    fn manager_len_and_is_empty() {
        let mgr = PluginManager::new(vec![]);
        assert!(mgr.is_empty());
        assert_eq!(mgr.len(), 0);
    }
}
