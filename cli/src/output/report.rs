use crate::{errors::CliError, watercolor};
use colored::Colorize;

/// Report to stdout that the CLI has started
pub fn cli_header() {
    let version = env!("CARGO_PKG_VERSION");
    println!("{}", format!("Lattice CLI {version}\n").dimmed());
}

/// Report an error to stderr
pub fn error(error: &CliError) {
    watercolor::output_error!("Error: {error}", @BrightRed);
    if let Some(hint) = error.to_hint() {
        watercolor::output_error!("Hint: {hint}", @BrightBlue);
    }
}

pub fn lint_success() {
    watercolor::output!("No issues found in your schema", @BrightGreen)
}

pub fn lint_warning(warning: String) {
    watercolor::output!("[Warning] {warning}", @BrightYellow);
}
