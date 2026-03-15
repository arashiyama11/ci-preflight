use std::collections::BTreeMap;

use thiserror::Error;
use yaml_rust2::{Yaml, YamlLoader};

const SIMPLE_COMMANDS_YAML: &str = include_str!("../../data/cmd_kind_rules/simple_commands.yaml");
const SUBCOMMANDS_YAML: &str = include_str!("../../data/cmd_kind_rules/subcommands.yaml");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuleCmdKind {
    EnvSetup,
    TestSetup,
    Test,
    Assert,
    Other,
}

#[derive(Clone, Debug, Default)]
struct CmdKindRules {
    simple_by_cmd: BTreeMap<String, RuleCmdKind>,
    subcommands: BTreeMap<String, Vec<SubcommandRule>>,
}

#[derive(Clone, Debug)]
struct SubcommandRule {
    matcher: SubcommandMatcher,
    kind: RuleCmdKind,
    ignore_options: bool,
}

#[derive(Clone, Debug)]
enum SubcommandMatcher {
    Exact(Vec<String>),
    Prefix { arg_index: usize, pattern: String },
}

#[derive(Debug, Error)]
pub enum CmdKindRulesError {
    #[error("failed to parse cmd kind rules YAML: {0}")]
    YamlScan(#[from] yaml_rust2::ScanError),
    #[error("simple_commands YAML must contain exactly one document")]
    InvalidSimpleDocCount,
    #[error("subcommands YAML must contain exactly one document")]
    InvalidSubcommandDocCount,
    #[error("{0} YAML root must be a sequence")]
    RootNotSeq(&'static str),
    #[error("{0} entry must be a mapping")]
    EntryNotMap(&'static str),
    #[error("{0} entry missing required field `{1}`")]
    MissingField(&'static str, &'static str),
    #[error("{0} entry field `{1}` must be a string")]
    InvalidStringField(&'static str, &'static str),
    #[error("subcommands entry field `args` must be a sequence of strings")]
    InvalidArgs,
    #[error("subcommands entry field `match` must be `exact` or `prefix`")]
    InvalidMatchType,
    #[error("subcommands entry field `ignore_options` must be a boolean")]
    InvalidIgnoreOptions,
    #[error("subcommands `prefix` match requires string field `pattern`")]
    MissingPrefixPattern,
    #[error("subcommands `prefix` match field `arg_index` must be non-negative integer")]
    InvalidArgIndex,
    #[error("invalid cmd kind `{0}`")]
    InvalidCmdKind(String),
}

pub fn classify_simple_command(words: &[String]) -> Result<RuleCmdKind, CmdKindRulesError> {
    let words = strip_command_wrappers(words);
    if words.is_empty() {
        return Ok(RuleCmdKind::Other);
    }

    let rules = load_rules()?;
    if let Some(sub_rules) = rules.subcommands.get(words[0].as_str()) {
        let tail = &words[1..];
        if let Some(kind) = classify_subcommand(tail, sub_rules) {
            return Ok(kind);
        }
    }

    Ok(rules
        .simple_by_cmd
        .get(words[0].as_str())
        .copied()
        .unwrap_or(RuleCmdKind::Other))
}

fn strip_command_wrappers(words: &[String]) -> &[String] {
    let mut out = words;
    while let Some(head) = out.first().map(|s| s.as_str()) {
        match head {
            // Treat sudo as a transparent wrapper for command intent classification.
            "sudo" => {
                out = &out[1..];
                while let Some(arg) = out.first() {
                    if arg == "--" {
                        out = &out[1..];
                        break;
                    }
                    if arg.starts_with('-') && arg != "-" {
                        out = &out[1..];
                        continue;
                    }
                    break;
                }
            }
            // `env KEY=VAL cmd` / `env -i KEY=VAL cmd`
            "env" => {
                out = &out[1..];
                while let Some(arg) = out.first() {
                    if arg == "--" {
                        out = &out[1..];
                        break;
                    }
                    if arg.starts_with('-') && arg != "-" {
                        out = &out[1..];
                        continue;
                    }
                    if arg.contains('=') && !arg.starts_with('=') {
                        out = &out[1..];
                        continue;
                    }
                    break;
                }
            }
            // `command cargo test`, `command -v cargo`
            "command" => {
                out = &out[1..];
                while let Some(arg) = out.first() {
                    if arg.starts_with('-') && arg != "-" {
                        out = &out[1..];
                        continue;
                    }
                    break;
                }
            }
            // `time cargo test`, `time -p cargo test`
            "time" => {
                out = &out[1..];
                while let Some(arg) = out.first() {
                    if arg.starts_with('-') && arg != "-" {
                        out = &out[1..];
                        continue;
                    }
                    break;
                }
            }
            _ => break,
        }
    }
    out
}

fn classify_subcommand(tail: &[String], sub_rules: &[SubcommandRule]) -> Option<RuleCmdKind> {
    for rule in sub_rules {
        let tail = if rule.ignore_options {
            strip_leading_options(tail)
        } else {
            tail
        };
        match &rule.matcher {
            SubcommandMatcher::Exact(args) => {
                if tail.len() < args.len() {
                    continue;
                }
                if tail
                    .iter()
                    .take(args.len())
                    .zip(args.iter())
                    .all(|(lhs, rhs)| lhs == rhs)
                {
                    return Some(rule.kind);
                }
            }
            SubcommandMatcher::Prefix { arg_index, pattern } => {
                if let Some(arg) = tail.get(*arg_index)
                    && arg.starts_with(pattern)
                {
                    return Some(rule.kind);
                }
            }
        }
    }
    None
}

fn strip_leading_options(args: &[String]) -> &[String] {
    let mut i = 0usize;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--" {
            i += 1;
            break;
        }
        if arg.starts_with('-') && arg != "-" {
            i += 1;
            continue;
        }
        break;
    }
    &args[i..]
}

fn load_rules() -> Result<CmdKindRules, CmdKindRulesError> {
    let simple = parse_simple_commands(SIMPLE_COMMANDS_YAML)?;
    let sub = parse_subcommands(SUBCOMMANDS_YAML)?;
    Ok(CmdKindRules {
        simple_by_cmd: simple,
        subcommands: sub,
    })
}

fn parse_simple_commands(raw: &str) -> Result<BTreeMap<String, RuleCmdKind>, CmdKindRulesError> {
    let docs = YamlLoader::load_from_str(raw)?;
    if docs.len() != 1 {
        return Err(CmdKindRulesError::InvalidSimpleDocCount);
    }
    let root = docs
        .first()
        .ok_or(CmdKindRulesError::InvalidSimpleDocCount)?;
    let entries = root
        .as_vec()
        .ok_or(CmdKindRulesError::RootNotSeq("simple_commands"))?;

    let mut out = BTreeMap::new();
    for entry in entries {
        let map = entry
            .as_hash()
            .ok_or(CmdKindRulesError::EntryNotMap("simple_commands"))?;
        let cmd = get_required_string(map, "simple_commands", "cmd")?;
        let kind = parse_kind(get_required_string(map, "simple_commands", "kind")?)?;
        out.insert(cmd.to_string(), kind);
    }
    Ok(out)
}

fn parse_subcommands(
    raw: &str,
) -> Result<BTreeMap<String, Vec<SubcommandRule>>, CmdKindRulesError> {
    let docs = YamlLoader::load_from_str(raw)?;
    if docs.len() != 1 {
        return Err(CmdKindRulesError::InvalidSubcommandDocCount);
    }
    let root = docs
        .first()
        .ok_or(CmdKindRulesError::InvalidSubcommandDocCount)?;
    let entries = root
        .as_vec()
        .ok_or(CmdKindRulesError::RootNotSeq("subcommands"))?;

    let mut out: BTreeMap<String, Vec<SubcommandRule>> = BTreeMap::new();
    for entry in entries {
        let map = entry
            .as_hash()
            .ok_or(CmdKindRulesError::EntryNotMap("subcommands"))?;
        let cmd = get_required_string(map, "subcommands", "cmd")?;
        let matcher = parse_subcommand_matcher(map)?;
        let kind = parse_kind(get_required_string(map, "subcommands", "kind")?)?;
        let ignore_options = parse_ignore_options(map)?;

        out.entry(cmd.to_string())
            .or_default()
            .push(SubcommandRule {
                matcher,
                kind,
                ignore_options,
            });
    }

    for rules in out.values_mut() {
        rules.sort_by_key(|r| std::cmp::Reverse(rule_priority(&r.matcher)));
    }

    Ok(out)
}

fn parse_subcommand_matcher(
    map: &yaml_rust2::yaml::Hash,
) -> Result<SubcommandMatcher, CmdKindRulesError> {
    let match_type = map
        .get(&Yaml::String("match".to_string()))
        .and_then(Yaml::as_str)
        .unwrap_or("exact");

    match match_type {
        "exact" => {
            let args_node = map
                .get(&Yaml::String("args".to_string()))
                .ok_or(CmdKindRulesError::MissingField("subcommands", "args"))?;
            let args = args_node
                .as_vec()
                .ok_or(CmdKindRulesError::InvalidArgs)?
                .iter()
                .map(|v| {
                    v.as_str()
                        .map(ToString::to_string)
                        .ok_or(CmdKindRulesError::InvalidArgs)
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(SubcommandMatcher::Exact(args))
        }
        "prefix" => {
            let pattern = map
                .get(&Yaml::String("pattern".to_string()))
                .and_then(Yaml::as_str)
                .ok_or(CmdKindRulesError::MissingPrefixPattern)?
                .to_string();
            let arg_index = map
                .get(&Yaml::String("arg_index".to_string()))
                .and_then(Yaml::as_i64)
                .unwrap_or(0);
            if arg_index < 0 {
                return Err(CmdKindRulesError::InvalidArgIndex);
            }
            Ok(SubcommandMatcher::Prefix {
                arg_index: arg_index as usize,
                pattern,
            })
        }
        _ => Err(CmdKindRulesError::InvalidMatchType),
    }
}

fn parse_ignore_options(map: &yaml_rust2::yaml::Hash) -> Result<bool, CmdKindRulesError> {
    match map
        .get(&Yaml::String("ignore_options".to_string()))
        .map(Yaml::as_bool)
    {
        None => Ok(false),
        Some(Some(v)) => Ok(v),
        Some(None) => Err(CmdKindRulesError::InvalidIgnoreOptions),
    }
}

fn rule_priority(matcher: &SubcommandMatcher) -> usize {
    match matcher {
        SubcommandMatcher::Exact(args) => args.len() + 100,
        SubcommandMatcher::Prefix { .. } => 1,
    }
}

fn get_required_string<'a>(
    map: &'a yaml_rust2::yaml::Hash,
    section: &'static str,
    field: &'static str,
) -> Result<&'a str, CmdKindRulesError> {
    let node = map
        .get(&Yaml::String(field.to_string()))
        .ok_or(CmdKindRulesError::MissingField(section, field))?;
    node.as_str()
        .ok_or(CmdKindRulesError::InvalidStringField(section, field))
}

fn parse_kind(raw: &str) -> Result<RuleCmdKind, CmdKindRulesError> {
    match raw {
        "EnvSetup" => Ok(RuleCmdKind::EnvSetup),
        "TestSetup" => Ok(RuleCmdKind::TestSetup),
        "Test" => Ok(RuleCmdKind::Test),
        "Assert" => Ok(RuleCmdKind::Assert),
        "Other" => Ok(RuleCmdKind::Other),
        _ => Err(CmdKindRulesError::InvalidCmdKind(raw.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::{RuleCmdKind, classify_simple_command};

    #[test]
    fn classify_simple_and_subcommands() {
        assert_eq!(
            classify_simple_command(&["apt-get".to_string(), "install".to_string()]).unwrap(),
            RuleCmdKind::EnvSetup
        );
        assert_eq!(
            classify_simple_command(&["npm".to_string(), "run".to_string(), "build".to_string(),])
                .unwrap(),
            RuleCmdKind::TestSetup
        );
        assert_eq!(
            classify_simple_command(&["cargo".to_string(), "test".to_string()]).unwrap(),
            RuleCmdKind::Test
        );
        assert_eq!(
            classify_simple_command(&["gradle".to_string(), "testDebugUnitTest".to_string(),])
                .unwrap(),
            RuleCmdKind::Test
        );
        assert_eq!(
            classify_simple_command(&[
                "[".to_string(),
                "-n".to_string(),
                "$X".to_string(),
                "]".to_string()
            ])
            .unwrap(),
            RuleCmdKind::Assert
        );
        assert_eq!(
            classify_simple_command(&[
                "sudo".to_string(),
                "apt-get".to_string(),
                "install".to_string(),
            ])
            .unwrap(),
            RuleCmdKind::EnvSetup
        );
        assert_eq!(
            classify_simple_command(&[
                "sudo".to_string(),
                "-E".to_string(),
                "apt-get".to_string(),
                "install".to_string(),
            ])
            .unwrap(),
            RuleCmdKind::EnvSetup
        );
        assert_eq!(
            classify_simple_command(&[
                "env".to_string(),
                "RUSTFLAGS=-Dwarnings".to_string(),
                "cargo".to_string(),
                "test".to_string(),
            ])
            .unwrap(),
            RuleCmdKind::Test
        );
        assert_eq!(
            classify_simple_command(&[
                "command".to_string(),
                "cargo".to_string(),
                "test".to_string(),
            ])
            .unwrap(),
            RuleCmdKind::Test
        );
        assert_eq!(
            classify_simple_command(&[
                "time".to_string(),
                "-p".to_string(),
                "cargo".to_string(),
                "test".to_string(),
            ])
            .unwrap(),
            RuleCmdKind::Test
        );
        assert_eq!(
            classify_simple_command(&[
                "npm".to_string(),
                "--silent".to_string(),
                "test".to_string()
            ])
            .unwrap(),
            RuleCmdKind::Test
        );
        assert_eq!(
            classify_simple_command(&[
                "cargo".to_string(),
                "--locked".to_string(),
                "test".to_string()
            ])
            .unwrap(),
            RuleCmdKind::Test
        );
    }
}
