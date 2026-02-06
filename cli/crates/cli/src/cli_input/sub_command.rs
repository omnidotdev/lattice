use clap::Parser;

use super::{CompletionsCommand, ComposeCommand, IntrospectCommand, LintCommand};

#[derive(Debug, Parser, strum::AsRefStr, strum::Display)]
#[strum(serialize_all = "lowercase")]
pub enum SubCommand {
    /// Output completions for the chosen shell to use, write the output to the
    /// appropriate location for your shell
    Completions(CompletionsCommand),
    /// Introspect a graph and print its schema
    Introspect(IntrospectCommand),
    /// Lint a GraphQL schema
    Lint(LintCommand),
    /// Compose subgraph schemas into a federated supergraph
    Compose(ComposeCommand),
}
