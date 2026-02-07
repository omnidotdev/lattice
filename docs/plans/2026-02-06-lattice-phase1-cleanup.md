# Lattice Phase 1: License & Cleanup Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Re-license lattice to MIT, remove unused crates/commands/packages, and get `cargo check` passing with a lean codebase.

**Architecture:** Surgical removal of independent code (packages, CLI commands, connectors, Pathfinder) while preserving the core gateway and engine. Engine v1 removal is deferred to Phase 1b — `gateway-core`, `parser-graphql`, and `parser-sdl` have deep v1 dependencies that require separate refactoring.

**Tech Stack:** Rust (Cargo workspace), MIT license

---

## Important Context

### Engine v1 Deferral

Engine v1 (`engine/crates/engine/` and subcrates) is **deeply entangled** with kept crates:
- `gateway-core` depends on `engine`, `engine-parser`, `engine-validation`, `engine-value`, `registry-for-cache`
- `parser-graphql` depends on `engine`, `engine-parser`, `registry-v2`
- `parser-sdl` depends on `engine` (via path)
- `partial-caching` depends on v1 types

Removing v1 requires rewriting these crates to use v2 types — a separate planning effort. For now, v1 stays in the workspace and gets re-licensed.

### Workspace Hack

`grafbase-workspace-hack` is referenced in **~120 files** (Cargo.toml deps + `use grafbase_workspace_hack as _;` in lib.rs/main.rs). Removing it is mechanical but touches many files.

### Files Changed by This Plan

**Deleted directories:** `packages/`, `auth-testing/`, `workspace-hack/`, `cli/crates/backend/`, `cli/crates/common/`, `engine/crates/parser-openapi/`, `engine/crates/parser-postgres/`, `engine/crates/postgres-connector-types/`, `engine/crates/integration-tests/`, `engine/crates/federation-audit-tests/`

**Deleted CLI files:** `branch.rs`, `check.rs`, `create.rs`, `login.rs`, `logout.rs`, `publish.rs`, `schema.rs`, `subgraphs.rs`, `trust.rs`, `upgrade.rs`, plus their `cli_input/` counterparts

**Edited files:** `LICENSE`, root `Cargo.toml`, `cli/crates/cli/Cargo.toml`, `cli/crates/cli/src/main.rs`, `cli/crates/cli/src/cli_input.rs`, `cli/crates/cli/src/cli_input/sub_command.rs`, `cli/crates/cli/src/errors.rs`, `cli/crates/cli/src/output/report.rs`, `engine/crates/runtime/Cargo.toml`, `engine/crates/runtime/src/lib.rs`, `engine/crates/runtime/src/pg.rs` (deleted), `engine/crates/runtime-local/Cargo.toml`, `engine/crates/runtime-local/src/pg.rs` (deleted), plus ~120 files for workspace-hack removal

---

### Task 1: Replace LICENSE with MIT

**Files:**
- Modify: `/home/brian/projects/omni/lattice/LICENSE`

**Step 1: Replace LICENSE file contents with MIT license**

Replace the entire file with:

```text
MIT License

Copyright (c) 2024 Omni

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

**Step 2: Update workspace license in root Cargo.toml**

In `/home/brian/projects/omni/lattice/Cargo.toml`, change line 74:

```toml
license = "MIT"
```

**Step 3: Update individual crate licenses that don't inherit from workspace**

Search for `license = "MPL-2.0"` and `license = "Apache-2.0"` in all Cargo.toml files. Replace with `license = "MIT"` (only in crates that have an explicit `license` field rather than `license.workspace = true`).

Affected crates (with explicit license fields):
- `engine/crates/runtime/Cargo.toml` (line 7)
- `engine/crates/runtime-local/Cargo.toml` (line 7)
- `engine/crates/runtime-noop/Cargo.toml` (line 7)
- `engine/crates/gateway-core/Cargo.toml` (line 7)
- `engine/crates/engine-v2/Cargo.toml`
- `engine/crates/engine/Cargo.toml`
- All engine subcrates with explicit license fields
- `engine/crates/federated-graph/Cargo.toml`
- `engine/crates/composition/Cargo.toml`
- `engine/crates/validation/Cargo.toml`
- `engine/crates/graphql-schema-diff/Cargo.toml`

Run: `grep -rl 'license = "MPL-2.0"\|license = "Apache-2.0"' --include='Cargo.toml' .` to find all instances and replace with `license = "MIT"`.

**Step 4: Commit**

```bash
git add LICENSE Cargo.toml
git add -u -- '*.toml'
git commit -m "chore: re-license from MPL-2.0/Apache-2.0 to MIT"
```

---

### Task 2: Remove independent directories

**Files:**
- Delete: `packages/` (entire directory)
- Delete: `auth-testing/` (entire directory)

**Step 1: Remove packages and auth-testing directories**

```bash
rm -rf packages/ auth-testing/
```

These directories are not workspace members and have no Rust crate dependencies on them.

**Step 2: Remove related config files if they reference packages/**

Check and clean:
- `pnpm-lock.yaml`, `pnpm-workspace.yaml` — delete if present
- `turbo.json` — delete if present
- `.npmrc` — delete if present
- Any root `package.json` — delete if present

```bash
rm -f pnpm-lock.yaml pnpm-workspace.yaml turbo.json .npmrc package.json
```

**Step 3: Commit**

```bash
git add -A packages/ auth-testing/
git add -u
git commit -m "chore: remove packages/ and auth-testing/ directories"
```

---

### Task 3: Remove workspace-hack

The `workspace-hack` crate and its references exist in ~120 files. This is mechanical.

**Files:**
- Delete: `workspace-hack/` directory
- Modify: `/home/brian/projects/omni/lattice/Cargo.toml` (remove member + patch + workspace dep)
- Modify: ~60 `Cargo.toml` files (remove `grafbase-workspace-hack` dependency line)
- Modify: ~60 `.rs` files (remove `use grafbase_workspace_hack as _;` line)

**Step 1: Delete workspace-hack directory**

```bash
rm -rf workspace-hack/
```

**Step 2: Remove workspace-hack from root Cargo.toml**

In `/home/brian/projects/omni/lattice/Cargo.toml`:
- Remove `"workspace-hack"` from `[workspace] members` (line 31)
- Remove `grafbase-workspace-hack.path = "workspace-hack"` from `[patch.crates-io]` (line 69)
- Remove `grafbase-workspace-hack = { version = "0.1" }` from `[workspace.dependencies]` (line 236)

**Step 3: Remove grafbase-workspace-hack dependency from all crate Cargo.toml files**

Run a script to remove lines containing `grafbase-workspace-hack` from all Cargo.toml files:

```bash
find . -name 'Cargo.toml' -not -path './workspace-hack/*' -exec sed -i '/grafbase-workspace-hack/d' {} +
```

**Step 4: Remove `use grafbase_workspace_hack as _;` from all Rust source files**

```bash
find . -name '*.rs' -exec sed -i '/use grafbase_workspace_hack as _/d' {} +
```

**Step 5: Also remove cargo-hakari config files**

```bash
rm -f .config/hakari.toml .github/workflows/cargo-hakari.yml
```

**Step 6: Commit**

```bash
git add -A workspace-hack/ .config/hakari.toml .github/workflows/cargo-hakari.yml
git add -u
git commit -m "chore: remove workspace-hack and all references"
```

---

### Task 4: Remove CLI backend and common crates

**Files:**
- Delete: `cli/crates/backend/` directory
- Delete: `cli/crates/common/` directory

**Step 1: Delete the directories**

```bash
rm -rf cli/crates/backend/ cli/crates/common/
```

**Step 2: Commit** (the CLI won't compile yet — fixed in Task 5)

```bash
git add -A cli/crates/backend/ cli/crates/common/
git commit -m "chore: remove grafbase-local-backend and grafbase-local-common crates"
```

---

### Task 5: Strip CLI to compose/introspect/lint

This is the most surgical task. The CLI currently depends on `backend` and `common` crates for all the commands we're removing. We need to:
1. Remove the cut command source files
2. Rewrite main.rs to not depend on backend/common
3. Simplify errors, cli_input, and report

**Files:**
- Delete: `cli/crates/cli/src/branch.rs`
- Delete: `cli/crates/cli/src/check.rs`
- Delete: `cli/crates/cli/src/create.rs`
- Delete: `cli/crates/cli/src/login.rs`
- Delete: `cli/crates/cli/src/logout.rs`
- Delete: `cli/crates/cli/src/publish.rs`
- Delete: `cli/crates/cli/src/schema.rs`
- Delete: `cli/crates/cli/src/subgraphs.rs`
- Delete: `cli/crates/cli/src/trust.rs`
- Delete: `cli/crates/cli/src/upgrade.rs`
- Delete: `cli/crates/cli/src/prompts.rs`
- Delete: `cli/crates/cli/src/cli_input/branch.rs`
- Delete: `cli/crates/cli/src/cli_input/branch_ref.rs`
- Delete: `cli/crates/cli/src/cli_input/check.rs`
- Delete: `cli/crates/cli/src/cli_input/create.rs`
- Delete: `cli/crates/cli/src/cli_input/publish.rs`
- Delete: `cli/crates/cli/src/cli_input/schema.rs`
- Delete: `cli/crates/cli/src/cli_input/subgraphs.rs`
- Delete: `cli/crates/cli/src/cli_input/trust.rs`
- Delete: `cli/crates/cli/src/cli_input/project_ref.rs` (only used by cut commands)
- Delete: `cli/crates/cli/src/cli_input/federated_graph.rs` (only used by cut commands)
- Delete: `cli/crates/cli/src/cli_input/graph_ref_no_branch.rs` (only used by cut commands)
- Modify: `cli/crates/cli/src/main.rs`
- Modify: `cli/crates/cli/src/cli_input.rs`
- Modify: `cli/crates/cli/src/cli_input/sub_command.rs`
- Modify: `cli/crates/cli/src/errors.rs`
- Modify: `cli/crates/cli/src/output/report.rs`
- Modify: `cli/crates/cli/Cargo.toml`

**Step 1: Delete cut command files**

```bash
cd cli/crates/cli/src
rm -f branch.rs check.rs create.rs login.rs logout.rs publish.rs schema.rs subgraphs.rs trust.rs upgrade.rs prompts.rs
cd cli_input
rm -f branch.rs branch_ref.rs check.rs create.rs publish.rs schema.rs subgraphs.rs trust.rs project_ref.rs federated_graph.rs graph_ref_no_branch.rs
```

**Step 2: Rewrite main.rs**

Replace `/home/brian/projects/omni/lattice/cli/crates/cli/src/main.rs` with:

```rust
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
```

**Step 3: Rewrite cli_input.rs**

Replace `/home/brian/projects/omni/lattice/cli/crates/cli/src/cli_input.rs` with:

```rust
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
```

**Step 4: Rewrite sub_command.rs**

Replace `/home/brian/projects/omni/lattice/cli/crates/cli/src/cli_input/sub_command.rs` with:

```rust
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
```

**Step 5: Rewrite errors.rs**

Replace `/home/brian/projects/omni/lattice/cli/crates/cli/src/errors.rs` with:

```rust
use std::io;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CliError {
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
```

**Step 6: Rewrite report.rs**

Replace `/home/brian/projects/omni/lattice/cli/crates/cli/src/output/report.rs` with:

```rust
use crate::{
    errors::CliError,
    watercolor::{self, watercolor},
};
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
    watercolor::output!("✅ No issues found in your schema", @BrightGreen)
}

pub fn lint_warning(warning: String) {
    watercolor::output!("⚠️ [Warning] {warning}", @BrightYellow);
}
```

**Step 7: Update CLI Cargo.toml**

Replace `/home/brian/projects/omni/lattice/cli/crates/cli/Cargo.toml` with:

```toml
[package]
name = "lattice-cli"
description = "The Lattice command line interface"
categories = ["command-line-utilities"]
readme = "README.md"
version.workspace = true
edition.workspace = true
license.workspace = true
homepage.workspace = true
repository.workspace = true

[dependencies]
clap = { version = "4.5.4", features = ["cargo", "wrap_help", "derive", "env"] }
clap_complete = "4.5.2"
colored = "2.1.0"
log = "0.4.21"
mimalloc = "0.1.41"
reqwest = { workspace = true, features = [
    "rustls-tls",
    "json",
], default-features = false }
rustls = { workspace = true, features = ["ring", "tls12"] }
serde = "1.0.199"
serde_derive = "1.0.199"
serde_json.workspace = true
strum = { version = "0.26.2", features = ["derive"] }
thiserror = "1.0.59"
tokio = { version = "1.37.0", features = ["full"] }
tracing-subscriber = { workspace = true, features = ["env-filter"] }
url = "2.5.0"

grafbase-graphql-introspection.workspace = true
graphql-lint.workspace = true
graph-ref = { path = "../../../graph-ref" }
graphql-composition.workspace = true
regex = "1"
serde_yaml = "0.9"

[dev-dependencies]
insta = { workspace = true, features = ["json", "redactions", "yaml"] }
tempfile = "3.10.1"
tokio = { version = "1.37.0", features = ["full"] }

[[bin]]
name = "lattice"
path = "src/main.rs"

[features]
default = []

[lints]
workspace = true
```

**Step 8: Commit**

```bash
git add -u
git add -A cli/crates/cli/src/
git commit -m "feat(cli): strip CLI to compose, introspect, and lint commands"
```

---

### Task 6: Remove connector crates

**Files:**
- Delete: `engine/crates/parser-openapi/`
- Delete: `engine/crates/parser-postgres/`
- Delete: `engine/crates/postgres-connector-types/`
- Delete: `engine/crates/integration-tests/`
- Delete: `engine/crates/federation-audit-tests/`

**Step 1: Delete connector and test directories**

```bash
rm -rf engine/crates/parser-openapi/
rm -rf engine/crates/parser-postgres/
rm -rf engine/crates/postgres-connector-types/
rm -rf engine/crates/integration-tests/
rm -rf engine/crates/federation-audit-tests/
```

**Step 2: Remove postgres dependency from runtime crate**

In `/home/brian/projects/omni/lattice/engine/crates/runtime/Cargo.toml`, remove line 36:
```
postgres-connector-types = { path = "../postgres-connector-types" }
```

In `/home/brian/projects/omni/lattice/engine/crates/runtime/src/lib.rs`, remove line 17:
```rust
pub mod pg;
```

Delete `/home/brian/projects/omni/lattice/engine/crates/runtime/src/pg.rs`.

**Step 3: Remove postgres dependency from runtime-local crate**

In `/home/brian/projects/omni/lattice/engine/crates/runtime-local/Cargo.toml`, remove line 39:
```
postgres-connector-types = { path = "../postgres-connector-types" }
```

Check `/home/brian/projects/omni/lattice/engine/crates/runtime-local/src/` for `pg.rs` and remove it. Also remove `pub mod pg;` from `lib.rs` if present.

**Step 4: Remove connector workspace deps from root Cargo.toml**

In `/home/brian/projects/omni/lattice/Cargo.toml`, remove these lines from `[workspace.dependencies]`:
```toml
parser-openapi = { path = "engine/crates/parser-openapi" }
parser-postgres = { path = "engine/crates/parser-postgres" }
postgres-connector-types = { path = "engine/crates/postgres-connector-types" }
```

Also remove any other workspace dependencies that are ONLY used by cut crates. Check for:
- `deadpool-postgres` (used by runtime-local optionally — keep)
- `tokio-postgres` (used by runtime-local — keep)

**Step 5: Commit**

```bash
git add -A engine/crates/parser-openapi/ engine/crates/parser-postgres/ engine/crates/postgres-connector-types/ engine/crates/integration-tests/ engine/crates/federation-audit-tests/
git add -u
git commit -m "chore: remove connector crates and integration tests"
```

---

### Task 7: Clean root Cargo.toml workspace members

**Files:**
- Modify: `/home/brian/projects/omni/lattice/Cargo.toml`

**Step 1: Remove cut members and deps**

Remove from `[workspace] members`:
- `"workspace-hack"` (already done in Task 3)

Remove from `[workspace] exclude`:
- `"engine/crates/wasi-component-loader/examples"` (check if wasi-component-loader is still needed)

Remove unused workspace dependencies that pointed to removed crates. These include any deps only consumed by cut crates. Run `cargo check` to identify unused workspace deps.

Also remove the `grafbase-workspace-hack.path` line from `[patch.crates-io]` if not already done.

**Step 2: Run cargo check**

```bash
cargo check 2>&1
```

Fix any compilation errors. Common issues:
- Missing imports from removed crates
- Unused `use` statements
- Feature flags referencing removed crates

**Step 3: Run cargo check on the CLI specifically**

```bash
cargo check -p lattice-cli 2>&1
```

**Step 4: Commit**

```bash
git add -u
git commit -m "chore: clean workspace Cargo.toml and fix compilation"
```

---

### Task 8: Remove Pathfinder references

Most Pathfinder code lives in `cli/crates/common/` (already deleted in Task 4) and `engine/crates/engine/` (v1, kept but will be cleaned).

**Files:**
- Check: `engine/crates/engine/src/http/pathfinder_source.rs`
- Check: `engine/crates/engine/src/http/pathfinder.html`

**Step 1: Remove Pathfinder from engine v1**

Since engine v1 is still in the workspace (deferred removal), check if Pathfinder files cause issues. If the engine v1 http module exports pathfinder and nothing uses it externally:

Delete:
- `engine/crates/engine/src/http/pathfinder_source.rs`
- `engine/crates/engine/src/http/pathfinder.html`

Remove `pub mod pathfinder_source;` from `engine/crates/engine/src/http/mod.rs` (or wherever it's declared).

**Step 2: Verify no remaining pathfinder references**

```bash
grep -ri 'pathfinder' --include='*.rs' --include='*.toml' --include='*.html' .
```

Should return nothing (except possibly the design doc).

**Step 3: Commit**

```bash
git add -u
git commit -m "chore: remove all Pathfinder references"
```

---

### Task 9: Final verification

**Step 1: Full cargo check**

```bash
cargo check 2>&1
```

**Step 2: Run clippy**

```bash
cargo clippy 2>&1
```

Fix any warnings. Common issues after mass removal:
- `unused_crate_dependencies` (workspace lint denies this)
- Dead code warnings

**Step 3: Run tests on kept crates**

```bash
cargo test -p lattice-cli 2>&1
cargo test -p graphql-lint 2>&1
cargo test -p graphql-composition 2>&1
```

**Step 4: Verify the CLI binary runs**

```bash
cargo run -p lattice-cli -- --help
cargo run -p lattice-cli -- compose --help
cargo run -p lattice-cli -- lint --help
cargo run -p lattice-cli -- introspect --help
```

**Step 5: Commit any remaining fixes**

```bash
git add -u
git commit -m "chore: fix compilation and lint issues after cleanup"
```

---

## Summary of Commits

1. `chore: re-license from MPL-2.0/Apache-2.0 to MIT`
2. `chore: remove packages/ and auth-testing/ directories`
3. `chore: remove workspace-hack and all references`
4. `chore: remove grafbase-local-backend and grafbase-local-common crates`
5. `feat(cli): strip CLI to compose, introspect, and lint commands`
6. `chore: remove connector crates and integration tests`
7. `chore: clean workspace Cargo.toml and fix compilation`
8. `chore: remove all Pathfinder references`
9. `chore: fix compilation and lint issues after cleanup`

## Next Phase

After this plan is complete, proceed to:
- **Phase 1b:** Remove engine v1 (requires refactoring gateway-core, parser-graphql, parser-sdl to use v2 types)
- **Phase 2:** Restructure directory layout into `core/`, `sdk/`, `cli/`, `tools/`
- **Phase 3:** Rebrand remaining grafbase references
