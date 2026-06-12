use anyhow::{Context, Result};
use serde::Deserialize;
use std::fmt;
use std::path::Path;

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
