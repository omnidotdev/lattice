<p align="center">
  <a href="https://lattice.omni.dev">
    <img src="https://lattice.omni.dev/images/other/lattice-logo-circle.png" height="96">
  </a>
  <h3 align="center">Lattice CLI</h3>
</p>

<p align="center">
  <a href="https://github.com/omnidotdev/lattice/actions/workflows/cli-build.yml">
    <img alt="CLI build status" src=https://github.com/omnidotdev/lattice/actions/workflows/cli-build.yml/badge.svg>
  </a>
</p>

Welcome to the Lattice CLI!

This is a cargo workspace with several crates relating to the Lattice CLI and developer tooling

The directories in the `crates` directory in the root of the repo are organized as follows:

| Directory                  | Description                             |
| -------------------------- | --------------------------------------- |
| [backend](crates/backend/) | Universal backend for Lattice devtools  |
| [cli](crates/cli/)         | Command Line Interface                  |
| [common](crates/common/)   | Shared functions and utilities          |
| [server](crates/server/)   | Wrapper for the API worker              |
