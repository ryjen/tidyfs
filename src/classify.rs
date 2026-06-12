use crate::db::Database;
use anyhow::Result;
use rusqlite::params;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct ClassificationResult {
    pub classifications: u64,
}

#[derive(Debug, Clone)]
struct EntryForClassify {
    path: PathBuf,
    name: String,
    entry_type: String,
    extension: Option<String>,
}

#[derive(Debug, Clone)]
struct Classification {
    label: &'static str,
    confidence: f64,
    reason: String,
}

const SOURCE: &str = "builtin_path_classifier";

pub fn classify_scan(database: &mut Database, scan_id: i64) -> Result<ClassificationResult> {
    let entries = load_entries(database, scan_id)?;
    let path_set: HashSet<PathBuf> = entries.iter().map(|e| e.path.clone()).collect();

    let tx = database.transaction()?;
    tx.execute(
        "DELETE FROM classifications WHERE scan_id = ?1 AND source = ?2",
        params![scan_id, SOURCE],
    )?;

    let mut inserted = 0_u64;
    let mut stmt = tx.prepare(
        r#"
        INSERT INTO classifications(scan_id, path, label, confidence, source, reason)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6)
        "#,
    )?;

    for entry in &entries {
        for classification in classify_entry(entry, &path_set) {
            stmt.execute(params![
                scan_id,
                entry.path.to_string_lossy(),
                classification.label,
                classification.confidence,
                SOURCE,
                classification.reason,
            ])?;
            inserted += 1;
        }
    }

    drop(stmt);
    tx.commit()?;

    Ok(ClassificationResult {
        classifications: inserted,
    })
}

pub fn print_classification_summary(database: &Database, scan_id: i64) -> Result<()> {
    let mut stmt = database.connection().prepare(
        r#"
        SELECT label, COUNT(*) AS count
        FROM classifications
        WHERE scan_id = ?1
        GROUP BY label
        ORDER BY count DESC, label ASC
        "#,
    )?;

    let rows = stmt.query_map(params![scan_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;

    println!();
    println!("{:>8}  LABEL", "COUNT");

    for row in rows {
        let (label, count) = row?;
        println!("{:>8}  {}", count, label);
    }

    Ok(())
}

fn load_entries(database: &Database, scan_id: i64) -> Result<Vec<EntryForClassify>> {
    let mut stmt = database.connection().prepare(
        r#"
        SELECT path, name, entry_type, extension
        FROM entries
        WHERE scan_id = ?1
        "#,
    )?;

    let rows = stmt
        .query_map(params![scan_id], |row| {
            Ok(EntryForClassify {
                path: PathBuf::from(row.get::<_, String>(0)?),
                name: row.get(1)?,
                entry_type: row.get(2)?,
                extension: row.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(rows)
}

fn classify_entry(entry: &EntryForClassify, path_set: &HashSet<PathBuf>) -> Vec<Classification> {
    let mut out = Vec::new();
    let path = entry.path.as_path();
    let name = entry.name.as_str();

    if is_secret_path(path, name) {
        out.push(classification(
            "secret_material",
            0.99,
            "known secret/key/password-store path",
        ));
    }

    if name == ".git" {
        out.push(classification(
            "git_repo",
            0.99,
            "Git metadata directory",
        ));
    }

    if has_ancestor_named(path, ".git") || child_exists(path, ".git", path_set) {
        out.push(classification(
            "source_repo",
            0.90,
            "path is inside or contains a Git repository",
        ));
    }

    if name == ".env" || name.ends_with(".env") || name.starts_with(".env.") {
        out.push(classification(
            "secret_material",
            0.98,
            "environment file pattern",
        ));
    }

    if is_cache_path(path, name) {
        out.push(classification(
            "cache",
            0.85,
            "path matches common cache naming/location pattern",
        ));
    }

    if path_contains(path, ".cache/thumbnails") || path_contains(path, "thumbnails") && has_ancestor_named(path, ".cache") {
        out.push(classification(
            "thumbnail_cache",
            0.95,
            "desktop thumbnail cache path",
        ));
    }

    if path_contains(path, ".local/share/Trash") || path_contains(path, ".Trash") {
        out.push(classification(
            "trash",
            0.95,
            "trash directory path",
        ));
    }

    if is_browser_profile(path) {
        out.push(classification(
            "browser_profile",
            0.95,
            "browser profile/storage path; should be protected",
        ));
    }

    if is_browser_cache(path) {
        out.push(classification(
            "browser_cache",
            0.88,
            "browser cache path",
        ));
    }

    if name == "node_modules" {
        out.push(classification(
            "node_dependencies",
            0.97,
            "Node.js dependency directory",
        ));
    }

    if path_contains(path, ".npm") || path_contains(path, ".cache/yarn") || path_contains(path, ".cache/pnpm") {
        out.push(classification(
            "node_cache",
            0.90,
            "Node package-manager cache/store path",
        ));
    }

    if matches!(name, ".next" | ".nuxt" | "dist" | "build" | ".turbo") && has_node_project_marker(path, path_set) {
        out.push(classification(
            "node_build_artifacts",
            0.82,
            "common JavaScript/TypeScript build output in a Node project",
        ));
    }

    if path_contains(path, ".cache/pip") || path_contains(path, ".cache/uv") || path_contains(path, ".cache/pypoetry") {
        out.push(classification(
            "python_cache",
            0.92,
            "Python package/tool cache path",
        ));
    }

    if matches!(name, ".venv" | "venv" | "virtualenv") {
        out.push(classification(
            "python_virtualenv",
            0.90,
            "Python virtual environment directory",
        ));
    }

    if name == "__pycache__" || matches!(entry.extension.as_deref(), Some("pyc") | Some("pyo")) {
        out.push(classification(
            "python_bytecode_cache",
            0.95,
            "Python bytecode cache",
        ));
    }

    if path_contains(path, ".cargo/registry") || path_contains(path, ".cargo/git") {
        out.push(classification(
            "rust_cache",
            0.90,
            "Cargo registry/git cache path",
        ));
    }

    if name == "target" && has_rust_project_marker(path, path_set) {
        out.push(classification(
            "rust_build_artifacts",
            0.92,
            "Rust Cargo target directory",
        ));
    }

    if path_contains(path, ".cache/go-build") || path_contains(path, "/pkg/mod") {
        out.push(classification(
            "go_cache",
            0.80,
            "Go build or module cache path",
        ));
    }

    if path_contains(path, ".gradle/caches") || path_contains(path, ".gradle/daemon") {
        out.push(classification(
            "gradle_cache",
            0.90,
            "Gradle cache/daemon path",
        ));
    }

    if path_contains(path, ".m2/repository") {
        out.push(classification(
            "maven_cache",
            0.92,
            "Maven local repository cache",
        ));
    }

    if path_contains(path, "/var/lib/docker") || path_contains(path, ".local/share/docker") {
        out.push(classification(
            "docker_data",
            0.95,
            "Docker data directory",
        ));
    }

    if path_contains(path, "/var/lib/containers") || path_contains(path, ".local/share/containers") {
        out.push(classification(
            "podman_data",
            0.90,
            "Podman/containers storage directory",
        ));
    }

    if path.starts_with("/nix/store") || path_contains(path, "/nix/store") {
        out.push(classification(
            "nix_store",
            0.99,
            "Nix store path; must be cleaned only through Nix tooling",
        ));
    }

    if path_contains(path, "/var/log/journal") || path_contains(path, "/run/log/journal") {
        out.push(classification(
            "systemd_journal",
            0.95,
            "systemd journal path",
        ));
    }

    if is_database(entry) {
        out.push(classification(
            "database",
            0.92,
            "database file extension/name",
        ));
    }

    if is_vm_image(entry) {
        out.push(classification(
            "vm_image",
            0.95,
            "virtual machine or disk image extension",
        ));
    }

    out
}

fn classification(label: &'static str, confidence: f64, reason: &str) -> Classification {
    Classification {
        label,
        confidence,
        reason: reason.to_string(),
    }
}

fn is_secret_path(path: &Path, name: &str) -> bool {
    matches!(
        name,
        ".ssh" | ".gnupg" | ".password-store" | "keyrings" | ".aws" | ".azure" | ".kube"
    ) || path_contains(path, ".config/1Password")
        || path_contains(path, ".local/share/keyrings")
        || path_contains(path, ".ssh")
        || path_contains(path, ".gnupg")
        || path_contains(path, ".password-store")
}

fn is_cache_path(path: &Path, name: &str) -> bool {
    name == ".cache"
        || name == "cache"
        || name == "Cache"
        || name.ends_with(".cache")
        || path_contains(path, "/.cache/")
        || path.ends_with(".cache")
}

fn is_browser_profile(path: &Path) -> bool {
    path_contains(path, ".mozilla/firefox")
        || path_contains(path, ".config/google-chrome")
        || path_contains(path, ".config/chromium")
        || path_contains(path, ".config/BraveSoftware")
}

fn is_browser_cache(path: &Path) -> bool {
    path_contains(path, ".cache/mozilla")
        || path_contains(path, ".cache/google-chrome")
        || path_contains(path, ".cache/chromium")
        || path_contains(path, "CacheStorage")
        || path_contains(path, "cache2")
}

fn is_database(entry: &EntryForClassify) -> bool {
    matches!(
        entry.extension.as_deref(),
        Some("sqlite") | Some("sqlite3") | Some("db") | Some("duckdb") | Some("mdb")
    ) || matches!(entry.name.as_str(), "database.sqlite" | "db.sqlite")
}

fn is_vm_image(entry: &EntryForClassify) -> bool {
    matches!(
        entry.extension.as_deref(),
        Some("vdi") | Some("vmdk") | Some("qcow2") | Some("vhd") | Some("vhdx") | Some("img") | Some("iso")
    )
}

fn has_node_project_marker(path: &Path, path_set: &HashSet<PathBuf>) -> bool {
    ancestor_contains_any(
        path,
        path_set,
        &["package.json", "pnpm-lock.yaml", "yarn.lock", "package-lock.json"],
    )
}

fn has_rust_project_marker(path: &Path, path_set: &HashSet<PathBuf>) -> bool {
    ancestor_contains_any(path, path_set, &["Cargo.toml", "Cargo.lock"])
}

fn ancestor_contains_any(path: &Path, path_set: &HashSet<PathBuf>, names: &[&str]) -> bool {
    let mut current = path.parent();

    while let Some(parent) = current {
        for name in names {
            if path_set.contains(&parent.join(name)) {
                return true;
            }
        }
        current = parent.parent();
    }

    false
}

fn child_exists(path: &Path, child_name: &str, path_set: &HashSet<PathBuf>) -> bool {
    path_set.contains(&path.join(child_name))
}

fn has_ancestor_named(path: &Path, name: &str) -> bool {
    path.ancestors().any(|ancestor| {
        ancestor
            .file_name()
            .map(|file_name| file_name == name)
            .unwrap_or(false)
    })
}

fn path_contains(path: &Path, needle: &str) -> bool {
    path.to_string_lossy().contains(needle)
}
