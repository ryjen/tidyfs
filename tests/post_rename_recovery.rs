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
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before Unix epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "tidyfs-post-rename-db-failure-{}-{nonce}",
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
fn recover_reconciles_quarantine_after_post_rename_status_failure() {
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

    let conn = sandbox.connection();
    conn.execute_batch(
        r#"
        CREATE TRIGGER fail_quarantined_status
        BEFORE UPDATE OF status ON actions
        WHEN NEW.status = 'quarantined'
        BEGIN
          SELECT RAISE(ABORT, 'injected post-rename status failure');
        END;
        "#,
    )
    .expect("install failure trigger");
    drop(conn);

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
    assert!(!candidate.exists(), "rename did not complete before failure");

    let conn = sandbox.connection();
    let (action_id, quarantine_path, status): (i64, String, String) = conn
        .query_row(
            "SELECT id, quarantine_path, status FROM actions ORDER BY id DESC LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("query interrupted action");
    assert_eq!(status, "moving");
    assert_eq!(
        fs::read(&quarantine_path).expect("read moved payload"),
        payload
    );

    conn.execute_batch("DROP TRIGGER fail_quarantined_status;")
        .expect("remove failure trigger");
    drop(conn);

    let warning = sandbox.run(&["actions"]);
    assert_success(&warning);
    assert!(
        String::from_utf8_lossy(&warning.stderr).contains("interrupted tidyfs action"),
        "startup warning missing: {}",
        String::from_utf8_lossy(&warning.stderr)
    );

    assert_success(&sandbox.run(&["recover", "--action", &action_id.to_string()]));

    let conn = sandbox.connection();
    let recovered_status: String = conn
        .query_row(
            "SELECT status FROM actions WHERE id = ?1",
            [action_id],
            |row| row.get(0),
        )
        .expect("query recovered action");
    assert_eq!(recovered_status, "quarantined");
    assert!(!candidate.exists());
    assert_eq!(
        fs::read(quarantine_path).expect("read reconciled payload"),
        payload
    );
}
