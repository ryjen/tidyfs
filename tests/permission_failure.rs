#![cfg(unix)]

use rusqlite::Connection;
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

struct Sandbox {
    root: PathBuf,
    scan_root: PathBuf,
    db_path: PathBuf,
    home: PathBuf,
}

impl Sandbox {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before Unix epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "tidyfs-permission-failure-{}-{nonce}",
            std::process::id()
        ));
        let scan_root = root.join("scan-root");
        let db_path = root.join("state/tidyfs.db");
        let home = root.join("home");
        fs::create_dir_all(&scan_root).expect("create scan root");
        fs::create_dir_all(&home).expect("create home");
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
            .expect("open stdin")
            .write_all(input)
            .expect("write stdin");
        child.wait_with_output().expect("wait for tidyfs")
    }

    fn connection(&self) -> Connection {
        Connection::open(&self.db_path).expect("open database")
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let quarantine_root = self.home.join(".local/share/tidyfs/quarantine");
        if quarantine_root.exists() {
            let _ = fs::set_permissions(&quarantine_root, fs::Permissions::from_mode(0o755));
        }
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
fn quarantine_permission_failure_preserves_source_and_records_failed_action() {
    let sandbox = Sandbox::new();
    let candidate = sandbox.scan_root.join("workspace/__pycache__/module.pyc");
    fs::create_dir_all(candidate.parent().expect("candidate parent"))
        .expect("create candidate parent");
    let payload = b"generated-bytecode";
    fs::write(&candidate, payload).expect("write candidate");
    let candidate_arg = candidate.to_str().expect("UTF-8 temporary path");

    assert_success(&sandbox.run(&[
        "scan",
        sandbox.scan_root.to_str().expect("UTF-8 temporary path"),
    ]));
    assert_success(&sandbox.run(&["plan", "--safe", "--root", candidate_arg]));

    let quarantine_root = sandbox.home.join(".local/share/tidyfs/quarantine");
    fs::create_dir_all(&quarantine_root).expect("create quarantine root");
    fs::set_permissions(&quarantine_root, fs::Permissions::from_mode(0o555))
        .expect("make quarantine root read-only");

    let clean = sandbox.run_with_input(
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
    );
    assert_success(&clean);
    assert!(
        String::from_utf8_lossy(&clean.stderr).contains("failed:"),
        "permission failure was not reported: {}",
        String::from_utf8_lossy(&clean.stderr)
    );
    assert_eq!(
        fs::read(&candidate).expect("read source after failed quarantine"),
        payload
    );

    let conn = sandbox.connection();
    let (status, quarantine_path, error): (String, Option<String>, Option<String>) = conn
        .query_row(
            "SELECT status, quarantine_path, error FROM actions ORDER BY id DESC LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("query failed action");
    assert_eq!(status, "failed");
    let quarantine_path = quarantine_path.expect("durable quarantine intent");
    assert!(
        !PathBuf::from(quarantine_path).exists(),
        "payload unexpectedly moved despite permission failure"
    );
    assert!(
        error
            .as_deref()
            .is_some_and(|message| message.contains("creating action quarantine dir")),
        "failed action did not retain the permission error context: {error:?}"
    );

    fs::set_permissions(&quarantine_root, fs::Permissions::from_mode(0o755))
        .expect("restore quarantine permissions");
}
