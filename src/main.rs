use clap::Parser as _;
use color_eyre::{Result, eyre::Ok, install};
use std::path::PathBuf;

mod action_catalog;
mod actions_parser;
mod analysis;
mod env_check;

#[derive(Debug, clap::Parser)]
#[command(name = "ci-preflight")]
struct Cli {
    /// Parse a workflow file and print debug output.
    #[arg(long = "parse-only", value_name = "FILE")]
    parse_only: Option<PathBuf>,

    /// Parse a workflow file and fail if required tools are missing on current PATH.
    #[arg(long = "check-tools", value_name = "FILE")]
    check_tools: Option<PathBuf>,
}

fn main() -> Result<()> {
    install()?;
    let cli = Cli::parse();
    if let Some(path) = cli.parse_only {
        let text = std::fs::read_to_string(&path)?;
        let mut source_map = actions_parser::source_map::SourceMap::new();
        let source_id = source_map.add_yaml(path, "workflow".to_string(), text);
        let (root, arena, errors) =
            actions_parser::parse_actions_yaml(&mut source_map, &source_id)?;
        let tree = actions_parser::format_actions_tree(&arena, &root);
        println!("{}", tree);
        if !errors.is_empty() {
            eprintln!("errors:");
            for err in errors {
                eprintln!("- {}", err);
            }
        }
        return Ok(());
    }

    if let Some(path) = cli.check_tools {
        let text = std::fs::read_to_string(&path)?;
        let mut source_map = actions_parser::source_map::SourceMap::new();
        let source_id = source_map.add_yaml(path, "workflow".to_string(), text);
        let (root, arena, errors) =
            actions_parser::parse_actions_yaml(&mut source_map, &source_id)?;

        if !errors.is_empty() {
            eprintln!("parse warnings:");
            for err in errors {
                eprintln!("- {}", err);
            }
        }

        let catalog = action_catalog::load_well_known_actions(std::path::Path::new(
            "data/well_known_actions.json",
        ))?;
        let report = env_check::check_workflow_tools(root, &arena, &catalog);

        println!("required: {}", report.required_tools.join(", "));
        println!("found: {}", report.found_tools.join(", "));
        println!("missing: {}", report.missing_tools.join(", "));
        println!("unknown_commands: {}", report.unknown_commands.join(", "));
        println!("unknown_uses: {}", report.unknown_uses.join(", "));

        match report.status() {
            env_check::PreflightStatus::Pass => {
                println!("PASS: all required tools are installed");
                return Ok(());
            }
            env_check::PreflightStatus::FailMissingTools => {
                eprintln!(
                    "FAIL: missing required tools: {}",
                    report.missing_tools.join(", ")
                );
                std::process::exit(2);
            }
        }
    }

    println!("Hello, world!");
    Ok(())
}
