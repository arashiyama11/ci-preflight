use color_eyre::Result;
use std::path::PathBuf;

use crate::{analyzer, executer, parser};

pub(crate) fn run(cli: crate::Cli) -> Result<()> {
    if let Some(path) = cli.parse_only {
        run_parse_only(path)?;
        return Ok(());
    }

    if let Some(path) = cli.check_tools {
        run_check_tools(path)?;
        return Ok(());
    }

    if let Some(path) = cli.print_cmd_kind {
        run_print_cmd_kind(path)?;
        return Ok(());
    }

    Ok(())
}

fn run_parse_only(path: PathBuf) -> Result<()> {
    let text = std::fs::read_to_string(&path)?;
    let mut source_map = parser::source_map::SourceMap::new();
    let source_id = source_map.add_yaml(path, "workflow".to_string(), text);
    let (root, arena, errors) = parser::parse_actions_yaml(&mut source_map, &source_id)?;
    let tree = parser::format_actions_tree(&arena, &root);
    println!("{}", tree);
    if !errors.is_empty() {
        eprintln!("errors:");
        for err in errors {
            eprintln!("- {}", err);
        }
    }
    Ok(())
}

fn run_check_tools(path: PathBuf) -> Result<()> {
    let workflow_base_dir = path.parent().map(|p| p.to_path_buf());
    let text = std::fs::read_to_string(&path)?;
    let mut source_map = parser::source_map::SourceMap::new();
    let source_id = source_map.add_yaml(path, "workflow".to_string(), text);
    let (root, arena, errors) = parser::parse_actions_yaml(&mut source_map, &source_id)?;

    if !errors.is_empty() {
        eprintln!("parse warnings:");
        for err in errors {
            eprintln!("- {}", err);
        }
    }

    let catalog = analyzer::action_catalog::load_action_catalog()?;
    let report = executer::env_check::check_workflow_tools_with_base_dir(
        root,
        &arena,
        &catalog,
        workflow_base_dir.as_deref(),
    );

    println!("required: {}", report.required_tools.join(", "));
    println!("found: {}", report.found_tools.join(", "));
    println!("missing: {}", report.missing_tools.join(", "));
    println!("unknown_commands: {}", report.unknown_commands.join(", "));
    println!("unknown_uses: {}", report.unknown_uses.join(", "));

    match report.status() {
        executer::env_check::PreflightStatus::Pass => {
            println!("PASS: all required tools are installed");
            Ok(())
        }
        executer::env_check::PreflightStatus::FailMissingTools => {
            eprintln!(
                "FAIL: missing required tools: {}",
                report.missing_tools.join(", ")
            );
            std::process::exit(2);
        }
    }
}

fn run_print_cmd_kind(path: PathBuf) -> Result<()> {
    let text = std::fs::read_to_string(&path)?;
    let mut source_map = parser::source_map::SourceMap::new();
    let source_id = source_map.add_yaml(path, "workflow".to_string(), text.clone());
    let (root, arena, errors) = parser::parse_actions_yaml(&mut source_map, &source_id)?;

    if !errors.is_empty() {
        eprintln!("parse warnings:");
        for err in errors {
            eprintln!("- {}", err);
        }
    }

    let analysis = analyzer::analyze_actions(root, &arena);
    let annotated = analyzer::annotate_yaml_with_cmd_kind(&text, &analysis);
    let colored = colorize_cmd_kind_annotations(&annotated);
    print!("{colored}");
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
