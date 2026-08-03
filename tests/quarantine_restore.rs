use rusqlite::Connection;
use std::fs;
use std::io::Write;
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

#[test]
fn quarantine_then_restore_preserves_payload_and_action_state() {
    let sandbox = Sandbox::new("quarantine-restore");
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

    assert!(!candidate.exists(), "candidate remained at original path");

    let conn = sandbox.connection();
    let (action_id, quarantine_path, status): (i64, String, String) = conn
        .query_row(
            "SELECT id, quarantine_path, status FROM actions ORDER BY id DESC LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("query quarantine action");
    assert_eq!(status, "quarantined");
    let quarantine_path = PathBuf::from(quarantine_path);
    assert_eq!(
        fs::read(&quarantine_path).expect("read quarantine payload"),
        payload
    );
    assert!(
        quarantine_path.starts_with(sandbox.home.join(".local/share/tidyfs/quarantine"))
    );

    assert_success(&sandbox.run(&["restore", "--action", &action_id.to_string()]));

    assert_eq!(fs::read(&candidate).expect("read restored payload"), payload);
    assert!(
        !quarantine_path.exists(),
        "quarantine payload remained after restore"
    );
    let (restored_status, restored_at): (String, Option<i64>) = conn
        .query_row(
            "SELECT status, restored_at FROM actions WHERE id = ?1",
            [action_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("query restored action");
    assert_eq!(restored_status, "restored");
    assert!(restored_at.is_some());
}
