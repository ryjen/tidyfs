use crate::db::Database;
use crate::rules::Risk;
use crate::util;
use anyhow::{bail, Result};
use rusqlite::params;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use tidyfs::ai_contract::{AiDeterministicFacts, AiObservation, AiPathMode};

pub const MAX_LABELS_PER_CANDIDATE: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedCandidate {
    pub identity_id: i64,
    pub path: PathBuf,
    pub labels: Vec<String>,
    pub size_bytes: u64,
    pub max_mtime: Option<i64>,
}

pub fn load_candidates(database: &Database, scan_id: i64) -> Result<Vec<IndexedCandidate>> {
    load_candidates_matching(database, scan_id, None)
}

pub fn load_candidate(
    database: &Database,
    scan_id: i64,
    path: &Path,
) -> Result<Option<IndexedCandidate>> {
    Ok(load_candidates_matching(database, scan_id, Some(path))?
        .into_iter()
        .next())
}

pub fn observation_for(
    scan_id: i64,
    candidate: &IndexedCandidate,
    path_mode: AiPathMode,
    max_risk: Risk,
) -> AiObservation {
    let path = privacy_path(&candidate.path, path_mode);
    let classification = (candidate.labels.len() == 1).then(|| candidate.labels[0].clone());
    let protected = candidate.labels.iter().any(|label| is_protected_label(label));
    let age_seconds = candidate.max_mtime.and_then(|mtime| {
        let age = util::unix_now().saturating_sub(mtime);
        (age >= 0).then_some(age as u64)
    });

    AiObservation {
        scan_id,
        candidate_key: format!("scan-{scan_id}:classification-{}", candidate.identity_id),
        path,
        path_mode,
        size_bytes: candidate.size_bytes,
        age_seconds,
        labels: candidate.labels.clone(),
        deterministic: AiDeterministicFacts {
            classification,
            matched_rule: None,
            protected,
            max_allowed_risk: max_risk.to_string(),
        },
        adapter: None,
    }
}

pub fn reconstruct_bound_observation(
    database: &Database,
    scan_id: i64,
    path: &Path,
    path_mode: AiPathMode,
    max_risk: Risk,
    analyzed_digest: &str,
) -> Result<AiObservation> {
    let Some(candidate) = load_candidate(database, scan_id, path)? else {
        bail!(
            "AI recommendation is stale: classified candidate no longer exists for {}",
            path.display()
        );
    };

    let observation = observation_for(scan_id, &candidate, path_mode, max_risk);
    let current_digest = observation.digest();
    if current_digest != analyzed_digest {
        bail!(
            "AI recommendation is stale: observation digest changed for {} (analyzed={}, current={})",
            path.display(),
            analyzed_digest,
            current_digest
        );
    }

    Ok(observation)
}

pub fn path_mode_name(mode: AiPathMode) -> &'static str {
    match mode {
        AiPathMode::Full => "full",
        AiPathMode::Basename => "basename",
        AiPathMode::Redacted => "redacted",
    }
}

fn load_candidates_matching(
    database: &Database,
    scan_id: i64,
    path: Option<&Path>,
) -> Result<Vec<IndexedCandidate>> {
    let sql = if path.is_some() {
        r#"
        SELECT
          c.id,
          c.path,
          c.label,
          COALESCE(dt.allocated_size_bytes, e.allocated_size_bytes, 0) AS size_bytes,
          COALESCE(dt.max_mtime, e.mtime)
        FROM classifications c
        LEFT JOIN directory_totals dt
          ON dt.scan_id = c.scan_id
         AND dt.path = c.path
        LEFT JOIN entries e
          ON e.scan_id = c.scan_id
         AND e.path = c.path
        WHERE c.scan_id = ?1
          AND c.path = ?2
        ORDER BY c.path ASC, c.id ASC
        "#
    } else {
        r#"
        SELECT
          c.id,
          c.path,
          c.label,
          COALESCE(dt.allocated_size_bytes, e.allocated_size_bytes, 0) AS size_bytes,
          COALESCE(dt.max_mtime, e.mtime)
        FROM classifications c
        LEFT JOIN directory_totals dt
          ON dt.scan_id = c.scan_id
         AND dt.path = c.path
        LEFT JOIN entries e
          ON e.scan_id = c.scan_id
         AND e.path = c.path
        WHERE c.scan_id = ?1
        ORDER BY c.path ASC, c.id ASC
        "#
    };

    let mut stmt = database.connection().prepare(sql)?;
    let mut grouped: BTreeMap<PathBuf, IndexedCandidate> = BTreeMap::new();

    if let Some(path) = path {
        let path_text = path.to_string_lossy();
        let rows = stmt.query_map(params![scan_id, path_text.as_ref()], map_candidate_row)?;
        for row in rows {
            merge_candidate(&mut grouped, row?);
        }
    } else {
        let rows = stmt.query_map(params![scan_id], map_candidate_row)?;
        for row in rows {
            merge_candidate(&mut grouped, row?);
        }
    }

    for candidate in grouped.values_mut() {
        candidate.labels.sort();
        candidate.labels.dedup();
        candidate.labels.truncate(MAX_LABELS_PER_CANDIDATE);
    }

    Ok(grouped.into_values().collect())
}

fn map_candidate_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<(i64, PathBuf, String, u64, Option<i64>)> {
    Ok((
        row.get::<_, i64>(0)?,
        PathBuf::from(row.get::<_, String>(1)?),
        row.get::<_, String>(2)?,
        row.get::<_, i64>(3)?.max(0) as u64,
        row.get::<_, Option<i64>>(4)?,
    ))
}

fn merge_candidate(
    grouped: &mut BTreeMap<PathBuf, IndexedCandidate>,
    row: (i64, PathBuf, String, u64, Option<i64>),
) {
    let (classification_id, path, label, size_bytes, max_mtime) = row;
    grouped
        .entry(path.clone())
        .and_modify(|candidate| {
            candidate.identity_id = candidate.identity_id.min(classification_id);
            candidate.labels.push(label.clone());
            candidate.size_bytes = candidate.size_bytes.max(size_bytes);
            candidate.max_mtime = max_optional(candidate.max_mtime, max_mtime);
        })
        .or_insert_with(|| IndexedCandidate {
            identity_id: classification_id,
            path,
            labels: vec![label],
            size_bytes,
            max_mtime,
        });
}

fn privacy_path(path: &Path, mode: AiPathMode) -> String {
    match mode {
        AiPathMode::Full => path.to_string_lossy().into_owned(),
        AiPathMode::Basename => path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "<root>".to_owned()),
        AiPathMode::Redacted => redact_path(path),
    }
}

fn redact_path(path: &Path) -> String {
    let value = path.to_string_lossy().replace('\\', "/");
    let markers = [
        ".gradle/caches",
        "Library/Developer/Xcode/DerivedData",
        "DerivedData",
        ".cargo/registry",
        ".pnpm-store",
        "node_modules",
        ".cache",
        "/nix/store",
    ];

    for marker in markers {
        if let Some(index) = value.find(marker) {
            if marker == "/nix/store" {
                return "/nix/store/<redacted>".to_owned();
            }
            let suffix = &value[index..];
            if marker == ".cache" {
                let mut parts = suffix.split('/');
                let first = parts.next().unwrap_or(".cache");
                return parts.next().map_or_else(
                    || format!("<redacted>/{first}"),
                    |child| format!("<redacted>/{first}/{child}"),
                );
            }
            return format!("<redacted>/{marker}");
        }
    }

    "<redacted>".to_owned()
}

fn is_protected_label(label: &str) -> bool {
    matches!(
        label,
        "secret_material"
            | "git_repo"
            | "database"
            | "vm_image"
            | "browser_profile"
            | "docker_data"
            | "podman_data"
            | "nix_store"
            | "systemd_journal"
    )
}

fn max_optional(left: Option<i64>, right: Option<i64>) -> Option<i64> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.max(right)),
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate() -> IndexedCandidate {
        IndexedCandidate {
            identity_id: 7,
            path: PathBuf::from("/home/alice/work/private/.gradle/caches/modules-2"),
            labels: vec!["cache".to_owned()],
            size_bytes: 1024,
            max_mtime: None,
        }
    }

    #[test]
    fn redaction_preserves_known_structure_without_user_prefix() {
        assert_eq!(
            redact_path(Path::new(
                "/home/alice/work/private/.gradle/caches/modules-2"
            )),
            "<redacted>/.gradle/caches"
        );
        assert_eq!(
            redact_path(Path::new("/home/alice/.cache/pip/http-v2")),
            "<redacted>/.cache/pip"
        );
        assert_eq!(
            redact_path(Path::new("/nix/store/abc-secret-package")),
            "/nix/store/<redacted>"
        );
        assert_eq!(
            redact_path(Path::new("/home/alice/work/private-project/target")),
            "<redacted>"
        );
    }

    #[test]
    fn protected_labels_match_planner_sensitive_categories() {
        assert!(is_protected_label("secret_material"));
        assert!(is_protected_label("nix_store"));
        assert!(!is_protected_label("cache"));
    }

    #[test]
    fn observation_digest_changes_when_authoritative_facts_change() {
        let original = candidate();
        let mut changed = original.clone();
        changed.size_bytes += 1;

        let original = observation_for(42, &original, AiPathMode::Redacted, Risk::Low);
        let changed = observation_for(42, &changed, AiPathMode::Redacted, Risk::Low);
        assert_ne!(original.digest(), changed.digest());
    }

    #[test]
    fn multiple_labels_do_not_invent_a_primary_classification() {
        let mut candidate = candidate();
        candidate.labels = vec!["cache".to_owned(), "generated_artifact".to_owned()];
        let observation = observation_for(42, &candidate, AiPathMode::Full, Risk::Low);
        assert_eq!(observation.deterministic.classification, None);
        assert_eq!(
            observation.labels,
            vec!["cache".to_owned(), "generated_artifact".to_owned()]
        );
    }
}
