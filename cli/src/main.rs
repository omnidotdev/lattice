#![cfg_attr(test, allow(unused_crate_dependencies))]
#![forbid(unsafe_code)]

mod cli_input;
mod compose;
mod errors;
mod introspect;
mod lint;
mod output;
mod panic_hook;
mod watercolor;

#[macro_use]
extern crate log;

use crate::cli_input::{Args, SubCommand};
use clap::Parser;
use errors::CliError;
use output::report;
use std::{io::IsTerminal as _, process};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};
use watercolor::ShouldColorize;

use mimalloc::MiMalloc;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

fn main() {
    panic_hook!();

    rustls::crypto::ring::default_provider().install_default().unwrap();

    let args = Args::parse();
    ShouldColorize::from_env();

    let exit_code = match try_main(args) {
        Ok(()) => 0,
        Err(error) => {
            report::error(&error);
            1
        }
    };

    process::exit(exit_code);
}

fn try_main(args: Args) -> Result<(), CliError> {
    let filter = {
        let builder = EnvFilter::builder();
        match args.log_filter() {
            Some(argument_filter) => builder.parse_lossy(argument_filter),
            None => builder.from_env_lossy(),
        }
    };

    tracing_subscriber::registry().with(fmt::layer()).with(filter).init();

    trace!("subcommand: {}", args.command);

    // Do not display header if output is piped
    if std::io::stdout().is_terminal() {
        report::cli_header();
    }

    match args.command {
        SubCommand::Completions(cmd) => {
            cmd.shell.completions();
            Ok(())
        }
        SubCommand::Introspect(cmd) => introspect::introspect(&cmd),
        SubCommand::Lint(cmd) => lint::lint(cmd.schema),
        SubCommand::Compose(cmd) => compose::compose_command(cmd),
    }
}
