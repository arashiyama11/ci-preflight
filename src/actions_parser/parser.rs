use crate::actions_parser::actions_ast::{ActionsAst, ActionsAstId};
use crate::actions_parser::arena::ActionsAstArena;
use crate::actions_parser::parser::ActionsParseError::InvalidActions;
use crate::actions_parser::source_map::{SourceId, SourceMap};
use std::fmt::Write;
use thiserror::Error;
use yaml_rust2::{ScanError, Yaml, YamlLoader};

#[derive(Error, Debug)]
pub enum ActionsParseError {
    #[error("InternalError $0")]
    ScanError(ScanError),
    #[error("Invalid Actions yaml: $0")]
    InvalidActions(&'static str),
}

struct ActionsParser {
    arena: ActionsAstArena,
}

impl ActionsParser {
    fn new() -> ActionsParser {
        ActionsParser {
            arena: ActionsAstArena::new(),
        }
    }
    fn parse(
        &mut self,
        source_id: &SourceId,
        source_map: &SourceMap,
    ) -> Result<ActionsAstId, ActionsParseError> {
        let s = source_map.get_text(source_id).unwrap();
        self.parse_from_str(s)
    }

    fn parse_from_str(&mut self, s: &str) -> Result<ActionsAstId, ActionsParseError> {
        let yaml = YamlLoader::load_from_str(s).map_err(|e| ActionsParseError::ScanError(e))?;
        if yaml.len() != 1 {
            return Err(ActionsParseError::InvalidActions("a"));
        }

        let yaml = &yaml[0];
        let name = yaml["name"]
            .as_str()
            .ok_or(ActionsParseError::InvalidActions("name is required"))?
            .to_string();

        let jobs = yaml["jobs"]
            .as_hash()
            .ok_or(ActionsParseError::InvalidActions("jobs is required"))?
            .iter()
            .map(|y| {
                self.parse_job(y.1)
            })
            .collect::<Result<Vec<_>, _>>()?;


        let on = match &yaml["on"] {
            Yaml::String(s) => ActionsAst::OnString(s.clone()),
            Yaml::Array(arr) => ActionsAst::OnArray(arr.iter().map(|y| y.as_str().unwrap().to_string()).collect()),
            Yaml::Hash(_) => ActionsAst::OnObject,
            _ => return Err(InvalidActions("on is required")),
        };

        let on = self.arena.alloc(on);

        let node = ActionsAst::Workflow { name, jobs, on };
        Ok(self.arena.alloc(node))
    }

    fn format_ast(&self, root: &ActionsAstId) -> String {
        let mut out = String::new();
        self.format_ast_impl(root, 0, &mut out);
        out
    }

    fn format_ast_impl(&self, id: &ActionsAstId, indent: usize, out: &mut String) {
        let node = self.arena.get(id);
        match node {
            ActionsAst::Workflow { name, jobs, on } => {
                self.push_line(indent, &format!("Workflow name=\"{}\"", name), out);
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
            ActionsAst::Job { runs_on, steps } => {
                self.push_line(indent, &format!("Job runs_on=\"{}\"", runs_on), out);
                self.push_line(indent + 1, "steps:", out);
                for step_id in steps {
                    self.format_ast_impl(step_id, indent + 2, out);
                }
            }
            ActionsAst::RunStep { run } => {
                self.push_line(indent, &format!("RunStep run=\"{}\"", run), out);
            }
            ActionsAst::UsesStep { uses } => {
                self.push_line(indent, &format!("UsesStep uses=\"{}\"", uses), out);
            }
            ActionsAst::Sh(sh) => {
                self.push_line(indent, &format!("Sh {:?}", sh), out);
            }
        }
    }

    fn push_line(&self, indent: usize, s: &str, out: &mut String) {
        for _ in 0..indent {
            out.push_str("  ");
        }
        let _ = writeln!(out, "{}", s);
    }

    fn parse_job(&mut self, yaml: &Yaml) -> Result<ActionsAstId, ActionsParseError> {
        let runs_on = yaml["runs-on"]
            .as_str()
            .ok_or(InvalidActions("jobs.<job_id>.runs-on is required"))?
            .to_string();
        let steps = yaml["steps"].as_vec().ok_or(InvalidActions("jobs.<job_id>.steps required"))?.iter().map(|y| self.parse_step(y)).collect::<Result<Vec<_>, _>>()?;
        let node = ActionsAst::Job {
            runs_on,
            steps,
        };
        Ok(self.arena.alloc(node))
    }

    fn parse_step(&mut self, yaml: &Yaml) -> Result<ActionsAstId, ActionsParseError> {
        let run = yaml["run"].as_str();
        let uses = yaml["uses"].as_str();



        let node = if let Some(run) = run {
            ActionsAst::RunStep {
                run: run.to_string(),
            }
        } else if let Some(uses) = uses {
            ActionsAst::UsesStep {
                uses: uses.to_string(),
            }
        } else {
            return Err(InvalidActions("steps.<job_id>.steps required"));
        };

        Ok(self.arena.alloc(node))
    }
}

#[cfg(test)]
mod actions_parser_tests {
    use crate::actions_parser::parser::ActionsParser;

    #[test]
    fn test() {
        let mut parser = ActionsParser::new();
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
        let root = parser.parse_from_str(s).unwrap();
            let tree = parser.format_ast(&root);
            println!("{}", tree);
    }
}
