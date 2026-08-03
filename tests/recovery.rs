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

fn quarantine_candidate(sandbox: &Sandbox) -> (PathBuf, i64, PathBuf) {
    let candidate = sandbox.scan_root.join("workspace/__pycache__/module.pyc");
    fs::create_dir_all(candidate.parent().expect("candidate parent"))
        .expect("create candidate parent");
    fs::write(&candidate, b"generated-bytecode").expect("write candidate");

    assert_success(&sandbox.run(&[
        "scan",
        sandbox.scan_root.to_str().expect("UTF-8 temporary path"),
    ]));
    assert_success(&sandbox.run(&[
        "plan",
        "--safe",
        "--root",
        candidate.to_str().expect("UTF-8 temporary path"),
    ]));
    assert_success(&sandbox.run_with_input(
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
    ));

    let (action_id, quarantine_path): (i64, String) = sandbox
        .connection()
        .query_row(
            "SELECT id, quarantine_path FROM actions ORDER BY id DESC LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("query quarantine action");

    (candidate, action_id, PathBuf::from(quarantine_path))
}

fn set_status(sandbox: &Sandbox, action_id: i64, status: &str) {
    sandbox
        .connection()
        .execute(
            "UPDATE actions SET status = ?1, restored_at = NULL WHERE id = ?2",
            (status, action_id),
        )
        .expect("set interrupted action status");
}

fn action_state(sandbox: &Sandbox, action_id: i64) -> (String, Option<i64>, Option<String>) {
    sandbox
        .connection()
        .query_row(
            "SELECT status, restored_at, error FROM actions WHERE id = ?1",
            [action_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("query action state")
}

fn recover_action(sandbox: &Sandbox, action_id: i64) -> Output {
    sandbox.run(&["recover", "--action", &action_id.to_string()])
}

fn assert_payload(path: &Path) {
    assert_eq!(fs::read(path).expect("read payload"), b"generated-bytecode");
}

#[test]
fn recover_moving_after_payload_move_marks_action_quarantined() {
    let sandbox = Sandbox::new("recover-moving-after-move");
    let (candidate, action_id, quarantine_path) = quarantine_candidate(&sandbox);
    set_status(&sandbox, action_id, "moving");

    let output = recover_action(&sandbox, action_id);
    assert_success(&output);

    assert!(!candidate.exists());
    assert_payload(&quarantine_path);
    let (status, restored_at, error) = action_state(&sandbox, action_id);
    assert_eq!(status, "quarantined");
    assert!(restored_at.is_none());
    assert!(error.is_none());
}

#[test]
fn recover_restoring_after_payload_move_marks_action_restored() {
    let sandbox = Sandbox::new("recover-restoring-after-move");
    let (candidate, action_id, quarantine_path) = quarantine_candidate(&sandbox);
    fs::rename(&quarantine_path, &candidate).expect("simulate restore payload move");
    set_status(&sandbox, action_id, "restoring");

    let output = recover_action(&sandbox, action_id);
    assert_success(&output);

    assert_payload(&candidate);
    assert!(!quarantine_path.exists());
    let (status, restored_at, error) = action_state(&sandbox, action_id);
    assert_eq!(status, "restored");
    assert!(restored_at.is_some());
    assert!(error.is_none());
}

#[test]
fn recover_moving_before_payload_move_marks_terminal_failure() {
    let sandbox = Sandbox::new("recover-moving-before-move");
    let (candidate, action_id, quarantine_path) = quarantine_candidate(&sandbox);
    fs::rename(&quarantine_path, &candidate).expect("simulate move not completed");
    set_status(&sandbox, action_id, "moving");

    let output = recover_action(&sandbox, action_id);
    assert_success(&output);

    assert_payload(&candidate);
    assert!(!quarantine_path.exists());
    let (status, restored_at, error) = action_state(&sandbox, action_id);
    assert_eq!(status, "failed");
    assert!(restored_at.is_none());
    assert!(error
        .expect("terminal recovery error")
        .contains("move did not complete"));
}
