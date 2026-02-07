use clap::Parser as _;
use color_eyre::{Result, eyre::Ok, install};
use std::path::PathBuf;

mod actions_parser;
mod ast;

#[derive(Debug, clap::Parser)]
#[command(name = "ci-preflight")]
struct Cli {
    /// Parse a workflow file and print debug output.
    #[arg(long = "parse-only", value_name = "FILE")]
    parse_only: Option<PathBuf>,
}

fn main() -> Result<()> {
    install()?;
    let cli = Cli::parse();
    if let Some(path) = cli.parse_only {
        let text = std::fs::read_to_string(&path)?;
        let mut source_map = actions_parser::source_map::SourceMap::new();
        let source_id = source_map.add_yaml(path, "workflow".to_string(), text);
        let (root, arena) = actions_parser::parse_actions_yaml(&mut source_map, &source_id)?;
        let tree = actions_parser::format_actions_tree(&arena, &root);
        println!("{}", tree);
        return Ok(());
    }

    println!("Hello, world!");
    Ok(())
}
