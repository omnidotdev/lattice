mod error;
mod hooks;
mod instance;
mod manager;

pub use error::PluginError;
pub use hooks::{
    HookResult, OnHttpResponseInput, OnOperationResponseInput, OnRequestInput, OnResponseInput, OnSubgraphRequestInput,
    OnSubgraphResponseInput,
};
pub use instance::PluginInstance;
pub use manager::{PluginManager, load_plugins};
