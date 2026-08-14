use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize)]
pub struct Rule {
    pub id: String,
    pub label: String,
    pub category: String,
    pub risk: Risk,
    pub action_type: ActionType,
    pub reversible: bool,
    #[serde(default)]
    pub r#match: RuleMatch,
    pub reason: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct RuleMatch {
    #[serde(default)]
    pub labels_any: Vec<String>,
    #[serde(default)]
    pub path_contains_any: Vec<String>,
    pub path_basename: Option<String>,
    pub older_than_days: Option<u64>,
    pub min_size_bytes: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Risk {
    Low,
    Medium,
    High,
    Forbidden,
}

impl Risk {
    pub fn as_str(self) -> &'static str {
        match self {
            Risk::Low => "low",
            Risk::Medium => "medium",
            Risk::High => "high",
            Risk::Forbidden => "forbidden",
        }
    }
}

impl fmt::Display for Risk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionType {
    ReportOnly,
    Trash,
    Quarantine,
    ToolNative,
}

impl ActionType {
    pub fn as_str(self) -> &'static str {
        match self {
            ActionType::ReportOnly => "report_only",
            ActionType::Trash => "trash",
            ActionType::Quarantine => "quarantine",
            ActionType::ToolNative => "tool_native",
        }
    }
}

impl fmt::Display for ActionType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct HierarchyInput<'a> {
    pub path: &'a Path,
    pub risk: Risk,
    pub blocked: bool,
    pub blocked_reason: Option<&'a str>,
    pub reversible: bool,
    pub action_type: &'a str,
    pub rule_id: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HierarchyDecision {
    pub source_index: usize,
    pub effective_risk: Risk,
    pub blocked: bool,
    pub blocked_reason: Option<String>,
}

pub fn load_builtin_rules() -> Result<Vec<Rule>> {
    let raw = include_str!("../rules/default.yaml");
    serde_yaml::from_str(raw).context("parsing built-in rules/default.yaml")
}

pub fn risk_allows(candidate: Risk, max: Risk) -> bool {
    candidate <= max && candidate != Risk::Forbidden
}

pub fn basename(path: &Path) -> Option<String> {
    path.file_name().map(|s| s.to_string_lossy().to_string())
}

/// Collapse cleanup policy to one deterministic decision per filesystem payload.
///
/// Adapter pseudo-paths are intentionally passed through unchanged because they do not
/// represent raw filesystem mutation candidates. Filesystem paths are first grouped by
/// exact path using the most restrictive matching policy, then every ancestor that has
/// any independently classified descendant is blocked. The descendant suppresses the
/// ancestor even when the descendant is itself blocked or higher-risk; this prevents an
/// ancestor from becoming a policy bypass and prevents overlapping reclaim-byte totals.
pub fn canonicalize_hierarchy(inputs: &[HierarchyInput<'_>]) -> Vec<HierarchyDecision> {
    #[derive(Debug)]
    struct CanonicalEntry {
        path: PathBuf,
        filesystem: bool,
        decision: HierarchyDecision,
    }

    let mut filesystem_groups: BTreeMap<PathBuf, Vec<usize>> = BTreeMap::new();
    let mut canonical = Vec::new();

    for (index, input) in inputs.iter().enumerate() {
        if is_adapter_path(input.path) {
            canonical.push(CanonicalEntry {
                path: input.path.to_path_buf(),
                filesystem: false,
                decision: HierarchyDecision {
                    source_index: index,
                    effective_risk: input.risk,
                    blocked: input.blocked,
                    blocked_reason: input.blocked_reason.map(str::to_owned),
                },
            });
        } else {
            filesystem_groups
                .entry(input.path.to_path_buf())
                .or_default()
                .push(index);
        }
    }

    for (path, mut indexes) in filesystem_groups {
        indexes.sort_by(|left_index, right_index| {
            let left = &inputs[*left_index];
            let right = &inputs[*right_index];
            right
                .risk
                .cmp(&left.risk)
                .then_with(|| right.blocked.cmp(&left.blocked))
                .then_with(|| left.reversible.cmp(&right.reversible))
                .then_with(|| {
                    is_raw_filesystem_action(left.action_type)
                        .cmp(&is_raw_filesystem_action(right.action_type))
                })
                .then_with(|| left.rule_id.cmp(right.rule_id))
                .then_with(|| left_index.cmp(right_index))
        });

        let representative = indexes[0];
        let effective_risk = indexes
            .iter()
            .map(|index| inputs[*index].risk)
            .max()
            .unwrap_or(Risk::Forbidden);
        let has_blocked_match = indexes.iter().any(|index| inputs[*index].blocked);
        let has_non_reversible_match = indexes.iter().any(|index| !inputs[*index].reversible);
        let has_non_executable_match = indexes
            .iter()
            .any(|index| !is_raw_filesystem_action(inputs[*index].action_type));
        let blocked = has_blocked_match || has_non_reversible_match || has_non_executable_match;

        let blocked_reason = if blocked {
            indexes
                .iter()
                .find_map(|index| inputs[*index].blocked_reason.map(str::to_owned))
                .or_else(|| {
                    has_non_reversible_match.then(|| {
                        "exact-path policy conflict includes a non-reversible match; filesystem cleanup suppressed"
                            .to_owned()
                    })
                })
                .or_else(|| {
                    has_non_executable_match.then(|| {
                        "exact-path policy conflict includes a non-quarantine/trash match; filesystem cleanup suppressed"
                            .to_owned()
                    })
                })
                .or_else(|| {
                    Some(
                        "exact-path policy conflict includes a blocked match; filesystem cleanup suppressed"
                            .to_owned(),
                    )
                })
        } else {
            None
        };

        canonical.push(CanonicalEntry {
            path,
            filesystem: true,
            decision: HierarchyDecision {
                source_index: representative,
                effective_risk,
                blocked,
                blocked_reason,
            },
        });
    }

    let filesystem_paths: Vec<_> = canonical
        .iter()
        .enumerate()
        .filter(|(_, entry)| entry.filesystem)
        .map(|(index, entry)| (index, entry.path.clone()))
        .collect();

    for (entry_index, path) in &filesystem_paths {
        let has_descendant = filesystem_paths.iter().any(|(other_index, other_path)| {
            other_index != entry_index && other_path != path && other_path.starts_with(path)
        });
        if has_descendant {
            let entry = &mut canonical[*entry_index];
            if !entry.decision.blocked {
                entry.decision.blocked = true;
                entry.decision.blocked_reason = Some(
                    "overlapping descendant candidate exists; ancestor suppressed to avoid double-counting or policy bypass"
                        .to_owned(),
                );
            }
        }
    }

    let mut decisions: Vec<_> = canonical.into_iter().map(|entry| entry.decision).collect();
    decisions.sort_by_key(|decision| decision.source_index);
    decisions
}

fn is_adapter_path(path: &Path) -> bool {
    path.to_string_lossy().starts_with("adapter://")
}

fn is_raw_filesystem_action(action_type: &str) -> bool {
    matches!(action_type, "quarantine" | "trash")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input<'a>(
        path: &'a str,
        risk: Risk,
        blocked: bool,
        blocked_reason: Option<&'a str>,
        reversible: bool,
        action_type: &'a str,
        rule_id: &'a str,
    ) -> HierarchyInput<'a> {
        HierarchyInput {
            path: Path::new(path),
            risk,
            blocked,
            blocked_reason,
            reversible,
            action_type,
            rule_id,
        }
    }

    #[test]
    fn exact_path_uses_highest_risk_and_one_representative() {
        let inputs = [
            input("/cache", Risk::Low, false, None, true, "quarantine", "low"),
            input(
                "/cache",
                Risk::Medium,
                false,
                None,
                true,
                "quarantine",
                "medium",
            ),
        ];

        let decisions = canonicalize_hierarchy(&inputs);
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].source_index, 1);
        assert_eq!(decisions[0].effective_risk, Risk::Medium);
        assert!(!decisions[0].blocked);
    }

    #[test]
    fn restrictive_exact_match_blocks_more_permissive_match() {
        let inputs = [
            input("/cache", Risk::Low, false, None, true, "quarantine", "safe"),
            input(
                "/cache",
                Risk::High,
                true,
                Some("protected by stricter rule"),
                false,
                "report_only",
                "strict",
            ),
        ];

        let decisions = canonicalize_hierarchy(&inputs);
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].effective_risk, Risk::High);
        assert!(decisions[0].blocked);
        assert_eq!(
            decisions[0].blocked_reason.as_deref(),
            Some("protected by stricter rule")
        );
    }

    #[test]
    fn descendants_suppress_all_ancestors_even_when_descendant_is_blocked() {
        let inputs = [
            input("/cache", Risk::Low, false, None, true, "quarantine", "parent"),
            input(
                "/cache/sub",
                Risk::Low,
                false,
                None,
                true,
                "quarantine",
                "child",
            ),
            input(
                "/cache/sub/private",
                Risk::Forbidden,
                true,
                Some("protected descendant"),
                false,
                "report_only",
                "leaf",
            ),
        ];

        let decisions = canonicalize_hierarchy(&inputs);
        assert_eq!(decisions.len(), 3);
        assert!(decisions.iter().all(|decision| decision.blocked));
        assert!(decisions[0]
            .blocked_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("overlapping descendant")));
        assert!(decisions[1]
            .blocked_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("overlapping descendant")));
        assert_eq!(
            decisions[2].blocked_reason.as_deref(),
            Some("protected descendant")
        );
    }

    #[test]
    fn leaf_most_non_overlapping_paths_remain_available() {
        let inputs = [
            input("/cache", Risk::Low, false, None, true, "quarantine", "parent"),
            input(
                "/cache/a",
                Risk::Low,
                false,
                None,
                true,
                "quarantine",
                "a",
            ),
            input(
                "/cache/b",
                Risk::Low,
                false,
                None,
                true,
                "quarantine",
                "b",
            ),
        ];

        let decisions = canonicalize_hierarchy(&inputs);
        assert!(decisions[0].blocked);
        assert!(!decisions[1].blocked);
        assert!(!decisions[2].blocked);
    }

    #[test]
    fn adapter_pseudo_paths_do_not_enter_filesystem_hierarchy_policy() {
        let inputs = [
            input(
                "adapter://docker",
                Risk::Medium,
                false,
                None,
                false,
                "tool_native",
                "docker",
            ),
            input(
                "/adapter:/docker/cache",
                Risk::Low,
                false,
                None,
                true,
                "quarantine",
                "filesystem",
            ),
        ];

        let decisions = canonicalize_hierarchy(&inputs);
        assert_eq!(decisions.len(), 2);
        assert!(decisions.iter().all(|decision| !decision.blocked));
    }
}
