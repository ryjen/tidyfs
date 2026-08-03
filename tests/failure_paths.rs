use rusqlite::Connection;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

struct Sandbox {
    root: PathBuf,
    scan_root: PathBuf,
    db_path: PathBuf,
    home: PathBuf,
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
        let home = root.join("home");
        fs::create_dir_all(&scan_root).expect("create isolated scan root");
        fs::create_dir_all(&home).expect("create isolated home");
        Self {
            root,
            scan_root,
            db_path,
            home,
        }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_tidyfs"));
        command
            .arg("--db")
            .arg(&self.db_path)
            .env("HOME", &self.home);
        command
    }

    fn run(&self, args: &[&str]) -> Output {
        self.command().args(args).output().expect("run tidyfs")
    }

    fn run_with_input(&self, args: &[&str], input: &[u8]) -> Output {
        let mut child = self
            .command()
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn tidyfs");
        child
            .stdin
            .take()
            .expect("open tidyfs stdin")
            .write_all(input)
            .expect("write tidyfs stdin");
        child.wait_with_output().expect("wait for tidyfs")
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

fn prepare_candidate(sandbox: &Sandbox) -> PathBuf {
    let candidate = sandbox.scan_root.join("workspace/__pycache__/module.pyc");
    fs::create_dir_all(candidate.parent().expect("candidate parent"))
        .expect("create candidate parent");
    fs::write(&candidate, b"generated-bytecode").expect("write candidate");
    let candidate_arg = candidate.to_str().expect("UTF-8 temporary path");
    assert_success(&sandbox.run(&[
        "scan",
        sandbox.scan_root.to_str().expect("UTF-8 temporary path"),
    ]));
    assert_success(&sandbox.run(&["plan", "--safe", "--root", candidate_arg]));
    candidate
}

fn quarantine_candidate(sandbox: &Sandbox, candidate: &Path) -> (i64, PathBuf) {
    let candidate_arg = candidate.to_str().expect("UTF-8 temporary path");
    assert_success(&sandbox.run_with_input(
        &[
            "clean",
            "--safe",
            "--interactive",
            "--root",
            candidate_arg,
            "--limit",
            "1",
        ],
        b"yes\n",
    ));

    let conn = sandbox.connection();
    let (action_id, quarantine_path): (i64, String) = conn
        .query_row(
            "SELECT id, quarantine_path FROM actions ORDER BY id DESC LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("query quarantine action");
    (action_id, PathBuf::from(quarantine_path))
}

#[test]
fn stale_candidate_is_reported_without_action_or_mutation() {
    let sandbox = Sandbox::new("stale-candidate");
    let candidate = prepare_candidate(&sandbox);
    fs::remove_file(&candidate).expect("remove candidate after planning");

    let output = sandbox.run_with_input(
        &[
            "clean",
            "--safe",
            "--interactive",
            "--root",
            candidate.to_str().expect("UTF-8 temporary path"),
            "--limit",
            "1",
        ],
        b"yes\n",
    );
    assert_success(&output);
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("failed:"),
        "stale candidate failure was not reported"
    );
    assert!(!candidate.exists());

    let actions: i64 = sandbox
        .connection()
        .query_row("SELECT COUNT(*) FROM actions", [], |row| row.get(0))
        .expect("query action count");
    assert_eq!(actions, 0, "stale preflight created an action row");
}

#[test]
fn restore_collision_preserves_destination_payload_and_action_state() {
    let sandbox = Sandbox::new("restore-collision");
    let candidate = prepare_candidate(&sandbox);
    let (action_id, quarantine_path) = quarantine_candidate(&sandbox, &candidate);
    assert!(!candidate.exists());
    assert!(quarantine_path.exists());

    fs::write(&candidate, b"replacement-data").expect("create restore collision");
    let output = sandbox.run(&["restore", "--action", &action_id.to_string()]);
    assert!(
        !output.status.success(),
        "restore unexpectedly overwrote destination"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("refusing to overwrite existing destination"),
        "restore collision error was not reported"
    );
    assert_eq!(
        fs::read(&candidate).expect("read colliding destination"),
        b"replacement-data"
    );
    assert_eq!(
        fs::read(&quarantine_path).expect("read preserved quarantine payload"),
        b"generated-bytecode"
    );

    let (status, restored_at): (String, Option<i64>) = sandbox
        .connection()
        .query_row(
            "SELECT status, restored_at FROM actions WHERE id = ?1",
            [action_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("query collided restore action");
    assert_eq!(status, "quarantined");
    assert!(restored_at.is_none());
}
