use clap::Parser as _;
use color_eyre::{Result, eyre::Ok, install};
use std::path::PathBuf;

mod action_catalog;
mod actions_parser;
mod analysis;
mod cmd_kind_rules;
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

    /// Parse a workflow file and print `command --- CmdKind` lines.
    #[arg(long = "print-cmd-kind", value_name = "FILE")]
    print_cmd_kind: Option<PathBuf>,
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

        let catalog = action_catalog::load_action_catalog()?;
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

    if let Some(path) = cli.print_cmd_kind {
        let text = std::fs::read_to_string(&path)?;
        let mut source_map = actions_parser::source_map::SourceMap::new();
        let source_id = source_map.add_yaml(path, "workflow".to_string(), text.clone());
        let (root, arena, errors) =
            actions_parser::parse_actions_yaml(&mut source_map, &source_id)?;

        if !errors.is_empty() {
            eprintln!("parse warnings:");
            for err in errors {
                eprintln!("- {}", err);
            }
        }

        let analysis = analysis::analyze_actions(root, &arena);
        let annotated = analysis::annotate_yaml_with_cmd_kind(&text, &analysis);
        let colored = colorize_cmd_kind_annotations(&annotated);
        print!("{colored}");
        return Ok(());
    }

    println!("Hello, world!");
    Ok(())
}

fn colorize_cmd_kind_annotations(text: &str) -> String {
    const GREEN: &str = "\x1b[32m";
    const RESET: &str = "\x1b[0m";

    text.lines()
        .map(|line| {
            if let Some((left, right)) = line.split_once(" --- ") {
                let colored_rhs = right
                    .split(" && ")
                    .map(|part| format!("{GREEN}{part}{RESET}"))
                    .collect::<Vec<_>>()
                    .join(" && ");
                format!("{left} --- {colored_rhs}")
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + if text.ends_with('\n') { "\n" } else { "" }
}

#[cfg(test)]
mod tests {
    use super::colorize_cmd_kind_annotations;

    #[test]
    fn colorize_only_cmd_kind_side() {
        let input = "run: cargo test --- Test\n";
        let out = colorize_cmd_kind_annotations(input);
        assert!(out.contains("run: cargo test --- \u{1b}[32mTest\u{1b}[0m\n"));
    }

    #[test]
    fn preserve_lines_without_annotation() {
        let input = "name: CI\njobs:\n";
        let out = colorize_cmd_kind_annotations(input);
        assert_eq!(out, input);
    }
}
