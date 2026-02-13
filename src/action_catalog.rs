use std::collections::BTreeMap;
use thiserror::Error;
use yaml_rust2::{Yaml, YamlLoader};

const WELL_KNOWN_ACTIONS_YAML: &str = include_str!("../data/well_known_actions.yaml");

#[derive(Clone, Debug)]
pub struct WellKnownAction {
    pub required_tools: Vec<String>,
    #[allow(dead_code)]
    pub confidence: Option<String>,
    #[allow(dead_code)]
    pub notes: Option<String>,
}

pub type ActionCatalog = BTreeMap<String, WellKnownAction>;

#[derive(Debug, Error)]
pub enum ActionCatalogError {
    #[error("failed to parse well-known actions YAML: {0}")]
    YamlScan(#[from] yaml_rust2::ScanError),
    #[error("well-known actions YAML must contain exactly one document")]
    InvalidDocCount,
    #[error("well-known actions YAML root must be a mapping")]
    RootNotMap,
    #[error("action key must be a string")]
    ActionKeyNotString,
    #[error("action `{0}` must be a mapping")]
    ActionValueNotMap(String),
    #[error("action `{0}` missing required field `required_tools`")]
    MissingRequiredTools(String),
    #[error("action `{0}` field `required_tools` must be a sequence of strings")]
    InvalidRequiredTools(String),
    #[error("action `{0}` field `{1}` must be a string")]
    InvalidStringField(String, &'static str),
}

pub fn load_well_known_actions() -> Result<ActionCatalog, ActionCatalogError> {
    parse_well_known_actions_yaml(WELL_KNOWN_ACTIONS_YAML)
}

fn parse_well_known_actions_yaml(raw: &str) -> Result<ActionCatalog, ActionCatalogError> {
    let docs = YamlLoader::load_from_str(raw)?;
    if docs.len() != 1 {
        return Err(ActionCatalogError::InvalidDocCount);
    }
    let root = docs.first().ok_or(ActionCatalogError::InvalidDocCount)?;
    let root_map = root.as_hash().ok_or(ActionCatalogError::RootNotMap)?;

    let mut catalog = ActionCatalog::new();
    for (k, v) in root_map {
        let key = k
            .as_str()
            .ok_or(ActionCatalogError::ActionKeyNotString)?
            .to_string();
        let action = parse_action_entry(&key, v)?;
        catalog.insert(key, action);
    }

    Ok(catalog)
}

fn parse_action_entry(key: &str, node: &Yaml) -> Result<WellKnownAction, ActionCatalogError> {
    let map = node
        .as_hash()
        .ok_or_else(|| ActionCatalogError::ActionValueNotMap(key.to_string()))?;
    let required_tools_node = map
        .get(&Yaml::String("required_tools".to_string()))
        .ok_or_else(|| ActionCatalogError::MissingRequiredTools(key.to_string()))?;

    let required_tools = required_tools_node
        .as_vec()
        .ok_or_else(|| ActionCatalogError::InvalidRequiredTools(key.to_string()))?
        .iter()
        .map(|item| {
            item.as_str()
                .map(ToString::to_string)
                .ok_or_else(|| ActionCatalogError::InvalidRequiredTools(key.to_string()))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let confidence = get_optional_string(map, key, "confidence")?;
    let notes = get_optional_string(map, key, "notes")?;

    Ok(WellKnownAction {
        required_tools,
        confidence,
        notes,
    })
}

fn get_optional_string(
    map: &yaml_rust2::yaml::Hash,
    action: &str,
    field: &'static str,
) -> Result<Option<String>, ActionCatalogError> {
    let Some(node) = map.get(&Yaml::String(field.to_string())) else {
        return Ok(None);
    };
    let value = node
        .as_str()
        .ok_or_else(|| ActionCatalogError::InvalidStringField(action.to_string(), field))?;
    Ok(Some(value.to_string()))
}

pub fn normalize_uses(uses: &str) -> Option<String> {
    if uses.starts_with("./") || uses.starts_with("../") || uses.starts_with("docker://") {
        return None;
    }

    let head = uses.split('@').next().unwrap_or(uses);
    let mut parts = head.split('/');
    let owner = parts.next()?;
    let repo = parts.next()?;

    if owner.is_empty() || repo.is_empty() {
        return None;
    }

    Some(format!("{owner}/{repo}"))
}

pub fn required_tools_for_uses<'a>(uses: &str, catalog: &'a ActionCatalog) -> Option<&'a [String]> {
    let key = normalize_uses(uses)?;
    catalog.get(&key).map(|v| v.required_tools.as_slice())
}

#[cfg(test)]
mod tests {
    use super::{
        ActionCatalog, ActionCatalogError, WellKnownAction, load_well_known_actions,
        normalize_uses, parse_well_known_actions_yaml, required_tools_for_uses,
    };
    use std::collections::BTreeMap;

    #[test]
    fn normalize_owner_repo_ref() {
        assert_eq!(
            normalize_uses("actions/checkout@v4"),
            Some("actions/checkout".to_string())
        );
        assert_eq!(
            normalize_uses("actions/setup-node/subpath@v4"),
            Some("actions/setup-node".to_string())
        );
    }

    #[test]
    fn normalize_local_and_docker_is_unknown() {
        assert_eq!(normalize_uses("./.github/actions/setup"), None);
        assert_eq!(normalize_uses("docker://alpine:3.20"), None);
    }

    #[test]
    fn resolve_required_tools() {
        let mut catalog: ActionCatalog = BTreeMap::new();
        catalog.insert(
            "actions/checkout".to_string(),
            WellKnownAction {
                required_tools: vec!["git".to_string()],
                confidence: Some("high".to_string()),
                notes: None,
            },
        );

        let tools = required_tools_for_uses("actions/checkout@v4", &catalog).unwrap();
        assert_eq!(tools, ["git"]);
        assert!(required_tools_for_uses("actions/cache@v4", &catalog).is_none());
    }

    #[test]
    fn load_embedded_catalog() {
        let catalog = load_well_known_actions().unwrap();
        assert!(catalog.contains_key("actions/checkout"));
    }

    #[test]
    fn parse_catalog_yaml_requires_required_tools() {
        let yaml = r#"
actions/checkout:
  confidence: high
"#;

        let err = parse_well_known_actions_yaml(yaml).unwrap_err();
        assert!(matches!(
            err,
            ActionCatalogError::MissingRequiredTools(action) if action == "actions/checkout"
        ));
    }
}
