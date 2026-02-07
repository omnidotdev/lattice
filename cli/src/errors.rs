use std::io;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CliError {
    /// The CLI argument was missing
    #[error("the following required argument was not provided: {0}")]
    MissingArgument(&'static str),
    #[error("error during graph introspection: {0}")]
    Introspection(String),
    #[error("could not read the GraphQL schema")]
    SchemaReadError(#[source] io::Error),
    #[error(transparent)]
    LintInvalidSchema(#[from] graphql_lint::LinterError),
    /// Returned if a linted schema could not be read
    #[error("could not read '{0}'\nCaused by: {1}")]
    ReadLintSchema(PathBuf, io::Error),
    /// Returned if a directory or file without an extension is passed to lint
    #[error("attempted to lint a directory or a file without an extension")]
    LintNoExtension,
    /// Returned if an unsupported extension is passed to lint
    #[error("attempted to lint a file with an unsupported extension: '{0}'")]
    LintUnsupportedFileExtension(String),
    /// Returned if the compose config file could not be read
    #[error("could not read compose config '{0}'\nCaused by: {1}")]
    ReadComposeConfig(PathBuf, io::Error),
    /// Returned if the compose config file could not be parsed
    #[error("could not parse compose config '{0}'\nCaused by: {1}")]
    ParseComposeConfig(PathBuf, String),
    /// Returned if a subgraph schema file could not be read
    #[error("could not read subgraph schema '{0}'\nCaused by: {1}")]
    ReadSubgraphSchema(PathBuf, io::Error),
    /// Returned if composition failed
    #[error("composition failed:\n{0}")]
    CompositionFailed(String),
    /// Returned if output file could not be written
    #[error("could not write output file '{0}'\nCaused by: {1}")]
    WriteComposeOutput(PathBuf, io::Error),
    /// Returned if environment variable is not set
    #[error("environment variable '{0}' is not set")]
    EnvVarNotSet(String),
    /// Returned if the tokio runtime could not be created
    #[error("could not create async runtime\nCaused by: {0}")]
    RuntimeCreation(io::Error),
    /// Returned if writing to stdout failed
    #[error("could not write to stdout\nCaused by: {0}")]
    StdoutWrite(io::Error),
    /// Returned if a subgraph config is invalid
    #[error("invalid subgraph '{0}': must have either 'url' or both 'schema' and 'routing_url'")]
    InvalidSubgraphConfig(String),
}

impl CliError {
    /// Return the appropriate hint for a [`CliError`]
    pub fn to_hint(&self) -> Option<String> {
        match self {
            Self::LintUnsupportedFileExtension(_) | Self::LintNoExtension => Some(
                "try passing a file with a supported extension: '.gql', '.graphql', '.graphqls' or '.sdl'".to_owned(),
            ),
            _ => None,
        }
    }
}
