#![allow(dead_code)]

use crate::actions_parser::actions_ast::ActionsAst;
use crate::actions_parser::arena::{AstArena, AstId};
use crate::actions_parser::sh_parser::sh_ast::ShAstNode;
use crate::action_catalog::normalize_uses;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CmdKind {
    EnvSetup,
    TestSetup,
    Test,
    Other,
}

impl CmdKind {
    fn as_str(&self) -> &'static str {
        match self {
            CmdKind::EnvSetup => "EnvSetup",
            CmdKind::TestSetup => "TestSetup",
            CmdKind::Test => "Test",
            CmdKind::Other => "Other",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpecialActionKind {
    Checkout,
    ArtifactUpload,
    ArtifactDownload,
}

impl SpecialActionKind {
    fn as_str(&self) -> &'static str {
        match self {
            SpecialActionKind::Checkout => "Checkout",
            SpecialActionKind::ArtifactUpload => "ArtifactUpload",
            SpecialActionKind::ArtifactDownload => "ArtifactDownload",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Attr {
    pub kind: Option<CmdKind>,
    pub special_action: Option<SpecialActionKind>,
    pub confidence: f32,
    pub notes: Vec<String>,
    pub tools: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct CommandPlan {
    pub ast_id: AstId,
    pub attr: Attr,
}

#[derive(Clone, Debug)]
pub struct StepPlan {
    pub step_id: AstId,
    pub commands: Vec<CommandPlan>,
}

#[derive(Clone, Debug, Default)]
pub struct AnalysisResult {
    pub steps: Vec<StepPlan>,
    pub unknown_uses: Vec<String>,
    pub errors: Vec<AnalysisError>,
}

#[derive(Clone, Debug)]
pub struct AnalysisError {
    pub message: String,
    pub at: Option<AstId>,
}

#[derive(Clone, Debug)]
pub struct PlanOptions {
    pub include_env_setup: bool,
    pub include_other: bool,
}

#[derive(Clone, Debug)]
pub struct ExecutionPlan {
    pub commands: Vec<PlannedCommand>,
}

#[derive(Clone, Debug)]
pub struct PlannedCommand {
    pub ast_id: AstId,
    pub kind: CmdKind,
}

pub fn analyze_actions(root: AstId, arena: &AstArena) -> AnalysisResult {
    let mut steps = Vec::new();
    let mut unknown_uses = Vec::new();
    collect_steps(root, arena, &mut steps, &mut unknown_uses);
    let result = AnalysisResult {
        steps,
        unknown_uses,
        errors: Vec::new(),
    };
    result
}

pub fn analyze_step(step_id: AstId, arena: &AstArena) -> StepPlan {
    let commands = match arena.get_actions(&step_id) {
        ActionsAst::RunStep { run, .. } => analyze_run_step(*run, arena),
        ActionsAst::UsesStep { uses, .. } => vec![CommandPlan {
            ast_id: step_id,
            attr: analyze_uses_step(uses),
        }],
        _ => Vec::new(),
    };
    StepPlan { step_id, commands }
}

pub fn analyze_run_step(run_id: AstId, arena: &AstArena) -> Vec<CommandPlan> {
    extract_simple_commands(run_id, arena)
        .into_iter()
        .map(|cmd_id| CommandPlan {
            ast_id: cmd_id,
            attr: analyze_simple_command(cmd_id, arena),
        })
        .collect()
}

pub fn analyze_simple_command(cmd_id: AstId, arena: &AstArena) -> Attr {
    let mut attr = Attr {
        confidence: 0.4,
        ..Attr::default()
    };
    let Some(argv) = read_simple_command_words(cmd_id, arena) else {
        return attr;
    };
    if argv.is_empty() {
        return attr;
    }
    attr.tools.push(argv.join(" "));
    attr.kind = Some(classify_simple_command_from_words(&argv));
    attr.confidence = 0.9;
    attr
}

fn analyze_uses_step(uses: &str) -> Attr {
    let mut attr = Attr {
        confidence: 0.9,
        ..Attr::default()
    };
    let special_action = classify_uses_special_action(uses);
    let kind = classify_uses_kind(uses);
    attr.kind = Some(kind);
    attr.special_action = special_action;
    attr.tools.push(uses.to_string());
    attr
}

pub fn extract_simple_commands(run_id: AstId, arena: &AstArena) -> Vec<AstId> {
    let mut out = Vec::new();
    collect_simple_commands(run_id, arena, &mut out);
    out
}

pub fn build_execution_plan(analysis: &AnalysisResult, opts: &PlanOptions) -> ExecutionPlan {
    let mut commands = Vec::new();
    for step in &analysis.steps {
        for command in &step.commands {
            let kind = command.attr.kind.clone().unwrap_or(CmdKind::Other);
            if matches!(kind, CmdKind::EnvSetup) && !opts.include_env_setup {
                continue;
            }
            if matches!(kind, CmdKind::Other) && !opts.include_other {
                continue;
            }
            commands.push(PlannedCommand {
                ast_id: command.ast_id,
                kind,
            });
        }
    }
    ExecutionPlan { commands }
}

pub fn format_cmd_kind_lines(analysis: &AnalysisResult) -> Vec<String> {
    let mut lines = Vec::new();
    for step in &analysis.steps {
        if step.commands.is_empty() {
            continue;
        }
        let left = step
            .commands
            .iter()
            .map(|cmd| {
                cmd.attr
                    .tools
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "<unknown>".to_string())
            })
            .collect::<Vec<_>>()
            .join(" && ");
        let right = step
            .commands
            .iter()
            .map(format_command_kind)
            .collect::<Vec<_>>()
            .join(" && ");
        lines.push(format!("{left} --- {right}"));
    }
    lines
}

pub fn annotate_yaml_with_cmd_kind(yaml: &str, analysis: &AnalysisResult) -> String {
    let mut step_index = 0usize;
    let mut out = String::new();
    let lines = yaml.lines().collect::<Vec<_>>();
    let mut i = 0usize;

    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim_start();

        if is_uses_line(trimmed) {
            if let Some(step) = nth_non_empty_step(analysis, step_index) {
                out.push_str(line);
                out.push_str(" --- ");
                out.push_str(&format_step_kinds(step));
                out.push('\n');
                step_index += 1;
            } else {
                out.push_str(line);
                out.push('\n');
            }
            i += 1;
            continue;
        }

        if is_run_line(trimmed) {
            let Some(step) = nth_non_empty_step(analysis, step_index) else {
                out.push_str(line);
                out.push('\n');
                i += 1;
                continue;
            };
            step_index += 1;

            if is_block_run_line(trimmed) {
                out.push_str(line);
                out.push('\n');
                i += 1;

                let base_indent = leading_spaces(line);
                let mut command_index = 0usize;
                while i < lines.len() {
                    let body_line = lines[i];
                    let body_trimmed = body_line.trim();
                    let body_indent = leading_spaces(body_line);
                    if !body_trimmed.is_empty() && body_indent <= base_indent {
                        break;
                    }

                    if body_trimmed.is_empty() || command_index >= step.commands.len() {
                        out.push_str(body_line);
                    } else {
                        out.push_str(body_line);
                        out.push_str(" --- ");
                        out.push_str(format_command_kind(&step.commands[command_index]).as_str());
                        command_index += 1;
                    }
                    out.push('\n');
                    i += 1;
                }
                continue;
            }

            out.push_str(line);
            out.push_str(" --- ");
            out.push_str(&format_step_kinds(step));
            out.push('\n');
            i += 1;
            continue;
        }

        out.push_str(line);
        out.push('\n');
        i += 1;
    }

    out
}

fn is_uses_line(trimmed: &str) -> bool {
    trimmed.starts_with("- uses:") || trimmed.starts_with("uses:")
}

fn is_run_line(trimmed: &str) -> bool {
    trimmed.starts_with("- run:") || trimmed.starts_with("run:")
}

fn is_block_run_line(trimmed: &str) -> bool {
    trimmed.starts_with("- run: |")
        || trimmed.starts_with("- run: >")
        || trimmed.starts_with("run: |")
        || trimmed.starts_with("run: >")
}

fn leading_spaces(line: &str) -> usize {
    line.len() - line.trim_start_matches(' ').len()
}

fn nth_non_empty_step(analysis: &AnalysisResult, mut idx: usize) -> Option<&StepPlan> {
    for step in &analysis.steps {
        if step.commands.is_empty() {
            continue;
        }
        if idx == 0 {
            return Some(step);
        }
        idx -= 1;
    }
    None
}

fn format_step_annotation(step: &StepPlan) -> String {
    let left = step
        .commands
        .iter()
        .map(|cmd| {
            cmd.attr
                .tools
                .first()
                .cloned()
                .unwrap_or_else(|| "<unknown>".to_string())
        })
        .collect::<Vec<_>>()
        .join(" && ");
    let right = step
        .commands
        .iter()
        .map(|cmd| {
            cmd.attr
                .kind
                .clone()
                .unwrap_or(CmdKind::Other)
                .as_str()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join(" && ");
    format!("{left} --- {right}")
}

fn format_step_kinds(step: &StepPlan) -> String {
    step
        .commands
        .iter()
        .map(format_command_kind)
        .collect::<Vec<_>>()
        .join(" && ")
}

fn format_command_kind(command: &CommandPlan) -> String {
    let kind = command.attr.kind.clone().unwrap_or(CmdKind::Other);
    format_kind_label(&kind, command.attr.special_action.as_ref())
}

fn format_kind_label(kind: &CmdKind, special: Option<&SpecialActionKind>) -> String {
    match special {
        Some(special) => format!("{} ({})", kind.as_str(), special.as_str()),
        None => kind.as_str().to_string(),
    }
}

pub fn classify_step_kind(step: &ActionsAst) -> Option<CmdKind> {
    match step {
        ActionsAst::UsesStep { uses, .. } => Some(classify_uses_kind(uses)),
        _ => None,
    }
}

fn collect_steps(
    id: AstId,
    arena: &AstArena,
    out: &mut Vec<StepPlan>,
    unknown_uses: &mut Vec<String>,
) {
    match arena.get_actions(&id) {
        ActionsAst::Workflow { jobs, .. } => {
            for job in jobs {
                collect_steps(*job, arena, out, unknown_uses);
            }
        }
        ActionsAst::Job { steps, .. } => {
            for step_id in steps {
                out.push(analyze_step(*step_id, arena));
                if let ActionsAst::UsesStep { uses, .. } = arena.get_actions(step_id)
                    && normalize_uses(uses).is_none()
                {
                    unknown_uses.push(uses.clone());
                }
            }
        }
        _ => {}
    }
}

fn read_simple_command_words(cmd_id: AstId, arena: &AstArena) -> Option<Vec<String>> {
    let ShAstNode::SimpleCommand { argv, .. } = arena.get_sh(cmd_id) else {
        return None;
    };
    let words = argv
        .iter()
        .filter_map(|id| match arena.get_sh(*id) {
            ShAstNode::Word(w) => {
                let s = w.trim();
                if s.is_empty() {
                    None
                } else {
                    Some(s.to_string())
                }
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    Some(words)
}

fn classify_simple_command_from_words(words: &[String]) -> CmdKind {
    let cmd = words[0].as_str();
    match cmd {
        "apt" | "apt-get" | "brew" | "asdf" | "pip" | "pip3" => CmdKind::EnvSetup,
        "pytest" => CmdKind::Test,
        "npm" => classify_npm(words),
        "cargo" => classify_cargo(words),
        "gradle" => classify_gradle(words),
        "go" => classify_go(words),
        _ => CmdKind::Other,
    }
}

fn classify_npm(words: &[String]) -> CmdKind {
    match words.get(1).map(String::as_str) {
        Some("test") => CmdKind::Test,
        Some("run") if words.get(2).map(String::as_str) == Some("build") => CmdKind::TestSetup,
        Some("install") | Some("ci") => CmdKind::EnvSetup,
        _ => CmdKind::Other,
    }
}

fn classify_cargo(words: &[String]) -> CmdKind {
    match words.get(1).map(String::as_str) {
        Some("test") => CmdKind::Test,
        Some("build") => CmdKind::TestSetup,
        Some("install") => CmdKind::EnvSetup,
        _ => CmdKind::Other,
    }
}

fn classify_gradle(words: &[String]) -> CmdKind {
    match words.get(1).map(String::as_str) {
        Some("test") => CmdKind::Test,
        Some("assemble") => CmdKind::TestSetup,
        _ => CmdKind::Other,
    }
}

fn classify_go(words: &[String]) -> CmdKind {
    match words.get(1).map(String::as_str) {
        Some("test") => CmdKind::Test,
        _ => CmdKind::Other,
    }
}

fn classify_uses_kind(uses: &str) -> CmdKind {
    if let Some(special) = classify_uses_special_action(uses) {
        return match special {
            SpecialActionKind::Checkout => CmdKind::EnvSetup,
            SpecialActionKind::ArtifactUpload | SpecialActionKind::ArtifactDownload => CmdKind::Other,
        };
    }
    let Some(owner_repo) = normalize_uses(uses) else {
        return CmdKind::Other;
    };
    match owner_repo.as_str() {
        "actions/checkout"
        | "actions/setup-node"
        | "actions/setup-python"
        | "actions/setup-java"
        | "actions-rs/toolchain" => CmdKind::EnvSetup,
        "actions/upload-artifact" | "actions/download-artifact" => CmdKind::Other,
        _ => CmdKind::Other,
    }
}

fn classify_uses_special_action(uses: &str) -> Option<SpecialActionKind> {
    let owner_repo = normalize_uses(uses)?;
    match owner_repo.as_str() {
        "actions/checkout" => Some(SpecialActionKind::Checkout),
        "actions/upload-artifact" => Some(SpecialActionKind::ArtifactUpload),
        "actions/download-artifact" => Some(SpecialActionKind::ArtifactDownload),
        _ => None,
    }
}

fn collect_simple_commands(id: AstId, arena: &AstArena, out: &mut Vec<AstId>) {
    match arena.get_sh(id) {
        ShAstNode::List(items) => {
            for item in items {
                collect_simple_commands(item.body, arena, out);
            }
        }
        ShAstNode::AndOr { first, rest } => {
            collect_simple_commands(*first, arena, out);
            for item in rest {
                collect_simple_commands(item.body, arena, out);
            }
        }
        ShAstNode::Pipeline { first, rest } => {
            collect_simple_commands(*first, arena, out);
            for item in rest {
                collect_simple_commands(*item, arena, out);
            }
        }
        ShAstNode::SimpleCommand { .. } => out.push(id),
        ShAstNode::If {
            cond,
            then_part,
            else_part,
        } => {
            collect_simple_commands(*cond, arena, out);
            collect_simple_commands(*then_part, arena, out);
            if let Some(else_part) = else_part {
                collect_simple_commands(*else_part, arena, out);
            }
        }
        ShAstNode::While { cond, body } => {
            collect_simple_commands(*cond, arena, out);
            collect_simple_commands(*body, arena, out);
        }
        ShAstNode::For { var, items, body } => {
            collect_simple_commands(*var, arena, out);
            for item in items {
                collect_simple_commands(*item, arena, out);
            }
            collect_simple_commands(*body, arena, out);
        }
        ShAstNode::FunctionDef { name, body } => {
            collect_simple_commands(*name, arena, out);
            collect_simple_commands(*body, arena, out);
        }
        ShAstNode::Subshell { body } | ShAstNode::Group { body } => {
            collect_simple_commands(*body, arena, out);
        }
        ShAstNode::Word(_)
        | ShAstNode::Assignment(_)
        | ShAstNode::Redir { .. }
        | ShAstNode::Unknown => {}
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CmdKind, PlanOptions, analyze_actions, analyze_simple_command, build_execution_plan,
        format_cmd_kind_lines, annotate_yaml_with_cmd_kind,
    };
    use crate::actions_parser::actions_ast::{ActionsAst, RunsOn};
    use crate::actions_parser::arena::AstArena;
    use crate::actions_parser::sh_parser::sh_ast::{ListItem, SeparatorKind, ShAstNode};

    fn alloc_simple_command(arena: &mut AstArena, words: &[&str]) -> crate::actions_parser::arena::AstId {
        let argv = words
            .iter()
            .map(|w| arena.alloc_sh(ShAstNode::Word((*w).to_string())))
            .collect::<Vec<_>>();
        arena.alloc_sh(ShAstNode::SimpleCommand {
            assignments: vec![],
            argv,
            redirs: vec![],
        })
    }

    #[test]
    fn classify_simple_command_kinds() {
        let mut arena = AstArena::new();

        let cargo_test = alloc_simple_command(&mut arena, &["cargo", "test"]);
        let cargo_build = alloc_simple_command(&mut arena, &["cargo", "build"]);
        let npm_install = alloc_simple_command(&mut arena, &["npm", "install"]);
        let echo = alloc_simple_command(&mut arena, &["echo", "ok"]);

        assert_eq!(
            analyze_simple_command(cargo_test, &arena).kind,
            Some(CmdKind::Test)
        );
        assert_eq!(
            analyze_simple_command(cargo_build, &arena).kind,
            Some(CmdKind::TestSetup)
        );
        assert_eq!(
            analyze_simple_command(npm_install, &arena).kind,
            Some(CmdKind::EnvSetup)
        );
        assert_eq!(
            analyze_simple_command(echo, &arena).kind,
            Some(CmdKind::Other)
        );
    }

    #[test]
    fn uses_step_is_classified_and_unknown_collected() {
        let mut arena = AstArena::new();
        let run_cmd = alloc_simple_command(&mut arena, &["cargo", "test"]);
        let run_list = arena.alloc_sh(ShAstNode::List(vec![ListItem {
            body: run_cmd,
            sep: SeparatorKind::Seq,
        }]));

        let step_run = arena.alloc_actions(ActionsAst::RunStep {
            run: run_list,
            name: None,
            id: None,
            if_cond: None,
            env: None,
            shell: None,
            working_directory: None,
            timeout_minutes: None,
            continue_on_error: None,
        });
        let step_uses_known = arena.alloc_actions(ActionsAst::UsesStep {
            uses: "actions/checkout@v4".to_string(),
            name: None,
            id: None,
            if_cond: None,
            env: None,
            with: None,
            timeout_minutes: None,
            continue_on_error: None,
        });
        let step_uses_unknown = arena.alloc_actions(ActionsAst::UsesStep {
            uses: "./.github/actions/setup".to_string(),
            name: None,
            id: None,
            if_cond: None,
            env: None,
            with: None,
            timeout_minutes: None,
            continue_on_error: None,
        });
        let job = arena.alloc_actions(ActionsAst::Job {
            name: None,
            runs_on: RunsOn::String("ubuntu-latest".to_string()),
            steps: vec![step_run, step_uses_known, step_uses_unknown],
            needs: None,
            env: None,
            defaults: None,
            permissions: None,
            if_cond: None,
            strategy: None,
            container: None,
            services: None,
            timeout_minutes: None,
            continue_on_error: None,
        });
        let on = arena.alloc_actions(ActionsAst::OnString("push".to_string()));
        let root = arena.alloc_actions(ActionsAst::Workflow {
            name: None,
            run_name: None,
            jobs: vec![job],
            on,
            env: None,
            defaults: None,
            permissions: None,
            concurrency: None,
        });

        let analysis = analyze_actions(root, &arena);
        let uses_known = analysis.steps[1].commands[0].attr.kind.clone();
        let uses_unknown = analysis.steps[2].commands[0].attr.kind.clone();

        assert_eq!(uses_known, Some(CmdKind::EnvSetup));
        assert_eq!(uses_unknown, Some(CmdKind::Other));
        assert_eq!(analysis.unknown_uses, vec!["./.github/actions/setup".to_string()]);
    }

    #[test]
    fn build_execution_plan_filters_only_env_and_other() {
        let analysis = super::AnalysisResult {
            steps: vec![super::StepPlan {
                step_id: crate::actions_parser::arena::AstId(1),
                commands: vec![
                    super::CommandPlan {
                        ast_id: crate::actions_parser::arena::AstId(10),
                        attr: super::Attr {
                            kind: Some(CmdKind::EnvSetup),
                            ..super::Attr::default()
                        },
                    },
                    super::CommandPlan {
                        ast_id: crate::actions_parser::arena::AstId(11),
                        attr: super::Attr {
                            kind: Some(CmdKind::Test),
                            ..super::Attr::default()
                        },
                    },
                    super::CommandPlan {
                        ast_id: crate::actions_parser::arena::AstId(12),
                        attr: super::Attr {
                            kind: Some(CmdKind::Other),
                            ..super::Attr::default()
                        },
                    },
                ],
            }],
            unknown_uses: vec![],
            errors: vec![],
        };

        let plan = build_execution_plan(
            &analysis,
            &PlanOptions {
                include_env_setup: false,
                include_other: false,
            },
        );
        assert_eq!(plan.commands.len(), 1);
        assert_eq!(plan.commands[0].kind, CmdKind::Test);
    }

    #[test]
    fn format_lines_join_commands_and_kinds() {
        let analysis = super::AnalysisResult {
            steps: vec![super::StepPlan {
                step_id: crate::actions_parser::arena::AstId(1),
                commands: vec![
                    super::CommandPlan {
                        ast_id: crate::actions_parser::arena::AstId(10),
                        attr: super::Attr {
                            kind: Some(CmdKind::TestSetup),
                            tools: vec!["cargo build".to_string()],
                            ..super::Attr::default()
                        },
                    },
                    super::CommandPlan {
                        ast_id: crate::actions_parser::arena::AstId(11),
                        attr: super::Attr {
                            kind: Some(CmdKind::Test),
                            tools: vec!["cargo test".to_string()],
                            ..super::Attr::default()
                        },
                    },
                ],
            }],
            unknown_uses: vec![],
            errors: vec![],
        };

        let lines = format_cmd_kind_lines(&analysis);
        assert_eq!(
            lines,
            vec!["cargo build && cargo test --- TestSetup && Test".to_string()]
        );
    }

    #[test]
    fn annotate_yaml_keeps_unrelated_lines() {
        let analysis = super::AnalysisResult {
            steps: vec![
                super::StepPlan {
                    step_id: crate::actions_parser::arena::AstId(1),
                    commands: vec![super::CommandPlan {
                        ast_id: crate::actions_parser::arena::AstId(10),
                        attr: super::Attr {
                            kind: Some(CmdKind::EnvSetup),
                            special_action: Some(super::SpecialActionKind::Checkout),
                            tools: vec!["actions/checkout@v4".to_string()],
                            ..super::Attr::default()
                        },
                    }],
                },
                super::StepPlan {
                    step_id: crate::actions_parser::arena::AstId(2),
                    commands: vec![
                        super::CommandPlan {
                            ast_id: crate::actions_parser::arena::AstId(11),
                            attr: super::Attr {
                                kind: Some(CmdKind::TestSetup),
                                tools: vec!["cargo build".to_string()],
                                ..super::Attr::default()
                            },
                        },
                        super::CommandPlan {
                            ast_id: crate::actions_parser::arena::AstId(12),
                            attr: super::Attr {
                                kind: Some(CmdKind::Test),
                                tools: vec!["cargo test".to_string()],
                                ..super::Attr::default()
                            },
                        },
                    ],
                },
            ],
            unknown_uses: vec![],
            errors: vec![],
        };
        let yaml = r#"name: CI
jobs:
  test:
    steps:
      - uses: actions/checkout@v4
      - name: build and test
        run: cargo build && cargo test
"#;
        let annotated = annotate_yaml_with_cmd_kind(yaml, &analysis);

        assert!(annotated.contains("name: CI\n"));
        assert!(annotated.contains("jobs:\n"));
        assert!(annotated.contains(
            "- uses: actions/checkout@v4 --- EnvSetup (Checkout)\n"
        ));
        assert!(annotated.contains(
            "run: cargo build && cargo test --- TestSetup && Test\n"
        ));
    }

    #[test]
    fn annotate_yaml_prints_multiline_run_per_command() {
        let analysis = super::AnalysisResult {
            steps: vec![super::StepPlan {
                step_id: crate::actions_parser::arena::AstId(2),
                commands: vec![
                    super::CommandPlan {
                        ast_id: crate::actions_parser::arena::AstId(11),
                        attr: super::Attr {
                            kind: Some(CmdKind::TestSetup),
                            tools: vec!["cargo build".to_string()],
                            ..super::Attr::default()
                        },
                    },
                    super::CommandPlan {
                        ast_id: crate::actions_parser::arena::AstId(12),
                        attr: super::Attr {
                            kind: Some(CmdKind::Test),
                            tools: vec!["cargo test".to_string()],
                            ..super::Attr::default()
                        },
                    },
                ],
            }],
            unknown_uses: vec![],
            errors: vec![],
        };
        let yaml = r#"jobs:
  test:
    steps:
      - run: |
          cargo build
          cargo test
"#;
        let annotated = annotate_yaml_with_cmd_kind(yaml, &analysis);

        assert!(annotated.contains("      - run: |\n"));
        assert!(annotated.contains("          cargo build --- TestSetup\n"));
        assert!(annotated.contains("          cargo test --- Test\n"));
    }

    #[test]
    fn format_lines_include_special_action_kind() {
        let analysis = super::AnalysisResult {
            steps: vec![super::StepPlan {
                step_id: crate::actions_parser::arena::AstId(1),
                commands: vec![
                    super::CommandPlan {
                        ast_id: crate::actions_parser::arena::AstId(10),
                        attr: super::Attr {
                            kind: Some(CmdKind::EnvSetup),
                            special_action: Some(super::SpecialActionKind::Checkout),
                            tools: vec!["actions/checkout@v4".to_string()],
                            ..super::Attr::default()
                        },
                    },
                    super::CommandPlan {
                        ast_id: crate::actions_parser::arena::AstId(11),
                        attr: super::Attr {
                            kind: Some(CmdKind::Other),
                            special_action: Some(super::SpecialActionKind::ArtifactUpload),
                            tools: vec!["actions/upload-artifact@v4".to_string()],
                            ..super::Attr::default()
                        },
                    },
                ],
            }],
            unknown_uses: vec![],
            errors: vec![],
        };

        let lines = format_cmd_kind_lines(&analysis);
        assert_eq!(
            lines,
            vec![
                "actions/checkout@v4 && actions/upload-artifact@v4 --- EnvSetup (Checkout) && Other (ArtifactUpload)".to_string()
            ]
        );
    }
}
