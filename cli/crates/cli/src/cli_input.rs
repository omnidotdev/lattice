mod completions;
mod compose;
mod introspect;
mod lint;
mod sub_command;

pub(crate) use compose::ComposeCommand;
pub(crate) use completions::CompletionsCommand;
pub(crate) use introspect::IntrospectCommand;
pub(crate) use lint::LintCommand;
pub(crate) use sub_command::SubCommand;

use clap::Parser;
use std::path::PathBuf;

fn split_header(header: &str) -> Option<(&str, &str)> {
    header.find(':').map(|split_index| {
        let key = header[0..split_index].trim();
        let value = header[split_index + 1..].trim();

        (key, value)
    })
}

#[derive(Debug, Parser)]
#[command(name = "Lattice CLI", version)]
/// The Lattice command line interface
pub struct Args {
    /// Set the tracing level
    #[arg(short, long, default_value_t = 0)]
    pub trace: u16,
    #[arg(long, hide = true)]
    pub custom_trace_filter: Option<String>,
    #[command(subcommand)]
    pub command: SubCommand,
    /// An optional replacement path for the home directory
    #[arg(long)]
    pub home: Option<PathBuf>,
}

impl Args {
    pub fn log_filter(&self) -> Option<&str> {
        if let Some(custom_trace) = &self.custom_trace_filter {
            Some(custom_trace.as_str())
        } else if self.trace >= 1 {
            Some("lattice=trace")
        } else {
            None
        }
    }
}
