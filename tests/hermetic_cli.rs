use rusqlite::Connection;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

struct Sandbox {
    root: PathBuf,
    scan_root: PathBuf,
    db_path: PathBuf,
}

impl Sandbox {
    fn new(name: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before Unix epoch")
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("tidyfs-{name}-{}-{nonce}", std::process::id()));
        let scan_root = root.join("scan-root");
        let db_path = root.join("state/tidyfs.db");
        fs::create_dir_all(&scan_root).expect("create isolated scan root");
        Self {
            root,
            scan_root,
            db_path,
        }
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_tidyfs"))
            .arg("--db")
            .arg(&self.db_path)
            .args(args)
            .env("HOME", self.root.join("home-not-used"))
            .output()
            .expect("run tidyfs")
    }

    fn run_with_path(&self, path: &std::path::Path, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_tidyfs"))
            .arg("--db")
            .arg(&self.db_path)
            .args(args)
            .env("HOME", self.root.join("home-not-used"))
            .env("PATH", path)
            .output()
            .expect("run tidyfs with controlled PATH")
    }

    fn run_with_empty_path(&self, args: &[&str]) -> Output {
        let empty_path = self.root.join("empty-path");
        fs::create_dir_all(&empty_path).expect("create empty PATH");
        self.run_with_path(&empty_path, args)
    }

    fn connection(&self) -> Connection {
        Connection::open(&self.db_path).expect("open isolated SQLite database")
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "command failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn adapters_json_is_versioned_and_does_not_initialize_state() {
    let sandbox = Sandbox::new("adapters-json");

    let output = sandbox.run_with_empty_path(&["adapters", "--format", "json"]);
    assert_success(&output);
    assert!(
        output.stderr.is_empty(),
        "machine mode wrote diagnostics to stderr"
    );
    assert!(
        !sandbox.db_path.exists(),
        "read-only adapter inspection initialized the TidyFS database"
    );

    let value: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("adapters output should be one JSON document");
    assert_eq!(value["schema"], "tidyfs.cli.adapters/v1");
    assert_eq!(value["command"], "adapters");

    let schema: serde_json::Value = serde_json::from_str(include_str!(
        "../schemas/tidyfs-cli-adapters-v1.schema.json"
    ))
    .expect("published adapters schema should be valid JSON");
    assert_eq!(schema["properties"]["schema"]["const"], value["schema"]);
    assert_eq!(schema["properties"]["command"]["const"], value["command"]);
    assert_eq!(schema["additionalProperties"], false);
    let adapters = value["adapters"]
        .as_array()
        .expect("adapters should be an array");
    assert_eq!(adapters.len(), 8);
    assert!(
        adapters.iter().all(|adapter| adapter["detected"] == false),
        "empty PATH should make every adapter unavailable"
    );
    assert!(
        adapters
            .iter()
            .all(|adapter| adapter["cleanup_executable"] == false),
        "adapter machine output must not imply cleanup execution authority"
    );
    assert_eq!(
        schema["properties"]["adapters"]["items"]["properties"]["cleanup_executable"]["const"],
        false
    );
}

#[cfg(unix)]
#[test]
fn adapters_json_does_not_report_non_executable_path_entries_as_detected() {
    use std::os::unix::fs::PermissionsExt;

    let sandbox = Sandbox::new("adapters-non-executable");
    let path = sandbox.root.join("fake-path");
    fs::create_dir_all(&path).expect("create controlled PATH");
    let docker = path.join("docker");
    fs::write(&docker, b"not executable").expect("write fake docker");
    let mut permissions = fs::metadata(&docker).expect("stat fake docker").permissions();
    permissions.set_mode(0o644);
    fs::set_permissions(&docker, permissions).expect("make fake docker non-executable");

    let output = sandbox.run_with_path(&path, &["adapters", "--format", "json"]);
    assert_success(&output);
    let value: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("adapters output should be JSON");
    let docker = value["adapters"]
        .as_array()
        .expect("adapters should be an array")
        .iter()
        .find(|adapter| adapter["name"] == "docker")
        .expect("docker adapter should be present");
    assert_eq!(
        docker["detected"], false,
        "non-executable PATH entries must not be reported as detected"
    );
}

#[test]
fn adapters_human_output_remains_default_and_read_only() {
    let sandbox = Sandbox::new("adapters-human");

    let output = sandbox.run_with_empty_path(&["adapters"]);
    assert_success(&output);
    assert!(output.stderr.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stdout).starts_with("Adapters:\n"),
        "default human output changed unexpectedly"
    );
    assert!(
        !sandbox.db_path.exists(),
        "human adapter inspection initialized the TidyFS database"
    );
}

#[test]
fn machine_format_is_not_accepted_as_cleanup_authority() {
    let sandbox = Sandbox::new("format-clean");

    let output = sandbox.run(&["clean", "--format", "json"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        !sandbox.db_path.exists(),
        "usage failure should occur before database initialization"
    );
}

#[test]
fn scan_indexes_only_the_isolated_tree_and_database() {
    let sandbox = Sandbox::new("scan");
    let project = sandbox.scan_root.join("project");
    fs::create_dir_all(project.join("target/debug")).expect("create fixture tree");
    fs::write(
        project.join("Cargo.toml"),
        "[package]\nname='fixture'\nversion='0.1.0'\n",
    )
    .expect("write Cargo manifest");
    fs::write(project.join("target/debug/artifact"), vec![b'x'; 4096])
        .expect("write fixture artifact");

    let output = sandbox.run(&["scan", sandbox.scan_root.to_str().expect("UTF-8 temp path")]);
    assert_success(&output);

    let conn = sandbox.connection();
    let completed: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM scans WHERE status = 'completed' AND root_path = ?1",
            [sandbox.scan_root.to_string_lossy().as_ref()],
            |row| row.get(0),
        )
        .expect("query completed scan");
    assert_eq!(completed, 1);

    let outside_entries: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM entries WHERE path NOT LIKE ?1",
            [format!("{}%", sandbox.scan_root.display())],
            |row| row.get(0),
        )
        .expect("query indexed paths");
    assert_eq!(outside_entries, 0, "scanner escaped the isolated root");

    let classified: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM classifications WHERE label = 'rust_build_artifacts'",
            [],
            |row| row.get(0),
        )
        .expect("query classifications");
    assert!(classified >= 1);
}

#[test]
fn dry_run_preserves_filesystem_and_records_no_actions() {
    let sandbox = Sandbox::new("dry-run");
    let bytecode = sandbox.scan_root.join("workspace/__pycache__/module.pyc");
    fs::create_dir_all(bytecode.parent().expect("fixture parent")).expect("create fixture dir");
    fs::write(&bytecode, b"generated-bytecode").expect("write fixture file");
    let before = fs::read(&bytecode).expect("read fixture before dry-run");

    assert_success(&sandbox.run(&["scan", sandbox.scan_root.to_str().expect("UTF-8 temp path")]));
    assert_success(&sandbox.run(&["plan", "--safe"]));
    assert_success(&sandbox.run(&["clean", "--dry-run", "--safe"]));

    assert_eq!(
        fs::read(&bytecode).expect("read fixture after dry-run"),
        before
    );
    let conn = sandbox.connection();
    let actions: i64 = conn
        .query_row("SELECT COUNT(*) FROM actions", [], |row| row.get(0))
        .expect("query actions");
    assert_eq!(actions, 0);
}

#[cfg(unix)]
#[test]
fn scan_records_symlink_without_following_external_target() {
    use std::os::unix::fs::symlink;

    let sandbox = Sandbox::new("symlink");
    let external = sandbox.root.join("external-secret");
    fs::write(&external, b"must not be indexed").expect("write external fixture");
    let link = sandbox.scan_root.join("external-link");
    symlink(&external, &link).expect("create symlink fixture");

    assert_success(&sandbox.run(&["scan", sandbox.scan_root.to_str().expect("UTF-8 temp path")]));

    let conn = sandbox.connection();
    let entry_type: String = conn
        .query_row(
            "SELECT entry_type FROM entries WHERE path = ?1",
            [link.to_string_lossy().as_ref()],
            |row| row.get(0),
        )
        .expect("query symlink entry");
    assert_eq!(entry_type, "symlink");

    let external_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM entries WHERE path = ?1",
            [external.to_string_lossy().as_ref()],
            |row| row.get(0),
        )
        .expect("query external target");
    assert_eq!(external_count, 0, "symlink target was followed");
}

#[cfg(unix)]
#[test]
fn scan_reports_invalid_utf8_names_without_lossy_indexing() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let sandbox = Sandbox::new("invalid-utf8");
    let first = sandbox
        .scan_root
        .join(OsString::from_vec(b"cache-\x80".to_vec()));
    let second = sandbox
        .scan_root
        .join(OsString::from_vec(b"cache-\x81".to_vec()));
    fs::write(&first, b"first").expect("write first invalid UTF-8 fixture");
    fs::write(&second, b"second").expect("write second invalid UTF-8 fixture");

    let output = sandbox.run(&["scan", sandbox.scan_root.to_str().expect("UTF-8 temp path")]);
    assert_success(&output);
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("errors: 2"),
        "scan did not report both invalid UTF-8 paths: {}",
        String::from_utf8_lossy(&output.stdout)
    );

    let conn = sandbox.connection();
    let mut stmt = conn
        .prepare("SELECT path, name FROM entries")
        .expect("prepare entries query");
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .expect("query indexed paths")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("collect indexed paths");
    assert!(
        rows.iter()
            .all(|(path, name)| !path.contains('\u{fffd}') && !name.contains('\u{fffd}')),
        "lossy replacement characters entered the index: {rows:?}"
    );

    let (invalid_errors, null_error_paths): (i64, i64) = conn
        .query_row(
            r#"
            SELECT
              SUM(CASE WHEN error LIKE '%non-UTF-8 filesystem path is unsupported%' THEN 1 ELSE 0 END),
              SUM(CASE WHEN path IS NULL THEN 1 ELSE 0 END)
            FROM scan_errors
            "#,
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("query invalid UTF-8 scan errors");
    assert_eq!(invalid_errors, 2);
    assert_eq!(null_error_paths, 2);

    assert_eq!(
        fs::read(&first).expect("read first invalid fixture"),
        b"first"
    );
    assert_eq!(
        fs::read(&second).expect("read second invalid fixture"),
        b"second"
    );
}
