# Lattice Compose CLI Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a `lattice compose` command that composes subgraph schemas into a federated supergraph SDL.

**Architecture:** Add a new CLI subcommand that parses a YAML config file, fetches subgraph schemas via introspection or local files, composes them using the existing `graphql-composition` crate, and outputs the federated SDL.

**Tech Stack:** Rust, clap, serde_yaml, graphql-composition, grafbase-graphql-introspection

---

## Task 1: Add Dependencies to Cargo.toml

**Files:**
- Modify: `cli/crates/cli/Cargo.toml`

**Step 1: Add new dependencies**

Add these lines to the `[dependencies]` section:

```toml
graphql-composition.workspace = true
serde_yaml = "0.9"
```

**Step 2: Verify workspace has graphql-composition**

Run:
```bash
grep "graphql-composition" Cargo.toml
```
Expected: Should show the workspace dependency definition

**Step 3: Commit**

```bash
git add cli/crates/cli/Cargo.toml
git commit -m "build(cli): add graphql-composition and serde_yaml dependencies"
```

---

## Task 2: Add Compose Command Arguments

**Files:**
- Create: `cli/crates/cli/src/cli_input/compose.rs`
- Modify: `cli/crates/cli/src/cli_input.rs`

**Step 1: Create the compose command args**

Create `cli/crates/cli/src/cli_input/compose.rs`:

```rust
use clap::Parser;
use std::path::PathBuf;

/// Compose subgraph schemas into a federated supergraph
#[derive(Debug, Parser)]
pub struct ComposeCommand {
    /// Path to the supergraph config file (YAML)
    #[arg(short, long)]
    pub config: PathBuf,

    /// Output file path (defaults to stdout)
    #[arg(short, long)]
    pub output: Option<PathBuf>,
}
```

**Step 2: Export from cli_input.rs**

Add to `cli/crates/cli/src/cli_input.rs`:

After line 5 (the other mod declarations), add:
```rust
mod compose;
```

After line 25 (the other pub use statements), add:
```rust
pub(crate) use compose::ComposeCommand;
```

**Step 3: Verify it compiles**

Run:
```bash
cargo check -p lattice-cli
```
Expected: Compiles without errors

**Step 4: Commit**

```bash
git add cli/crates/cli/src/cli_input/compose.rs cli/crates/cli/src/cli_input.rs
git commit -m "feat(cli): add compose command arguments"
```

---

## Task 3: Add Compose to SubCommand Enum

**Files:**
- Modify: `cli/crates/cli/src/cli_input/sub_command.rs`

**Step 1: Add Compose variant**

In `cli/crates/cli/src/cli_input/sub_command.rs`:

Add import at top (after line 6):
```rust
use super::ComposeCommand;
```

Add variant to SubCommand enum (after line 40, before the closing brace):
```rust
    /// Compose subgraph schemas into a federated supergraph
    Compose(ComposeCommand),
```

Add match arm in `argument_names` method (after line 57):
```rust
            SubCommand::Compose(_) => None,
```

**Step 2: Verify it compiles**

Run:
```bash
cargo check -p lattice-cli
```
Expected: Compiles without errors

**Step 3: Commit**

```bash
git add cli/crates/cli/src/cli_input/sub_command.rs
git commit -m "feat(cli): add Compose variant to SubCommand enum"
```

---

## Task 4: Add Error Types for Compose

**Files:**
- Modify: `cli/crates/cli/src/errors.rs`

**Step 1: Add compose error variants**

In `cli/crates/cli/src/errors.rs`, add these variants to the `CliError` enum (after line 58):

```rust
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
```

**Step 2: Verify it compiles**

Run:
```bash
cargo check -p lattice-cli
```
Expected: Compiles without errors

**Step 3: Commit**

```bash
git add cli/crates/cli/src/errors.rs
git commit -m "feat(cli): add compose error types"
```

---

## Task 5: Implement Compose Config Parser

**Files:**
- Create: `cli/crates/cli/src/compose/config.rs`
- Create: `cli/crates/cli/src/compose.rs` (module declaration)

**Step 1: Create compose module**

Create `cli/crates/cli/src/compose.rs`:

```rust
mod config;

pub(crate) use config::ComposeConfig;

use crate::{cli_input::ComposeCommand, errors::CliError};
use graphql_composition::{compose, render_federated_sdl, Subgraphs};
use std::{fs, io::Write};
use tokio::runtime::Runtime;

pub fn compose_command(cmd: ComposeCommand) -> Result<(), CliError> {
    let config = ComposeConfig::load(&cmd.config)?;
    let mut subgraphs = Subgraphs::default();

    let rt = Runtime::new().unwrap();

    for (name, subgraph) in &config.subgraphs {
        let schema = rt.block_on(subgraph.fetch_schema(&config.headers))?;
        subgraphs
            .ingest_str(&schema, name, subgraph.routing_url())
            .map_err(|e| CliError::CompositionFailed(e.to_string()))?;
    }

    let result = compose(&subgraphs);

    // Print warnings to stderr
    for diagnostic in result.diagnostics.iter_warnings() {
        eprintln!("Warning: {diagnostic}");
    }

    // Check for errors
    if result.diagnostics.any_fatal() {
        let errors: Vec<_> = result.diagnostics.iter_errors().map(|d| d.to_string()).collect();
        return Err(CliError::CompositionFailed(errors.join("\n")));
    }

    let federated_graph = result.federated_graph.expect("graph exists if no fatal errors");
    let sdl = render_federated_sdl(&federated_graph);

    match cmd.output {
        Some(path) => {
            fs::write(&path, &sdl).map_err(|e| CliError::WriteComposeOutput(path, e))?;
            eprintln!("Composed {} subgraphs into {}", config.subgraphs.len(), cmd.output.as_ref().unwrap().display());
        }
        None => {
            std::io::stdout().write_all(sdl.as_bytes()).unwrap();
        }
    }

    Ok(())
}
```

**Step 2: Create config parser**

Create `cli/crates/cli/src/compose/config.rs`:

```rust
use crate::errors::CliError;
use serde::Deserialize;
use std::{collections::BTreeMap, fs, path::Path};

#[derive(Debug, Deserialize)]
pub struct ComposeConfig {
    pub subgraphs: BTreeMap<String, SubgraphConfig>,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
pub struct SubgraphConfig {
    /// URL for introspection (also used as routing_url if schema is not provided)
    pub url: Option<String>,
    /// Path to local schema file
    pub schema: Option<String>,
    /// Routing URL (required if only schema is provided)
    pub routing_url: Option<String>,
}

impl ComposeConfig {
    pub fn load(path: &Path) -> Result<Self, CliError> {
        let content = fs::read_to_string(path)
            .map_err(|e| CliError::ReadComposeConfig(path.to_path_buf(), e))?;

        // Expand environment variables
        let content = expand_env_vars(&content)?;

        serde_yaml::from_str(&content)
            .map_err(|e| CliError::ParseComposeConfig(path.to_path_buf(), e.to_string()))
    }
}

impl SubgraphConfig {
    pub fn routing_url(&self) -> &str {
        self.routing_url
            .as_deref()
            .or(self.url.as_deref())
            .expect("validation ensures url or routing_url exists")
    }

    pub async fn fetch_schema(&self, headers: &BTreeMap<String, String>) -> Result<String, CliError> {
        match (&self.schema, &self.url) {
            (Some(path), _) => {
                // Read from file
                let path = Path::new(path);
                fs::read_to_string(path)
                    .map_err(|e| CliError::ReadSubgraphSchema(path.to_path_buf(), e))
            }
            (None, Some(url)) => {
                // Introspect from URL
                let header_pairs: Vec<_> = headers
                    .iter()
                    .map(|(k, v)| (k.as_str(), v.as_str()))
                    .collect();

                grafbase_graphql_introspection::introspect(url, &header_pairs)
                    .await
                    .map_err(CliError::Introspection)
            }
            (None, None) => {
                unreachable!("validation ensures url or schema exists")
            }
        }
    }
}

fn expand_env_vars(content: &str) -> Result<String, CliError> {
    let mut result = content.to_string();
    let re = regex::Regex::new(r"\$\{([A-Z_][A-Z0-9_]*)\}").unwrap();

    for cap in re.captures_iter(content) {
        let var_name = &cap[1];
        let var_value = std::env::var(var_name)
            .map_err(|_| CliError::EnvVarNotSet(var_name.to_string()))?;
        result = result.replace(&cap[0], &var_value);
    }

    Ok(result)
}
```

**Step 3: Add regex dependency**

Add to `cli/crates/cli/Cargo.toml` dependencies:

```toml
regex = "1"
```

**Step 4: Verify it compiles**

Run:
```bash
cargo check -p lattice-cli
```
Expected: Compiles without errors

**Step 5: Commit**

```bash
git add cli/crates/cli/src/compose.rs cli/crates/cli/src/compose/config.rs cli/crates/cli/Cargo.toml
git commit -m "feat(cli): implement compose config parser and command"
```

---

## Task 6: Wire Up Compose Command in main.rs

**Files:**
- Modify: `cli/crates/cli/src/main.rs`

**Step 1: Add compose module**

After line 8 (the check module), add:
```rust
mod compose;
```

**Step 2: Add match arm for Compose**

In the match statement (around line 90), add after the Lint arm (line 113):
```rust
        SubCommand::Compose(cmd) => compose::compose_command(cmd),
```

**Step 3: Verify it compiles and runs**

Run:
```bash
cargo build -p lattice-cli
./target/debug/lattice compose --help
```
Expected: Shows help for compose command

**Step 4: Commit**

```bash
git add cli/crates/cli/src/main.rs
git commit -m "feat(cli): wire up compose command"
```

---

## Task 7: Test the Compose Command

**Step 1: Create test config file**

Create `test-compose.yaml`:
```yaml
subgraphs:
  test:
    schema: ./test-schema.graphql
    routing_url: http://localhost:4001/graphql
```

Create `test-schema.graphql`:
```graphql
type Query {
  hello: String!
}
```

**Step 2: Run compose**

Run:
```bash
./target/debug/lattice compose --config test-compose.yaml
```
Expected: Outputs federated SDL to stdout

**Step 3: Test with output file**

Run:
```bash
./target/debug/lattice compose --config test-compose.yaml --output supergraph.graphql
cat supergraph.graphql
```
Expected: Writes federated SDL to file

**Step 4: Clean up test files**

```bash
rm test-compose.yaml test-schema.graphql supergraph.graphql
```

**Step 5: Final commit**

```bash
git add -A
git commit -m "feat(cli): complete lattice compose command"
```

---

## Checklist

- [ ] Task 1: Add dependencies to Cargo.toml
- [ ] Task 2: Add compose command arguments
- [ ] Task 3: Add Compose to SubCommand enum
- [ ] Task 4: Add error types for compose
- [ ] Task 5: Implement compose config parser
- [ ] Task 6: Wire up compose command in main.rs
- [ ] Task 7: Test the compose command
