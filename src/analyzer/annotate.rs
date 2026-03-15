use super::{
    AnalysisResult, CmdKind, StepPlan, classify_simple_command_from_words, format_command_kind,
    format_step_kinds,
};
use crate::analyzer::action_catalog::{load_action_catalog, shell_input_keys_for_uses};

pub fn annotate_yaml_with_cmd_kind(yaml: &str, analysis: &AnalysisResult) -> String {
    let mut step_index = 0usize;
    let mut out = String::new();
    let lines = yaml.lines().collect::<Vec<_>>();
    let catalog = load_action_catalog().ok();
    let mut i = 0usize;

    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim_start();

        if is_uses_line(trimmed) {
            let Some(step) = nth_non_empty_step(analysis, step_index) else {
                out.push_str(line);
                out.push('\n');
                i += 1;
                continue;
            };
            step_index += 1;

            let uses_value = extract_uses_value(trimmed);
            let shell_input_keys = uses_value
                .as_deref()
                .and_then(|uses| {
                    catalog
                        .as_ref()
                        .and_then(|c| shell_input_keys_for_uses(uses, c))
                })
                .map(|keys| keys.iter().map(|k| k.as_str()).collect::<Vec<_>>())
                .unwrap_or_default();

            if shell_input_keys.is_empty() || step.commands.len() <= 1 {
                out.push_str(line);
                out.push_str(" --- ");
                out.push_str(&format_step_kinds(step));
                out.push('\n');
                i += 1;
                continue;
            }

            out.push_str(line);
            out.push_str(" --- ");
            out.push_str(&format_command_kind(&step.commands[0]));
            out.push('\n');
            i += 1;

            let step_base_indent = leading_spaces(line);
            let mut command_index = 1usize;
            while i < lines.len() {
                let body_line = lines[i];
                let body_trimmed = body_line.trim();
                let body_indent = leading_spaces(body_line);
                if !body_trimmed.is_empty()
                    && body_indent <= step_base_indent
                    && body_trimmed.starts_with('-')
                {
                    break;
                }

                if let Some((key, value)) = parse_mapping_key_value(body_trimmed) {
                    if shell_input_keys.iter().any(|k| *k == key) {
                        if value == "|" || value == ">" {
                            out.push_str(body_line);
                            out.push('\n');
                            i += 1;

                            let key_indent = body_indent;
                            let mut in_continuation = false;
                            while i < lines.len() {
                                let script_line = lines[i];
                                let script_trimmed = script_line.trim();
                                let script_indent = leading_spaces(script_line);
                                if !script_trimmed.is_empty() && script_indent <= key_indent {
                                    break;
                                }

                                if script_trimmed.is_empty()
                                    || is_shell_comment_line(script_trimmed)
                                {
                                    out.push_str(script_line);
                                } else {
                                    out.push_str(script_line);
                                    if !in_continuation {
                                        let line_cmd_count =
                                            estimate_simple_command_count_in_line(script_trimmed);
                                        if line_cmd_count > 0 {
                                            let mut labels = Vec::new();
                                            if command_index < step.commands.len() {
                                                let end = (command_index + line_cmd_count)
                                                    .min(step.commands.len());
                                                labels.extend(
                                                    step.commands[command_index..end]
                                                        .iter()
                                                        .map(format_command_kind),
                                                );
                                                command_index = end;
                                            }
                                            if labels.len() < line_cmd_count {
                                                let fallback =
                                                    classify_line_kinds_by_words(script_trimmed);
                                                labels.extend(
                                                    fallback
                                                        .into_iter()
                                                        .take(
                                                            line_cmd_count
                                                                .saturating_sub(labels.len()),
                                                        )
                                                        .map(|k| k.as_str().to_string()),
                                                );
                                            }
                                            if labels.is_empty() {
                                                labels.push(CmdKind::Other.as_str().to_string());
                                            }
                                            out.push_str(" --- ");
                                            out.push_str(&labels.join(" && "));
                                        }
                                    }
                                }
                                in_continuation = has_trailing_unescaped_backslash(script_trimmed);
                                out.push('\n');
                                i += 1;
                            }
                            continue;
                        }

                        out.push_str(body_line);
                        let line_cmd_count = estimate_simple_command_count_in_line(value);
                        if line_cmd_count > 0 {
                            let mut labels = Vec::new();
                            if command_index < step.commands.len() {
                                let end = (command_index + line_cmd_count).min(step.commands.len());
                                labels.extend(
                                    step.commands[command_index..end]
                                        .iter()
                                        .map(format_command_kind),
                                );
                                command_index = end;
                            }
                            if labels.len() < line_cmd_count {
                                let fallback = classify_line_kinds_by_words(value);
                                labels.extend(
                                    fallback
                                        .into_iter()
                                        .take(line_cmd_count.saturating_sub(labels.len()))
                                        .map(|k| k.as_str().to_string()),
                                );
                            }
                            if labels.is_empty() {
                                labels.push(CmdKind::Other.as_str().to_string());
                            }
                            out.push_str(" --- ");
                            out.push_str(&labels.join(" && "));
                        }
                        out.push('\n');
                        i += 1;
                        continue;
                    }
                }

                out.push_str(body_line);
                out.push('\n');
                i += 1;
            }
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
                let mut in_continuation = false;
                while i < lines.len() {
                    let body_line = lines[i];
                    let body_trimmed = body_line.trim();
                    let body_indent = leading_spaces(body_line);
                    if !body_trimmed.is_empty() && body_indent <= base_indent {
                        break;
                    }

                    if body_trimmed.is_empty() || is_shell_comment_line(body_trimmed) {
                        out.push_str(body_line);
                    } else {
                        out.push_str(body_line);
                        if !in_continuation {
                            let line_cmd_count =
                                estimate_simple_command_count_in_line(body_trimmed);
                            let mut labels = Vec::new();
                            if line_cmd_count > 0 {
                                if command_index < step.commands.len() {
                                    let end =
                                        (command_index + line_cmd_count).min(step.commands.len());
                                    labels.extend(
                                        step.commands[command_index..end]
                                            .iter()
                                            .map(format_command_kind),
                                    );
                                    command_index = end;
                                }
                                if labels.len() < line_cmd_count {
                                    let fallback = classify_line_kinds_by_words(body_trimmed);
                                    labels.extend(
                                        fallback
                                            .into_iter()
                                            .take(line_cmd_count.saturating_sub(labels.len()))
                                            .map(|k| k.as_str().to_string()),
                                    );
                                }
                                if labels.is_empty() {
                                    labels.push(CmdKind::Other.as_str().to_string());
                                }
                                out.push_str(" --- ");
                                out.push_str(&labels.join(" && "));
                            }
                        }
                    }
                    in_continuation = has_trailing_unescaped_backslash(body_trimmed);
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

fn extract_uses_value(trimmed: &str) -> Option<String> {
    let rest = if let Some(rest) = trimmed.strip_prefix("- uses:") {
        rest
    } else {
        trimmed.strip_prefix("uses:")?
    };
    let value = rest.split('#').next().unwrap_or(rest).trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn parse_mapping_key_value(trimmed: &str) -> Option<(&str, &str)> {
    let (key, value) = trimmed.split_once(':')?;
    let key = key.trim();
    if key.is_empty() {
        return None;
    }
    Some((key, value.trim()))
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

fn is_shell_comment_line(trimmed: &str) -> bool {
    trimmed.starts_with('#')
}

fn estimate_simple_command_count_in_line(line: &str) -> usize {
    classify_line_kinds_by_words(line).len()
}

fn classify_line_kinds_by_words(line: &str) -> Vec<CmdKind> {
    let segments = split_shell_segments(line);
    let mut out = Vec::new();
    for segment in segments {
        let trimmed = segment.trim();
        if trimmed.is_empty() || is_shell_comment_line(trimmed) {
            continue;
        }
        let tokens = trimmed.split_whitespace().collect::<Vec<_>>();
        if tokens.is_empty() {
            continue;
        }
        let first = tokens[0];
        if matches!(first, "if" | "while" | "until") {
            if let Some(second) = tokens.get(1).copied()
                && !is_shell_control_keyword(second)
                && !is_simple_assignment_token(second)
            {
                out.push(classify_simple_command_from_words(&[second.to_string()]));
            }
            continue;
        }
        if is_simple_assignment_token(first) {
            if let Some(cmd) = command_in_assignment_substitution(first) {
                out.push(classify_simple_command_from_words(&[cmd]));
            }
            continue;
        }
        if is_shell_control_keyword(first) || is_simple_assignment_token(first) {
            continue;
        }
        let words = tokens.iter().map(|t| (*t).to_string()).collect::<Vec<_>>();
        out.push(classify_simple_command_from_words(&words));
    }
    out
}

fn split_shell_segments(line: &str) -> Vec<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    let mut segments = Vec::new();
    let mut current = String::new();
    let chars = trimmed.chars().collect::<Vec<_>>();
    let mut i = 0usize;
    let mut single_quote = false;
    let mut double_quote = false;
    let mut backtick_quote = false;

    while i < chars.len() {
        let c = chars[i];
        if c == '\'' && !double_quote && !backtick_quote {
            single_quote = !single_quote;
            current.push(c);
            i += 1;
            continue;
        }
        if c == '"' && !single_quote && !backtick_quote {
            double_quote = !double_quote;
            current.push(c);
            i += 1;
            continue;
        }
        if c == '`' && !single_quote && !double_quote {
            backtick_quote = !backtick_quote;
            current.push(c);
            i += 1;
            continue;
        }
        if c == '\\' && i + 1 < chars.len() {
            current.push(c);
            current.push(chars[i + 1]);
            i += 2;
            continue;
        }
        if !single_quote && !double_quote && !backtick_quote {
            if c == '|' && chars.get(i + 1) == Some(&'|') {
                segments.push(current.trim().to_string());
                current.clear();
                i += 2;
                continue;
            }
            if c == '&' && chars.get(i + 1) == Some(&'&') {
                segments.push(current.trim().to_string());
                current.clear();
                i += 2;
                continue;
            }
            if c == ';' || c == '|' {
                segments.push(current.trim().to_string());
                current.clear();
                i += 1;
                continue;
            }
        }
        current.push(c);
        i += 1;
    }
    if !current.trim().is_empty() {
        segments.push(current.trim().to_string());
    }
    segments
}

fn is_shell_control_keyword(token: &str) -> bool {
    matches!(
        token,
        "if" | "then" | "else" | "elif" | "fi" | "for" | "while" | "until" | "do" | "done"
    )
}

fn is_simple_assignment_token(token: &str) -> bool {
    let Some((name, _value)) = token.split_once('=') else {
        return false;
    };
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn command_in_assignment_substitution(token: &str) -> Option<String> {
    let (_name, value) = token.split_once('=')?;
    let body = value.strip_prefix("$(")?;
    let cmd = body
        .chars()
        .take_while(|c| !c.is_whitespace() && *c != ')' && *c != '"' && *c != '\'')
        .collect::<String>();
    if cmd.is_empty() { None } else { Some(cmd) }
}

fn has_trailing_unescaped_backslash(line: &str) -> bool {
    let trimmed = line.trim_end();
    if !trimmed.ends_with('\\') {
        return false;
    }
    let count = trimmed.chars().rev().take_while(|c| *c == '\\').count();
    count % 2 == 1
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
