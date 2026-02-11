use crate::actions_parser::actions_ast::{
    ActionsAst, Concurrency, Container, ContainerCredentials, ContainerSpec, Defaults, Permissions,
    RunsOn, ScalarValue, Strategy, StringOrArray,
};
use crate::actions_parser::arena::{AstArena, AstId};
use crate::actions_parser::source_map::{SourceId, SourceMap};
use std::collections::BTreeMap;
use std::fmt::Write;
use thiserror::Error;
use yaml_rust2::{ScanError, Yaml, YamlEmitter, YamlLoader};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParseLocation {
    pub source_id: SourceId,
    pub span: Option<(usize, usize)>,
}

#[derive(Error, Debug)]
pub enum ActionsParseError {
    #[error("InternalError $0")]
    ScanError(ScanError),
    #[error("Invalid Actions yaml: {message}")]
    InvalidActions {
        message: String,
        location: Option<ParseLocation>,
    },
}

fn sh_error_location(
    err: &crate::actions_parser::sh_parser::ShParseError,
) -> Option<ParseLocation> {
    match err {
        crate::actions_parser::sh_parser::ShParseError::Lexer(
            crate::actions_parser::sh_parser::LexerError::UnexpectedEof(span),
        ) => Some(ParseLocation {
            source_id: span.source_id,
            span: Some((span.index, span.len)),
        }),
        crate::actions_parser::sh_parser::ShParseError::Lexer(
            crate::actions_parser::sh_parser::LexerError::Unknown,
        ) => None,
        crate::actions_parser::sh_parser::ShParseError::Parser(
            crate::actions_parser::sh_parser::ParseError::UnexpectedToken(tok),
        ) => Some(ParseLocation {
            source_id: tok.span.source_id,
            span: Some((tok.span.index, tok.span.len)),
        }),
        crate::actions_parser::sh_parser::ShParseError::Parser(
            crate::actions_parser::sh_parser::ParseError::UnexpectedEof,
        ) => None,
        crate::actions_parser::sh_parser::ShParseError::Parser(
            crate::actions_parser::sh_parser::ParseError::InternalError(_),
        ) => None,
    }
}

struct ActionsParser {
    arena: AstArena,
    errors: Vec<ActionsParseError>,
    current_source_id: Option<SourceId>,
}

impl ActionsParser {
    fn new() -> ActionsParser {
        ActionsParser {
            arena: AstArena::new(),
            errors: vec![],
            current_source_id: None,
        }
    }
    fn record_error(&mut self, msg: impl Into<String>) {
        let message = msg.into();
        if let Some(source_id) = self.current_source_id {
            self.errors.push(ActionsParseError::InvalidActions {
                message,
                location: Some(ParseLocation {
                    source_id,
                    span: None,
                }),
            });
        } else {
            self.errors.push(ActionsParseError::InvalidActions {
                message,
                location: None,
            });
        }
    }
    fn errors(&self) -> &[ActionsParseError] {
        &self.errors
    }
    fn parse(
        &mut self,
        source_id: &SourceId,
        source_map: &mut SourceMap,
    ) -> Result<AstId, ActionsParseError> {
        self.current_source_id = Some(*source_id);
        let s = source_map
            .get_text(source_id)
            .ok_or_else(|| ActionsParseError::InvalidActions {
                message: "source not found".to_string(),
                location: self.current_source_id.map(|source_id| ParseLocation {
                    source_id,
                    span: None,
                }),
            })?
            .to_string();
        self.parse_from_str(source_map, &s)
    }

    fn parse_from_str(
        &mut self,
        source_map: &mut SourceMap,
        s: &str,
    ) -> Result<AstId, ActionsParseError> {
        let yaml = YamlLoader::load_from_str(s).map_err(|e| ActionsParseError::ScanError(e))?;
        if yaml.len() != 1 {
            return Err(ActionsParseError::InvalidActions {
                message: "expected a single document".to_string(),
                location: self.current_source_id.map(|source_id| ParseLocation {
                    source_id,
                    span: None,
                }),
            });
        }

        let yaml = &yaml[0];
        let name = yaml["name"].as_str().map(|s| s.to_string());
        let run_name = yaml["run-name"].as_str().map(|s| s.to_string());
        let env = self.parse_string_map(self.get_map_value(yaml, "env"));
        let defaults = self.parse_defaults(self.get_map_value(yaml, "defaults"));
        let permissions = self.parse_permissions(self.get_map_value(yaml, "permissions"));
        let concurrency = self.parse_concurrency(self.get_map_value(yaml, "concurrency"));

        let mut jobs = Vec::new();
        if let Some(jobs_yaml) = self.get_map_value(yaml, "jobs") {
            if let Some(jobs_hash) = jobs_yaml.as_hash() {
                for (_job_id, job_yaml) in jobs_hash.iter() {
                    match self.parse_job(source_map, job_yaml) {
                        Ok(job_id) => jobs.push(job_id),
                        Err(err) => self.errors.push(err),
                    }
                }
            } else {
                self.record_error("jobs must be an object");
            }
        } else {
            self.record_error("jobs is required");
        }

        let on = match &yaml["on"] {
            Yaml::String(s) => ActionsAst::OnString(s.clone()),
            Yaml::Array(arr) => {
                let mut items = Vec::new();
                for y in arr {
                    match y.as_str() {
                        Some(s) => items.push(s.to_string()),
                        None => self.record_error("on array must contain only strings"),
                    }
                }
                ActionsAst::OnArray(items)
            }
            Yaml::Hash(_) => ActionsAst::OnObject,
            _ => {
                self.record_error("on is required");
                ActionsAst::OnObject
            }
        };

        let on = self.arena.alloc_actions(on);

        let node = ActionsAst::Workflow {
            name,
            run_name,
            jobs,
            on,
            env,
            defaults,
            permissions,
            concurrency,
        };
        Ok(self.arena.alloc_actions(node))
    }

    fn format_ast(&self, root: &AstId) -> String {
        let mut out = String::new();
        self.format_ast_impl(root, 0, &mut out);
        out
    }

    fn format_ast_impl(&self, id: &AstId, indent: usize, out: &mut String) {
        let node = self.arena.get_actions(id);
        match node {
            ActionsAst::Workflow {
                name,
                run_name,
                jobs,
                on,
                env,
                defaults,
                permissions,
                concurrency,
            } => {
                self.push_line(
                    indent,
                    &format!("Workflow name={:?} run_name={:?}", name, run_name),
                    out,
                );
                if let Some(env) = env {
                    self.push_line(indent + 1, &format!("env {:?}", env), out);
                }
                if let Some(defaults) = defaults {
                    self.push_line(
                        indent + 1,
                        &format!(
                            "defaults shell={:?} working_directory={:?}",
                            defaults.run_shell, defaults.run_working_directory
                        ),
                        out,
                    );
                }
                if let Some(permissions) = permissions {
                    self.push_line(indent + 1, &format!("permissions {:?}", permissions), out);
                }
                if let Some(concurrency) = concurrency {
                    self.push_line(indent + 1, &format!("concurrency {:?}", concurrency), out);
                }
                self.push_line(indent + 1, "on:", out);
                self.format_ast_impl(on, indent + 2, out);
                self.push_line(indent + 1, "jobs:", out);
                for job_id in jobs {
                    self.format_ast_impl(job_id, indent + 2, out);
                }
            }
            ActionsAst::OnString(s) => {
                self.push_line(indent, &format!("OnString \"{}\"", s), out);
            }
            ActionsAst::OnArray(arr) => {
                self.push_line(indent, "OnArray", out);
                for s in arr {
                    self.push_line(indent + 1, &format!("\"{}\"", s), out);
                }
            }
            ActionsAst::OnObject => {
                self.push_line(indent, "OnObject", out);
            }
            ActionsAst::Job {
                name,
                runs_on,
                steps,
                needs,
                env,
                defaults,
                permissions,
                if_cond,
                strategy,
                container,
                services,
                timeout_minutes,
                continue_on_error,
            } => {
                self.push_line(
                    indent,
                    &format!("Job name={:?} runs_on={:?}", name, runs_on),
                    out,
                );
                if let Some(needs) = needs {
                    self.push_line(indent + 1, &format!("needs {:?}", needs), out);
                }
                if let Some(env) = env {
                    self.push_line(indent + 1, &format!("env {:?}", env), out);
                }
                if let Some(defaults) = defaults {
                    self.push_line(
                        indent + 1,
                        &format!(
                            "defaults shell={:?} working_directory={:?}",
                            defaults.run_shell, defaults.run_working_directory
                        ),
                        out,
                    );
                }
                if let Some(permissions) = permissions {
                    self.push_line(indent + 1, &format!("permissions {:?}", permissions), out);
                }
                if let Some(if_cond) = if_cond {
                    self.push_line(indent + 1, &format!("if {:?}", if_cond), out);
                }
                if let Some(strategy) = strategy {
                    self.push_line(indent + 1, &format!("strategy {:?}", strategy), out);
                }
                if let Some(container) = container {
                    self.push_line(indent + 1, &format!("container {:?}", container), out);
                }
                if let Some(services) = services {
                    self.push_line(indent + 1, &format!("services {:?}", services), out);
                }
                if let Some(timeout_minutes) = timeout_minutes {
                    self.push_line(
                        indent + 1,
                        &format!("timeout_minutes {:?}", timeout_minutes),
                        out,
                    );
                }
                if let Some(continue_on_error) = continue_on_error {
                    self.push_line(
                        indent + 1,
                        &format!("continue_on_error {:?}", continue_on_error),
                        out,
                    );
                }
                self.push_line(indent + 1, "steps:", out);
                for step_id in steps {
                    self.format_ast_impl(step_id, indent + 2, out);
                }
            }
            ActionsAst::RunStep {
                run,
                name,
                id,
                if_cond,
                env,
                shell,
                working_directory,
                timeout_minutes,
                continue_on_error,
            } => {
                self.push_line(
                    indent,
                    &format!("RunStep run={:?} name={:?} id={:?}", run, name, id),
                    out,
                );
                self.push_line(indent + 1, "run:", out);
                self.format_sh_impl(run, indent + 2, out);
                if let Some(if_cond) = if_cond {
                    self.push_line(indent + 1, &format!("if {:?}", if_cond), out);
                }
                if let Some(env) = env {
                    self.push_line(indent + 1, &format!("env {:?}", env), out);
                }
                if let Some(shell) = shell {
                    self.push_line(indent + 1, &format!("shell {:?}", shell), out);
                }
                if let Some(working_directory) = working_directory {
                    self.push_line(
                        indent + 1,
                        &format!("working_directory {:?}", working_directory),
                        out,
                    );
                }
                if let Some(timeout_minutes) = timeout_minutes {
                    self.push_line(
                        indent + 1,
                        &format!("timeout_minutes {:?}", timeout_minutes),
                        out,
                    );
                }
                if let Some(continue_on_error) = continue_on_error {
                    self.push_line(
                        indent + 1,
                        &format!("continue_on_error {:?}", continue_on_error),
                        out,
                    );
                }
            }
            ActionsAst::UsesStep {
                uses,
                name,
                id,
                if_cond,
                env,
                with,
                timeout_minutes,
                continue_on_error,
            } => {
                self.push_line(
                    indent,
                    &format!("UsesStep uses=\"{}\" name={:?} id={:?}", uses, name, id),
                    out,
                );
                if let Some(if_cond) = if_cond {
                    self.push_line(indent + 1, &format!("if {:?}", if_cond), out);
                }
                if let Some(env) = env {
                    self.push_line(indent + 1, &format!("env {:?}", env), out);
                }
                if let Some(with) = with {
                    self.push_line(indent + 1, &format!("with {:?}", with), out);
                }
                if let Some(timeout_minutes) = timeout_minutes {
                    self.push_line(
                        indent + 1,
                        &format!("timeout_minutes {:?}", timeout_minutes),
                        out,
                    );
                }
                if let Some(continue_on_error) = continue_on_error {
                    self.push_line(
                        indent + 1,
                        &format!("continue_on_error {:?}", continue_on_error),
                        out,
                    );
                }
            }
            ActionsAst::Sh(sh) => {
                let node = self.arena.get_sh(*sh);
                self.push_line(indent, &format!("Sh {:?}", node), out);
            }
        }
    }

    fn format_sh_impl(&self, id: &AstId, indent: usize, out: &mut String) {
        let node = self.arena.get_sh(*id);
        match node {
            crate::actions_parser::sh_parser::sh_ast::ShAstNode::List(items) => {
                self.push_line(indent, "List", out);
                for item in items {
                    self.push_line(indent + 1, &format!("Item sep={:?}", item.sep), out);
                    self.format_sh_impl(&item.body, indent + 2, out);
                }
            }
            crate::actions_parser::sh_parser::sh_ast::ShAstNode::AndOr { first, rest } => {
                self.push_line(indent, "AndOr", out);
                self.push_line(indent + 1, "first:", out);
                self.format_sh_impl(first, indent + 2, out);
                for item in rest {
                    self.push_line(indent + 1, &format!("rest op={:?}", item.op), out);
                    self.format_sh_impl(&item.body, indent + 2, out);
                }
            }
            crate::actions_parser::sh_parser::sh_ast::ShAstNode::Pipeline { first, rest } => {
                self.push_line(indent, "Pipeline", out);
                self.format_sh_impl(first, indent + 1, out);
                for node in rest {
                    self.format_sh_impl(node, indent + 1, out);
                }
            }
            crate::actions_parser::sh_parser::sh_ast::ShAstNode::SimpleCommand {
                assignments,
                argv,
                redirs,
            } => {
                self.push_line(indent, "SimpleCommand", out);
                for a in assignments {
                    self.push_line(indent + 1, "assign:", out);
                    self.format_sh_impl(a, indent + 2, out);
                }
                for a in argv {
                    self.push_line(indent + 1, "argv:", out);
                    self.format_sh_impl(a, indent + 2, out);
                }
                for r in redirs {
                    self.push_line(indent + 1, "redir:", out);
                    self.format_sh_impl(r, indent + 2, out);
                }
            }
            crate::actions_parser::sh_parser::sh_ast::ShAstNode::If {
                cond,
                then_part,
                else_part,
            } => {
                self.push_line(indent, "If", out);
                self.push_line(indent + 1, "cond:", out);
                self.format_sh_impl(cond, indent + 2, out);
                self.push_line(indent + 1, "then:", out);
                self.format_sh_impl(then_part, indent + 2, out);
                if let Some(else_part) = else_part {
                    self.push_line(indent + 1, "else:", out);
                    self.format_sh_impl(else_part, indent + 2, out);
                }
            }
            crate::actions_parser::sh_parser::sh_ast::ShAstNode::While { cond, body } => {
                self.push_line(indent, "While", out);
                self.push_line(indent + 1, "cond:", out);
                self.format_sh_impl(cond, indent + 2, out);
                self.push_line(indent + 1, "body:", out);
                self.format_sh_impl(body, indent + 2, out);
            }
            crate::actions_parser::sh_parser::sh_ast::ShAstNode::For { var, items, body } => {
                self.push_line(indent, "For", out);
                self.push_line(indent + 1, "var:", out);
                self.format_sh_impl(var, indent + 2, out);
                for item in items {
                    self.push_line(indent + 1, "item:", out);
                    self.format_sh_impl(item, indent + 2, out);
                }
                self.push_line(indent + 1, "body:", out);
                self.format_sh_impl(body, indent + 2, out);
            }
            crate::actions_parser::sh_parser::sh_ast::ShAstNode::FunctionDef { name, body } => {
                self.push_line(indent, "FunctionDef", out);
                self.push_line(indent + 1, "name:", out);
                self.format_sh_impl(name, indent + 2, out);
                self.push_line(indent + 1, "body:", out);
                self.format_sh_impl(body, indent + 2, out);
            }
            crate::actions_parser::sh_parser::sh_ast::ShAstNode::Subshell { body } => {
                self.push_line(indent, "Subshell", out);
                self.format_sh_impl(body, indent + 1, out);
            }
            crate::actions_parser::sh_parser::sh_ast::ShAstNode::Group { body } => {
                self.push_line(indent, "Group", out);
                self.format_sh_impl(body, indent + 1, out);
            }
            crate::actions_parser::sh_parser::sh_ast::ShAstNode::Word(s) => {
                self.push_line(indent, &format!("Word {:?}", s), out);
            }
            crate::actions_parser::sh_parser::sh_ast::ShAstNode::Assignment(s) => {
                self.push_line(indent, &format!("Assignment {:?}", s), out);
            }
            crate::actions_parser::sh_parser::sh_ast::ShAstNode::Redir { op, body } => {
                self.push_line(indent, &format!("Redir op={:?} body={:?}", op, body), out);
            }
            crate::actions_parser::sh_parser::sh_ast::ShAstNode::Unknown => {
                self.push_line(indent, "Unknown", out);
            }
        }
    }

    fn push_line(&self, indent: usize, s: &str, out: &mut String) {
        for _ in 0..indent {
            out.push_str("  ");
        }
        let _ = writeln!(out, "{}", s);
    }

    fn parse_run_to_sh_ast(&mut self, source_map: &mut SourceMap, run: String) -> AstId {
        let run_source_id = source_map.add_sh_file(std::path::PathBuf::from("<run>"), run);
        let arena = std::mem::replace(&mut self.arena, AstArena::new());
        match crate::actions_parser::sh_parser::parse_sh_with_arena(
            source_map,
            &run_source_id,
            arena,
        ) {
            Ok((program, arena)) => {
                self.arena = arena;
                program.list
            }
            Err(err) => {
                let message = format!("run shell parse error: {}", err.error);
                let location = sh_error_location(&err.error).or_else(|| {
                    Some(ParseLocation {
                        source_id: run_source_id,
                        span: None,
                    })
                });
                self.errors
                    .push(ActionsParseError::InvalidActions { message, location });
                self.arena = err.arena;
                self.arena
                    .alloc_sh(crate::actions_parser::sh_parser::sh_ast::ShAstNode::Unknown)
            }
        }
    }

    fn parse_job(
        &mut self,
        source_map: &mut SourceMap,
        yaml: &Yaml,
    ) -> Result<AstId, ActionsParseError> {
        let name = self
            .get_map_value(yaml, "name")
            .and_then(|v| v.as_str().map(|s| s.to_string()));
        let runs_on = match self.get_map_value(yaml, "runs-on") {
            Some(runs_on_yaml) => match self.parse_runs_on(runs_on_yaml) {
                Ok(v) => v,
                Err(err) => {
                    self.errors.push(err);
                    RunsOn::String("<invalid>".to_string())
                }
            },
            None => {
                self.record_error("jobs.<job_id>.runs-on is required");
                RunsOn::String("<invalid>".to_string())
            }
        };
        let steps = match self.get_map_value(yaml, "steps") {
            Some(steps_yaml) => match steps_yaml.as_vec() {
                Some(steps_vec) => {
                    let mut out = Vec::new();
                    for step_yaml in steps_vec {
                        match self.parse_step(source_map, step_yaml) {
                            Ok(step_id) => out.push(step_id),
                            Err(err) => self.errors.push(err),
                        }
                    }
                    out
                }
                None => {
                    self.record_error("jobs.<job_id>.steps must be array");
                    vec![]
                }
            },
            None => {
                self.record_error("jobs.<job_id>.steps required");
                vec![]
            }
        };
        let needs = self.parse_needs(self.get_map_value(yaml, "needs"));
        let env = self.parse_string_map(self.get_map_value(yaml, "env"));
        let defaults = self.parse_defaults(self.get_map_value(yaml, "defaults"));
        let permissions = self.parse_permissions(self.get_map_value(yaml, "permissions"));
        let if_cond = self
            .get_map_value(yaml, "if")
            .and_then(|v| self.parse_scalar_value(v));
        let strategy = self.parse_strategy(self.get_map_value(yaml, "strategy"));
        let container = self.parse_container(self.get_map_value(yaml, "container"));
        let services = self.parse_services(self.get_map_value(yaml, "services"));
        let timeout_minutes = self
            .get_map_value(yaml, "timeout-minutes")
            .and_then(|v| self.parse_scalar_value(v));
        let continue_on_error = self
            .get_map_value(yaml, "continue-on-error")
            .and_then(|v| self.parse_scalar_value(v));

        let node = ActionsAst::Job {
            name,
            runs_on,
            steps,
            needs,
            env,
            defaults,
            permissions,
            if_cond,
            strategy,
            container,
            services,
            timeout_minutes,
            continue_on_error,
        };
        Ok(self.arena.alloc_actions(node))
    }

    fn parse_step(
        &mut self,
        source_map: &mut SourceMap,
        yaml: &Yaml,
    ) -> Result<AstId, ActionsParseError> {
        let name = self
            .get_map_value(yaml, "name")
            .and_then(|v| v.as_str().map(|s| s.to_string()));
        let id = self
            .get_map_value(yaml, "id")
            .and_then(|v| v.as_str().map(|s| s.to_string()));
        let if_cond = self
            .get_map_value(yaml, "if")
            .and_then(|v| self.parse_scalar_value(v));
        let env = self.parse_string_map(self.get_map_value(yaml, "env"));
        let timeout_minutes = self
            .get_map_value(yaml, "timeout-minutes")
            .and_then(|v| self.parse_scalar_value(v));
        let continue_on_error = self
            .get_map_value(yaml, "continue-on-error")
            .and_then(|v| self.parse_scalar_value(v));

        let run = self
            .get_map_value(yaml, "run")
            .and_then(|v| v.as_str().map(|s| s.to_string()));
        let uses = self
            .get_map_value(yaml, "uses")
            .and_then(|v| v.as_str().map(|s| s.to_string()));

        let node = if let Some(run) = run {
            let shell = self
                .get_map_value(yaml, "shell")
                .and_then(|v| v.as_str().map(|s| s.to_string()));
            let working_directory = self
                .get_map_value(yaml, "working-directory")
                .and_then(|v| v.as_str().map(|s| s.to_string()));
            let run_id = self.parse_run_to_sh_ast(source_map, run);
            ActionsAst::RunStep {
                run: run_id,
                name,
                id,
                if_cond,
                env,
                shell,
                working_directory,
                timeout_minutes,
                continue_on_error,
            }
        } else if let Some(uses) = uses {
            let with = self.parse_string_map(self.get_map_value(yaml, "with"));
            ActionsAst::UsesStep {
                uses,
                name,
                id,
                if_cond,
                env,
                with,
                timeout_minutes,
                continue_on_error,
            }
        } else {
            self.record_error("steps.<job_id>.run or uses required");
            ActionsAst::RunStep {
                run: self
                    .arena
                    .alloc_sh(crate::actions_parser::sh_parser::sh_ast::ShAstNode::Unknown),
                name,
                id,
                if_cond,
                env,
                shell: None,
                working_directory: None,
                timeout_minutes,
                continue_on_error,
            }
        };

        Ok(self.arena.alloc_actions(node))
    }

    fn get_map_value<'a>(&self, yaml: &'a Yaml, key: &str) -> Option<&'a Yaml> {
        if let Yaml::Hash(map) = yaml {
            map.get(&Yaml::String(key.to_string()))
        } else {
            None
        }
    }

    fn yaml_to_string_lossy(&self, yaml: &Yaml) -> String {
        match yaml {
            Yaml::String(s) => s.clone(),
            Yaml::Integer(i) => i.to_string(),
            Yaml::Real(s) => s.clone(),
            Yaml::Boolean(b) => b.to_string(),
            Yaml::Null => "null".to_string(),
            _ => {
                let mut out = String::new();
                let mut emitter = YamlEmitter::new(&mut out);
                let _ = emitter.dump(yaml);
                out
            }
        }
    }

    fn parse_string_map(&self, yaml: Option<&Yaml>) -> Option<BTreeMap<String, String>> {
        let yaml = yaml?;
        let map = yaml.as_hash()?;
        let mut out = BTreeMap::new();
        for (k, v) in map.iter() {
            let key = self.yaml_to_string_lossy(k);
            let val = self.yaml_to_string_lossy(v);
            out.insert(key, val);
        }
        Some(out)
    }

    fn parse_scalar_value(&self, yaml: &Yaml) -> Option<ScalarValue> {
        match yaml {
            Yaml::String(s) => Some(ScalarValue::String(s.clone())),
            Yaml::Integer(i) => Some(ScalarValue::Integer(*i)),
            Yaml::Real(s) => Some(ScalarValue::Float(s.clone())),
            Yaml::Boolean(b) => Some(ScalarValue::Boolean(*b)),
            Yaml::Null => Some(ScalarValue::String("null".to_string())),
            _ => Some(ScalarValue::String(self.yaml_to_string_lossy(yaml))),
        }
    }

    fn parse_needs(&self, yaml: Option<&Yaml>) -> Option<StringOrArray> {
        let yaml = yaml?;
        match yaml {
            Yaml::String(s) => Some(StringOrArray::String(s.clone())),
            Yaml::Array(arr) => {
                let items = arr.iter().map(|y| self.yaml_to_string_lossy(y)).collect();
                Some(StringOrArray::Array(items))
            }
            _ => Some(StringOrArray::String(self.yaml_to_string_lossy(yaml))),
        }
    }

    fn parse_defaults(&self, yaml: Option<&Yaml>) -> Option<Defaults> {
        let yaml = yaml?;
        let run_yaml = self.get_map_value(yaml, "run")?;
        Some(Defaults {
            run_shell: self
                .get_map_value(run_yaml, "shell")
                .and_then(|v| v.as_str().map(|s| s.to_string())),
            run_working_directory: self
                .get_map_value(run_yaml, "working-directory")
                .and_then(|v| v.as_str().map(|s| s.to_string())),
        })
    }

    fn parse_permissions(&self, yaml: Option<&Yaml>) -> Option<Permissions> {
        let yaml = yaml?;
        match yaml {
            Yaml::String(s) => Some(Permissions::String(s.clone())),
            Yaml::Hash(_) => self.parse_string_map(Some(yaml)).map(Permissions::Map),
            _ => Some(Permissions::String(self.yaml_to_string_lossy(yaml))),
        }
    }

    fn parse_concurrency(&self, yaml: Option<&Yaml>) -> Option<Concurrency> {
        let yaml = yaml?;
        match yaml {
            Yaml::String(s) => Some(Concurrency::String(s.clone())),
            Yaml::Hash(_) => {
                let group = self
                    .get_map_value(yaml, "group")
                    .and_then(|v| v.as_str().map(|s| s.to_string()))?;
                let cancel_in_progress = self
                    .get_map_value(yaml, "cancel-in-progress")
                    .and_then(|v| self.parse_scalar_value(v));
                Some(Concurrency::Group {
                    group,
                    cancel_in_progress,
                })
            }
            _ => Some(Concurrency::String(self.yaml_to_string_lossy(yaml))),
        }
    }

    fn parse_runs_on(&self, yaml: &Yaml) -> Result<RunsOn, ActionsParseError> {
        match yaml {
            Yaml::String(s) => Ok(RunsOn::String(s.clone())),
            Yaml::Array(arr) => Ok(RunsOn::Array(
                arr.iter().map(|y| self.yaml_to_string_lossy(y)).collect(),
            )),
            Yaml::Hash(_) => {
                let group = self
                    .get_map_value(yaml, "group")
                    .and_then(|v| v.as_str().map(|s| s.to_string()));
                let labels_yaml = self.get_map_value(yaml, "labels");
                let labels = match labels_yaml {
                    Some(Yaml::String(s)) => vec![s.clone()],
                    Some(Yaml::Array(arr)) => {
                        arr.iter().map(|y| self.yaml_to_string_lossy(y)).collect()
                    }
                    Some(other) => vec![self.yaml_to_string_lossy(other)],
                    None => vec![],
                };
                Ok(RunsOn::GroupLabels { group, labels })
            }
            _ => Ok(RunsOn::String(self.yaml_to_string_lossy(yaml))),
        }
    }

    fn parse_container(&self, yaml: Option<&Yaml>) -> Option<Container> {
        let yaml = yaml?;
        match yaml {
            Yaml::String(s) => Some(Container::String(s.clone())),
            Yaml::Hash(_) => {
                let image = self
                    .get_map_value(yaml, "image")
                    .and_then(|v| v.as_str().map(|s| s.to_string()))?;
                let credentials_yaml = self.get_map_value(yaml, "credentials");
                let credentials = credentials_yaml.and_then(|c| {
                    Some(ContainerCredentials {
                        username: self
                            .get_map_value(c, "username")
                            .and_then(|v| v.as_str().map(|s| s.to_string())),
                        password: self
                            .get_map_value(c, "password")
                            .and_then(|v| v.as_str().map(|s| s.to_string())),
                    })
                });
                let env = self.parse_string_map(self.get_map_value(yaml, "env"));
                let ports = self
                    .get_map_value(yaml, "ports")
                    .and_then(|v| {
                        v.as_vec()
                            .map(|arr| arr.iter().map(|y| self.yaml_to_string_lossy(y)).collect())
                    })
                    .unwrap_or_default();
                let volumes = self
                    .get_map_value(yaml, "volumes")
                    .and_then(|v| {
                        v.as_vec()
                            .map(|arr| arr.iter().map(|y| self.yaml_to_string_lossy(y)).collect())
                    })
                    .unwrap_or_default();
                let options = self
                    .get_map_value(yaml, "options")
                    .and_then(|v| v.as_str().map(|s| s.to_string()));
                Some(Container::Spec(ContainerSpec {
                    image,
                    credentials,
                    env,
                    ports,
                    volumes,
                    options,
                }))
            }
            _ => Some(Container::String(self.yaml_to_string_lossy(yaml))),
        }
    }

    fn parse_services(&self, yaml: Option<&Yaml>) -> Option<BTreeMap<String, Container>> {
        let yaml = yaml?;
        let map = yaml.as_hash()?;
        let mut out = BTreeMap::new();
        for (k, v) in map.iter() {
            let name = self.yaml_to_string_lossy(k);
            if let Some(container) = self.parse_container(Some(v)) {
                out.insert(name, container);
            }
        }
        Some(out)
    }

    fn parse_strategy(&self, yaml: Option<&Yaml>) -> Option<Strategy> {
        let yaml = yaml?;
        let matrix_yaml = self.get_map_value(yaml, "matrix")?;
        let matrix = self.yaml_to_string_lossy(matrix_yaml);
        let fail_fast = self
            .get_map_value(yaml, "fail-fast")
            .and_then(|v| self.parse_scalar_value(v));
        let max_parallel = self
            .get_map_value(yaml, "max-parallel")
            .and_then(|v| self.parse_scalar_value(v));
        Some(Strategy {
            matrix,
            fail_fast,
            max_parallel,
        })
    }
}

pub(crate) fn parse_actions_yaml(
    source_map: &mut SourceMap,
    source_id: &SourceId,
) -> Result<(AstId, AstArena, Vec<ActionsParseError>), ActionsParseError> {
    let mut parser = ActionsParser::new();
    let root = parser.parse(source_id, source_map)?;
    let ActionsParser { arena, errors, .. } = parser;
    Ok((root, arena, errors))
}

pub(crate) fn format_actions_tree(arena: &AstArena, root: &AstId) -> String {
    let parser = ActionsParser {
        arena: arena.clone(),
        errors: vec![],
        current_source_id: None,
    };
    parser.format_ast(root)
}

#[cfg(test)]
mod actions_parser_tests {
    use crate::actions_parser::actions_ast::{
        ActionsAst, Concurrency, Container, RunsOn, ScalarValue, StringOrArray,
    };
    use crate::actions_parser::parser::ActionsParser;

    #[test]
    fn test() {
        let mut parser = ActionsParser::new();
        let mut source_map = crate::actions_parser::source_map::SourceMap::new();
        let s = r#"name: Unit Test

on:
  pull_request:
    branches: [ main, develop ]
  push:
    branches: [ main, develop ]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v6
      - uses: ./.github/actions/setup-java

      - name: Unit Test
        run: ./gradlew clean desktopTest --stacktrace --no-daemon"#;
        let root = parser.parse_from_str(&mut source_map, s).unwrap();
        let tree = parser.format_ast(&root);
        assert!(parser.errors().is_empty());
        assert!(tree.contains("Workflow name=Some(\"Unit Test\") run_name=None"));
        assert!(tree.contains("OnObject"));
        assert!(tree.contains("Job name=None runs_on=String(\"ubuntu-latest\")"));
        assert!(tree.contains("UsesStep uses=\"actions/checkout@v6\""));
        assert!(tree.contains("UsesStep uses=\"./.github/actions/setup-java\""));
        assert!(tree.contains("RunStep run="));
        assert!(tree.contains("name=Some(\"Unit Test\")"));
    }

    #[test]
    fn parse_extended_fields() {
        let mut parser = ActionsParser::new();
        let mut source_map = crate::actions_parser::source_map::SourceMap::new();
        let s = r#"
name: CI
run-name: Build ${{ github.ref }}
env:
  RUST_LOG: info
defaults:
  run:
    shell: bash
    working-directory: ./app
permissions: read-all
concurrency:
  group: ci-${{ github.ref }}
  cancel-in-progress: true
on: push
jobs:
  test:
    name: Unit Test
    runs-on: [self-hosted, linux]
    needs: [setup]
    if: ${{ github.event_name == 'push' }}
    strategy:
      matrix:
        os: [ubuntu-latest]
        java: [17]
    container:
      image: ghcr.io/example/app:latest
      options: --cpus 2
    services:
      redis:
        image: redis:7
    timeout-minutes: 30
    continue-on-error: false
    steps:
      - name: Setup
        uses: actions/checkout@v4
        with:
          fetch-depth: 1
      - name: Test
        run: ./gradlew test
        shell: bash
        working-directory: ./app
        env:
          JAVA_HOME: /opt/java
"#;
        let root = parser.parse_from_str(&mut source_map, s).unwrap();
        assert!(parser.errors().is_empty());
        let workflow = parser.arena.get_actions(&root);
        match workflow {
            ActionsAst::Workflow {
                name,
                run_name,
                jobs,
                env,
                defaults,
                permissions,
                concurrency,
                ..
            } => {
                assert_eq!(name.as_deref(), Some("CI"));
                assert!(run_name.as_deref().unwrap().contains("Build"));
                assert_eq!(
                    env.as_ref().unwrap().get("RUST_LOG").map(String::as_str),
                    Some("info")
                );
                let defaults = defaults.as_ref().unwrap();
                assert_eq!(defaults.run_shell.as_deref(), Some("bash"));
                assert_eq!(defaults.run_working_directory.as_deref(), Some("./app"));
                match permissions.as_ref().unwrap() {
                    crate::actions_parser::actions_ast::Permissions::String(s) => {
                        assert_eq!(s, "read-all");
                    }
                    _ => panic!("unexpected permissions"),
                }
                match concurrency.as_ref().unwrap() {
                    Concurrency::Group {
                        group,
                        cancel_in_progress,
                    } => {
                        assert!(group.contains("ci-"));
                        assert!(matches!(
                            cancel_in_progress,
                            Some(ScalarValue::Boolean(true))
                        ));
                    }
                    _ => panic!("unexpected concurrency"),
                }
                assert_eq!(jobs.len(), 1);
                let job = parser.arena.get_actions(&jobs[0]);
                match job {
                    ActionsAst::Job {
                        name,
                        runs_on,
                        needs,
                        container,
                        services,
                        timeout_minutes,
                        continue_on_error,
                        steps,
                        ..
                    } => {
                        assert_eq!(name.as_deref(), Some("Unit Test"));
                        match runs_on {
                            RunsOn::Array(arr) => {
                                assert!(arr.contains(&"self-hosted".to_string()));
                            }
                            _ => panic!("runs-on not array"),
                        }
                        assert!(matches!(needs, Some(StringOrArray::Array(_))));
                        assert!(matches!(timeout_minutes, Some(ScalarValue::Integer(30))));
                        assert!(matches!(
                            continue_on_error,
                            Some(ScalarValue::Boolean(false))
                        ));
                        match container.as_ref().unwrap() {
                            Container::Spec(spec) => {
                                assert_eq!(spec.image, "ghcr.io/example/app:latest");
                                assert_eq!(spec.options.as_deref(), Some("--cpus 2"));
                            }
                            _ => panic!("container not spec"),
                        }
                        let services = services.as_ref().unwrap();
                        assert!(services.contains_key("redis"));
                        assert_eq!(steps.len(), 2);
                    }
                    _ => panic!("unexpected job"),
                }
            }
            _ => panic!("unexpected workflow"),
        }
    }

    #[test]
    fn invalid_inputs_are_collected() {
        let mut source_map = crate::actions_parser::source_map::SourceMap::new();
        let s = r#"
name: Bad
on: [push, 1]
jobs:
  test:
    runs-on: 123
    steps:
      - run: if
"#;
        let source_id = source_map.add_yaml(
            std::path::PathBuf::from("bad.yml"),
            "workflow".to_string(),
            s.to_string(),
        );
        let (root, arena, errors) =
            crate::actions_parser::parse_actions_yaml(&mut source_map, &source_id).unwrap();
        let tree = crate::actions_parser::format_actions_tree(&arena, &root);
        assert!(!tree.is_empty());
        assert!(!errors.is_empty());
    }
}
