#![allow(dead_code)]

use super::arena::AstId;
use std::collections::BTreeMap;

#[derive(Clone, PartialEq, PartialOrd, Debug, Eq, Ord, Hash)]
pub enum ScalarValue {
    String(String),
    Boolean(bool),
    Integer(i64),
    Float(String),
}

#[derive(Clone, PartialEq, PartialOrd, Debug, Eq, Ord, Hash)]
pub enum StringOrArray {
    String(String),
    Array(Vec<String>),
}

#[derive(Clone, Debug)]
pub enum RunsOn {
    String(String),
    Array(Vec<String>),
    GroupLabels {
        group: Option<String>,
        labels: Vec<String>,
    },
}

#[derive(Clone, Debug)]
pub enum Permissions {
    String(String),
    Map(BTreeMap<String, String>),
}

#[derive(Clone, Debug)]
pub struct Defaults {
    pub run_shell: Option<String>,
    pub run_working_directory: Option<String>,
}

#[derive(Clone, Debug)]
pub enum Concurrency {
    String(String),
    Group {
        group: String,
        cancel_in_progress: Option<ScalarValue>,
    },
}

#[derive(Clone, Debug)]
pub struct ContainerCredentials {
    pub username: Option<String>,
    pub password: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ContainerSpec {
    pub image: String,
    pub credentials: Option<ContainerCredentials>,
    pub env: Option<BTreeMap<String, String>>,
    pub ports: Vec<String>,
    pub volumes: Vec<String>,
    pub options: Option<String>,
}

#[derive(Clone, Debug)]
pub enum Container {
    String(String),
    Spec(ContainerSpec),
}

#[derive(Clone, Debug)]
pub struct Strategy {
    pub matrix: String,
    pub fail_fast: Option<ScalarValue>,
    pub max_parallel: Option<ScalarValue>,
}

// https://docs.github.com/ja/actions/reference/workflows-and-actions/workflow-syntax
#[derive(Clone, Debug)]
pub enum ActionsAst {
    // ignore on
    Workflow {
        name: Option<String>,
        run_name: Option<String>,
        jobs: Vec<AstId>,
        on: AstId,
        env: Option<BTreeMap<String, String>>,
        defaults: Option<Defaults>,
        permissions: Option<Permissions>,
        concurrency: Option<Concurrency>,
    },
    OnString(String),
    OnArray(Vec<String>),
    OnObject,
    Job {
        name: Option<String>,
        runs_on: RunsOn,
        steps: Vec<AstId>,
        needs: Option<StringOrArray>,
        env: Option<BTreeMap<String, String>>,
        defaults: Option<Defaults>,
        permissions: Option<Permissions>,
        if_cond: Option<ScalarValue>,
        strategy: Option<Strategy>,
        container: Option<Container>,
        services: Option<BTreeMap<String, Container>>,
        timeout_minutes: Option<ScalarValue>,
        continue_on_error: Option<ScalarValue>,
    },
    RunStep {
        run: AstId,
        name: Option<String>,
        id: Option<String>,
        if_cond: Option<ScalarValue>,
        env: Option<BTreeMap<String, String>>,
        shell: Option<String>,
        working_directory: Option<String>,
        timeout_minutes: Option<ScalarValue>,
        continue_on_error: Option<ScalarValue>,
    },
    UsesStep {
        uses: String,
        name: Option<String>,
        id: Option<String>,
        if_cond: Option<ScalarValue>,
        env: Option<BTreeMap<String, String>>,
        with: Option<BTreeMap<String, String>>,
        timeout_minutes: Option<ScalarValue>,
        continue_on_error: Option<ScalarValue>,
    },
    Sh(AstId),
}
