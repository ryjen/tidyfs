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
    let mut stmt = conn.prepare("SELECT path, name FROM entries").expect("prepare entries query");
    let rows = stmt
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
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

    assert_eq!(fs::read(&first).expect("read first invalid fixture"), b"first");
    assert_eq!(fs::read(&second).expect("read second invalid fixture"), b"second");
}
