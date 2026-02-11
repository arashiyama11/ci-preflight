use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Clone, Debug, Deserialize)]
pub struct WellKnownAction {
    pub required_tools: Vec<String>,
    #[allow(dead_code)]
    pub confidence: Option<String>,
    #[allow(dead_code)]
    pub notes: Option<String>,
}

pub type ActionCatalog = BTreeMap<String, WellKnownAction>;

pub fn load_well_known_actions(path: &Path) -> Result<ActionCatalog, serde_json::Error> {
    let raw = std::fs::read_to_string(path).map_err(serde_json::Error::io)?;
    serde_json::from_str(&raw)
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
    use super::{ActionCatalog, WellKnownAction, normalize_uses, required_tools_for_uses};
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
}
