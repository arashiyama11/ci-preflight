use clap::Parser as _;
use color_eyre::{Result, install};
use std::path::PathBuf;

mod analyzer;
mod commands;
mod executer;
mod optimizer;
mod parser;

#[derive(Debug, clap::Parser)]
#[command(name = "ci-preflight")]
struct Cli {
    /// Parse a workflow file and print debug output.
    #[arg(long = "parse-only", value_name = "FILE")]
    parse_only: Option<PathBuf>,

    /// Parse a workflow file and fail if required tools are missing on current PATH.
    #[arg(long = "check-tools", value_name = "FILE")]
    check_tools: Option<PathBuf>,

    /// Parse a workflow file and print `command --- CmdKind` lines.
    #[arg(long = "print-cmd-kind", value_name = "FILE")]
    print_cmd_kind: Option<PathBuf>,
}

fn main() -> Result<()> {
    install()?;
    commands::run(Cli::parse())
}
