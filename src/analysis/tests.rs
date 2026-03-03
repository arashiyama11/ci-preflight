use super::{
    CmdKind, PlanOptions, analyze_actions, analyze_simple_command, annotate_yaml_with_cmd_kind,
    build_execution_plan, format_cmd_kind_lines,
};
use crate::action_catalog::{ActionCatalog, ActionCatalogEntry};
use crate::actions_parser::actions_ast::{ActionsAst, RunsOn};
use crate::actions_parser::arena::AstArena;
use crate::actions_parser::sh_parser::sh_ast::{ListItem, SeparatorKind, ShAstNode};
use std::collections::BTreeMap;

fn alloc_simple_command(
    arena: &mut AstArena,
    words: &[&str],
) -> crate::actions_parser::arena::AstId {
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
    let bracket_test = alloc_simple_command(&mut arena, &["[", "-n", "$X", "]"]);
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
        analyze_simple_command(bracket_test, &arena).kind,
        Some(CmdKind::Assert)
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
    assert_eq!(
        analysis.unknown_uses,
        vec!["./.github/actions/setup".to_string()]
    );
}

#[test]
fn uses_step_shell_inputs_are_classified_as_commands() {
    let mut arena = AstArena::new();
    let mut with = BTreeMap::new();
    with.insert("command".to_string(), "echo hello\ncargo test".to_string());
    let step = arena.alloc_actions(ActionsAst::UsesStep {
        uses: "nick-fields/retry@v3".to_string(),
        name: None,
        id: None,
        if_cond: None,
        env: None,
        with: Some(with),
        timeout_minutes: None,
        continue_on_error: None,
    });

    let mut catalog: ActionCatalog = ActionCatalog::new();
    catalog.insert(
        "nick-fields/retry".to_string(),
        ActionCatalogEntry {
            required_tools: vec![],
            shell_inputs: vec!["command".to_string()],
            cmd_kind: Some("Other".to_string()),
            special_action: None,
            confidence: None,
            notes: None,
        },
    );

    let plan = super::analyze_step_with_catalog(step, &arena, Some(&catalog));
    assert_eq!(plan.commands.len(), 3);
    assert_eq!(plan.commands[0].attr.kind, Some(CmdKind::Other));
    assert_eq!(plan.commands[1].attr.kind, Some(CmdKind::Other));
    assert_eq!(plan.commands[2].attr.kind, Some(CmdKind::Test));
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
    assert!(annotated.contains("- uses: actions/checkout@v4 --- EnvSetup (Checkout)\n"));
    assert!(annotated.contains("run: cargo build && cargo test --- TestSetup && Test\n"));
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
fn annotate_yaml_prints_uses_shell_input_on_script_lines() {
    let analysis = super::AnalysisResult {
        steps: vec![super::StepPlan {
            step_id: crate::actions_parser::arena::AstId(1),
            commands: vec![
                super::CommandPlan {
                    ast_id: crate::actions_parser::arena::AstId(10),
                    attr: super::Attr {
                        kind: Some(CmdKind::Other),
                        tools: vec!["reactivecircus/android-emulator-runner@v2".to_string()],
                        ..super::Attr::default()
                    },
                },
                super::CommandPlan {
                    ast_id: crate::actions_parser::arena::AstId(11),
                    attr: super::Attr {
                        kind: Some(CmdKind::Other),
                        tools: vec!["adb install -r app.apk".to_string()],
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
      - uses: reactivecircus/android-emulator-runner@v2
        with:
          script: |
            adb install -r app.apk
            cargo test
    "#;
    let annotated = annotate_yaml_with_cmd_kind(yaml, &analysis);

    assert!(annotated.contains("- uses: reactivecircus/android-emulator-runner@v2 --- Other\n"));
    assert!(annotated.contains("            adb install -r app.apk --- Other\n"));
    assert!(annotated.contains("            cargo test --- Test\n"));
}

#[test]
fn annotate_yaml_skips_comment_lines_in_multiline_run() {
    let analysis = super::AnalysisResult {
        steps: vec![super::StepPlan {
            step_id: crate::actions_parser::arena::AstId(2),
            commands: vec![super::CommandPlan {
                ast_id: crate::actions_parser::arena::AstId(12),
                attr: super::Attr {
                    kind: Some(CmdKind::Test),
                    tools: vec!["cargo test".to_string()],
                    ..super::Attr::default()
                },
            }],
        }],
        unknown_uses: vec![],
        errors: vec![],
    };
    let yaml = r#"jobs:
      test:
    steps:
      - name: Run tests
        run: |
          # 単体テスト
          cargo test
    "#;

    let annotated = annotate_yaml_with_cmd_kind(yaml, &analysis);
    assert!(annotated.contains("          # 単体テスト\n"));
    assert!(annotated.contains("          cargo test --- Test\n"));
}

#[test]
fn annotate_yaml_skips_control_and_assignment_lines() {
    let analysis = super::AnalysisResult {
        steps: vec![super::StepPlan {
            step_id: crate::actions_parser::arena::AstId(2),
            commands: vec![
                super::CommandPlan {
                    ast_id: crate::actions_parser::arena::AstId(11),
                    attr: super::Attr {
                        kind: Some(CmdKind::Other),
                        tools: vec!["[".to_string()],
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
          failed=false
          if [ "$failed" = true ]; then
            cargo test
          fi
    "#;
    let annotated = annotate_yaml_with_cmd_kind(yaml, &analysis);
    assert!(annotated.contains("          failed=false\n"));
    assert!(annotated.contains("          if [ \"$failed\" = true ]; then --- Other\n"));
    assert!(annotated.contains("            cargo test --- Test\n"));
    assert!(annotated.contains("          fi\n"));
}

#[test]
fn annotate_yaml_multiline_keeps_other_for_simple_commands() {
    let analysis = super::AnalysisResult {
        steps: vec![super::StepPlan {
            step_id: crate::actions_parser::arena::AstId(2),
            commands: vec![
                super::CommandPlan {
                    ast_id: crate::actions_parser::arena::AstId(11),
                    attr: super::Attr {
                        kind: Some(CmdKind::Other),
                        tools: vec!["set -x".to_string()],
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
          set -x
          cargo test
    "#;
    let annotated = annotate_yaml_with_cmd_kind(yaml, &analysis);
    assert!(annotated.contains("          set -x --- Other\n"));
    assert!(annotated.contains("          cargo test --- Test\n"));
}

#[test]
fn annotate_yaml_multiline_line_continuation_is_single_command() {
    let analysis = super::AnalysisResult {
        steps: vec![super::StepPlan {
            step_id: crate::actions_parser::arena::AstId(2),
            commands: vec![super::CommandPlan {
                ast_id: crate::actions_parser::arena::AstId(11),
                attr: super::Attr {
                    kind: Some(CmdKind::Other),
                    tools: vec!["sudo apt-get install bison zsh".to_string()],
                    ..super::Attr::default()
                },
            }],
        }],
        unknown_uses: vec![],
        errors: vec![],
    };
    let yaml = r#"jobs:
      test:
    steps:
      - run: |
          sudo apt-get install \
            bison \
            zsh
    "#;
    let annotated = annotate_yaml_with_cmd_kind(yaml, &analysis);
    assert!(annotated.contains("          sudo apt-get install \\ --- Other\n"));
    assert!(annotated.contains("            bison \\\n"));
    assert!(annotated.contains("            zsh\n"));
    assert!(!annotated.contains("            bison \\ --- "));
    assert!(!annotated.contains("            zsh --- "));
}

#[test]
fn annotate_yaml_multiline_assignment_substitution_gets_annotation() {
    let analysis = super::AnalysisResult {
        steps: vec![super::StepPlan {
            step_id: crate::actions_parser::arena::AstId(2),
            commands: vec![super::CommandPlan {
                ast_id: crate::actions_parser::arena::AstId(11),
                attr: super::Attr {
                    kind: Some(CmdKind::Other),
                    tools: vec!["curl -s".to_string()],
                    ..super::Attr::default()
                },
            }],
        }],
        unknown_uses: vec![],
        errors: vec![],
    };
    let yaml = r#"jobs:
      test:
    steps:
      - run: |
          LATEST_RELEASE=$(curl -s -H "Authorization: token $GITHUB_TOKEN" \
            "https://api.github.com/repos/${{ github.repository }}/releases/latest" \
            | jq -r '.tag_name')
    "#;
    let annotated = annotate_yaml_with_cmd_kind(yaml, &analysis);
    assert!(annotated.contains("          LATEST_RELEASE=$(curl -s -H "));
    assert!(annotated.contains("--- Other\n"));
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
