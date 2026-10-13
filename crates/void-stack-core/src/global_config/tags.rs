//! Project tags: free-form labels on a registered project, plus the one
//! well-known pair that answers "is this mine or someone else's?".
//!
//! Tags are stored canonical (lowercase, `-` for spaces) in the global
//! registry. Ownership is spelled in English on disk (`own`,
//! `third-party`) so the registry reads the same whatever language the UI
//! uses; the Spanish spellings people actually type (`propio`, `tercero`)
//! are accepted as aliases.

use super::GlobalConfig;
use crate::error::{Result, VoidStackError};
use crate::model::Project;

/// Tag for a project you wrote or maintain.
pub const TAG_OWN: &str = "own";
/// Tag for a project cloned from someone else (analysis only, not yours).
pub const TAG_THIRD_PARTY: &str = "third-party";

const MAX_TAGS: usize = 16;
const MAX_TAG_LEN: usize = 32;

/// Who a project belongs to, read from its tags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ownership {
    Own,
    ThirdParty,
}

impl Ownership {
    /// The ownership a tag list declares, if any.
    pub fn of(tags: &[String]) -> Option<Self> {
        tags.iter().find_map(|t| match t.as_str() {
            TAG_OWN => Some(Self::Own),
            TAG_THIRD_PARTY => Some(Self::ThirdParty),
            _ => None,
        })
    }

    pub fn tag(self) -> &'static str {
        match self {
            Self::Own => TAG_OWN,
            Self::ThirdParty => TAG_THIRD_PARTY,
        }
    }
}

/// Canonical form of one tag, or an error explaining what is wrong with it.
/// Ownership aliases collapse onto [`TAG_OWN`] / [`TAG_THIRD_PARTY`].
pub fn normalize_tag(raw: &str) -> Result<String> {
    let tag: String = raw
        .trim()
        .trim_start_matches('#')
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-");
    if tag.is_empty() {
        return Err(invalid("empty tag".to_string()));
    }
    if tag.chars().count() > MAX_TAG_LEN {
        return Err(invalid(format!(
            "tag '{tag}' is longer than {MAX_TAG_LEN} characters"
        )));
    }
    if let Some(bad) = tag
        .chars()
        .find(|c| !(c.is_alphanumeric() || matches!(c, '-' | '_' | '.')))
    {
        return Err(invalid(format!(
            "tag '{tag}' has an invalid character '{bad}' (use letters, digits, - _ .)"
        )));
    }
    Ok(match tag.as_str() {
        "own" | "mine" | "mio" | "mío" | "propio" | "propia" => TAG_OWN.to_string(),
        "third-party" | "thirdparty" | "3rd-party" | "3rd" | "tercero" | "terceros" | "ajeno"
        | "external" => TAG_THIRD_PARTY.to_string(),
        _ => tag,
    })
}

/// What to do to a project's tags. `set` replaces the whole list first;
/// `add`/`remove` then apply on top. Adding one ownership tag drops the
/// other, so a project is never both yours and someone else's.
#[derive(Debug, Clone, Default)]
pub struct TagEdit {
    pub set: Option<Vec<String>>,
    pub add: Vec<String>,
    pub remove: Vec<String>,
}

impl TagEdit {
    pub fn is_empty(&self) -> bool {
        self.set.is_none() && self.add.is_empty() && self.remove.is_empty()
    }
}

/// Apply `edit` to `current`, returning the new canonical tag list.
pub fn apply_tag_edit(current: &[String], edit: &TagEdit) -> Result<Vec<String>> {
    let mut tags: Vec<String> = Vec::new();
    let base: Vec<String> = match &edit.set {
        Some(set) => {
            let set = normalize_all(set)?;
            if set.iter().any(|t| t == TAG_OWN) && set.iter().any(|t| t == TAG_THIRD_PARTY) {
                return Err(invalid(format!(
                    "a project can't be both '{TAG_OWN}' and '{TAG_THIRD_PARTY}'"
                )));
            }
            set
        }
        None => current.to_vec(),
    };
    for t in base {
        push_unique(&mut tags, t);
    }
    for t in normalize_all(&edit.add)? {
        let rival = match t.as_str() {
            TAG_OWN => Some(TAG_THIRD_PARTY),
            TAG_THIRD_PARTY => Some(TAG_OWN),
            _ => None,
        };
        if let Some(rival) = rival {
            tags.retain(|x| x != rival);
        }
        push_unique(&mut tags, t);
    }
    let remove = normalize_all(&edit.remove)?;
    tags.retain(|t| !remove.contains(t));
    if tags.len() > MAX_TAGS {
        return Err(invalid(format!("at most {MAX_TAGS} tags per project")));
    }
    Ok(tags)
}

/// Worker behind [`update_project_tags`]: edits the in-memory registry
/// (testable without touching the user's real config). Caller persists.
pub fn set_project_tags_in(
    config: &mut GlobalConfig,
    name: &str,
    edit: &TagEdit,
) -> Result<Project> {
    let project = config
        .projects
        .iter_mut()
        .find(|p| p.name.eq_ignore_ascii_case(name))
        .ok_or_else(|| VoidStackError::ProjectNotFound(name.to_string()))?;
    project.tags = apply_tag_edit(&project.tags, edit)?;
    Ok(project.clone())
}

/// Edit a registered project's tags and save the registry.
pub fn update_project_tags(name: &str, edit: &TagEdit) -> Result<Project> {
    let mut config = super::load_global_config()?;
    let project = set_project_tags_in(&mut config, name, edit)?;
    super::save_global_config(&config)?;
    Ok(project)
}

fn normalize_all(raw: &[String]) -> Result<Vec<String>> {
    raw.iter()
        // "a, b" from a single text box counts as two tags.
        .flat_map(|r| r.split(','))
        .filter(|r| !r.trim().is_empty())
        .map(normalize_tag)
        .collect()
}

fn push_unique(tags: &mut Vec<String>, tag: String) {
    if !tags.contains(&tag) {
        tags.push(tag);
    }
}

fn invalid(msg: String) -> VoidStackError {
    VoidStackError::InvalidConfig(msg)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    fn project(name: &str, tags: &[&str]) -> Project {
        Project {
            name: name.into(),
            description: String::new(),
            path: "/tmp/x".into(),
            project_type: None,
            tags: s(tags),
            services: vec![],
            hooks: None,
        }
    }

    #[test]
    fn test_normalize_tag_canonical_and_aliases() {
        assert_eq!(normalize_tag("  Rust  ").unwrap(), "rust");
        assert_eq!(normalize_tag("#Trading Bots").unwrap(), "trading-bots");
        assert_eq!(normalize_tag("Propio").unwrap(), TAG_OWN);
        assert_eq!(normalize_tag("tercero").unwrap(), TAG_THIRD_PARTY);
        assert_eq!(normalize_tag("3rd party").unwrap(), TAG_THIRD_PARTY);
        assert_eq!(normalize_tag("mql4").unwrap(), "mql4");
    }

    #[test]
    fn test_normalize_tag_rejects_bad_input() {
        assert!(normalize_tag("   ").is_err());
        assert!(normalize_tag("a/b").is_err());
        assert!(normalize_tag(&"x".repeat(MAX_TAG_LEN + 1)).is_err());
    }

    #[test]
    fn test_add_ownership_replaces_the_rival() {
        let edit = TagEdit {
            add: s(&["tercero"]),
            ..Default::default()
        };
        let out = apply_tag_edit(&s(&["own", "rust"]), &edit).unwrap();
        assert_eq!(out, s(&["rust", "third-party"]));
        assert_eq!(Ownership::of(&out), Some(Ownership::ThirdParty));
    }

    #[test]
    fn test_set_with_both_ownerships_is_an_error() {
        let edit = TagEdit {
            set: Some(s(&["own", "third-party"])),
            ..Default::default()
        };
        assert!(apply_tag_edit(&[], &edit).is_err());
    }

    #[test]
    fn test_set_add_remove_compose_and_dedupe() {
        let edit = TagEdit {
            set: Some(s(&["Rust, cli", "rust"])),
            add: s(&["propio", "cli"]),
            remove: s(&["CLI"]),
        };
        assert_eq!(
            apply_tag_edit(&s(&["old"]), &edit).unwrap(),
            s(&["rust", "own"])
        );
        let clear = TagEdit {
            set: Some(vec![]),
            ..Default::default()
        };
        assert!(apply_tag_edit(&s(&["own"]), &clear).unwrap().is_empty());
    }

    #[test]
    fn test_set_project_tags_in_registry() {
        let mut config = GlobalConfig {
            projects: vec![project("xerj", &[])],
            ..Default::default()
        };
        let edit = TagEdit {
            add: s(&["tercero", "search"]),
            ..Default::default()
        };
        let p = set_project_tags_in(&mut config, "XERJ", &edit).unwrap();
        assert_eq!(p.tags, s(&["third-party", "search"]));
        assert_eq!(config.projects[0].tags, p.tags);
        assert!(set_project_tags_in(&mut config, "missing", &edit).is_err());
    }

    #[test]
    fn test_ownership_none_when_untagged() {
        assert_eq!(Ownership::of(&s(&["rust"])), None);
        assert_eq!(
            Ownership::of(&s(&["own"])).map(Ownership::tag),
            Some(TAG_OWN)
        );
    }
}
