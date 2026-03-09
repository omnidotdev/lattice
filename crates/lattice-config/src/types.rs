use std::collections::HashMap;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub supergraph: SupergraphConfig,
    #[serde(default)]
    pub subgraphs: Option<HashMap<String, SubgraphConfig>>,
}

#[derive(Debug, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_listen")]
    pub listen: String,
    #[serde(default)]
    pub playground: Option<bool>,
}

fn default_listen() -> String {
    "0.0.0.0:4000".into()
}

#[derive(Debug, Deserialize)]
pub struct SupergraphConfig {
    pub path: Option<String>,
    pub url: Option<String>,
    pub poll_interval: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SubgraphConfig {
    pub url: Option<String>,
    pub schema: Option<String>,
}
