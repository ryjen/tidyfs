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

    let (action_id, quarantine_path, status): (i64, String, String) = sandbox
        .connection()
        .query_row(
            "SELECT id, quarantine_path, status FROM actions ORDER BY id DESC LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("query quarantined action");
    assert_eq!(status, "quarantined");
    (action_id, PathBuf::from(quarantine_path))
}

#[test]
fn moving_status_database_failure_prevents_quarantine_rename_and_recovers_terminally() {
    let sandbox = Sandbox::new("moving-status-db-failure");
    let candidate = prepare_candidate(&sandbox);
    let payload = fs::read(&candidate).expect("read candidate before cleanup");

    let conn = sandbox.connection();
    conn.execute_batch(
        r#"
        CREATE TRIGGER fail_moving_status
        BEFORE UPDATE OF status ON actions
        WHEN NEW.status = 'moving'
        BEGIN
          SELECT RAISE(ABORT, 'injected moving status failure');
        END;
        "#,
    )
    .expect("install moving-status failure trigger");
    drop(conn);

    let clean = sandbox.run_with_input(
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
    assert_success(&clean);
    assert!(
        String::from_utf8_lossy(&clean.stderr).contains("injected moving status failure"),
        "clean did not report injected transition failure: {}",
        String::from_utf8_lossy(&clean.stderr)
    );
    assert_eq!(
        fs::read(&candidate).expect("source after failed transition"),
        payload
    );

    let conn = sandbox.connection();
    let (action_id, status, quarantine_path): (i64, String, String) = conn
        .query_row(
            "SELECT id, status, quarantine_path FROM actions ORDER BY id DESC LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("query planned action");
    assert_eq!(status, "planned");
    assert!(
        !PathBuf::from(&quarantine_path).exists(),
        "payload moved despite failure before moving state"
    );
    conn.execute_batch("DROP TRIGGER fail_moving_status;")
        .expect("remove moving-status failure trigger");
    drop(conn);

    let warning = sandbox.run(&["actions"]);
    assert_success(&warning);
    assert!(
        String::from_utf8_lossy(&warning.stderr).contains("interrupted tidyfs action"),
        "planned action was not detected at startup"
    );

    assert_success(&sandbox.run(&["recover", "--action", &action_id.to_string()]));

    let conn = sandbox.connection();
    let (recovered_status, error): (String, Option<String>) = conn
        .query_row(
            "SELECT status, error FROM actions WHERE id = ?1",
            [action_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("query recovered planned action");
    assert_eq!(recovered_status, "failed");
    assert!(
        error
            .as_deref()
            .is_some_and(|message| message.contains("move did not complete")),
        "unexpected recovery error: {error:?}"
    );
    assert_eq!(fs::read(candidate).expect("source after recovery"), payload);
}

#[test]
fn restoring_status_database_failure_preserves_quarantine_and_allows_retry() {
    let sandbox = Sandbox::new("restoring-status-db-failure");
    let candidate = prepare_candidate(&sandbox);
    let payload = fs::read(&candidate).expect("read candidate before quarantine");
    let (action_id, quarantine_path) = quarantine_candidate(&sandbox, &candidate);
    assert!(!candidate.exists());
    assert_eq!(
        fs::read(&quarantine_path).expect("read quarantined payload"),
        payload
    );

    let conn = sandbox.connection();
    conn.execute_batch(
        r#"
        CREATE TRIGGER fail_restoring_status
        BEFORE UPDATE OF status ON actions
        WHEN NEW.status = 'restoring'
        BEGIN
          SELECT RAISE(ABORT, 'injected restoring status failure');
        END;
        "#,
    )
    .expect("install restoring-status failure trigger");
    drop(conn);

    let restore = sandbox.run(&["restore", "--action", &action_id.to_string()]);
    assert!(
        !restore.status.success(),
        "restore unexpectedly succeeded despite injected transition failure"
    );
    assert!(
        String::from_utf8_lossy(&restore.stderr).contains("injected restoring status failure"),
        "restore did not report injected transition failure: {}",
        String::from_utf8_lossy(&restore.stderr)
    );
    assert!(!candidate.exists());
    assert_eq!(
        fs::read(&quarantine_path).expect("quarantine after failed restore transition"),
        payload
    );

    let conn = sandbox.connection();
    let status: String = conn
        .query_row(
            "SELECT status FROM actions WHERE id = ?1",
            [action_id],
            |row| row.get(0),
        )
        .expect("query action after restoring transition failure");
    assert_eq!(status, "quarantined");
    conn.execute_batch("DROP TRIGGER fail_restoring_status;")
        .expect("remove restoring-status failure trigger");
    drop(conn);

    let actions = sandbox.run(&["actions"]);
    assert_success(&actions);
    assert!(
        !String::from_utf8_lossy(&actions.stderr).contains("interrupted tidyfs action"),
        "quarantined retryable action was incorrectly classified as interrupted"
    );

    assert_success(&sandbox.run(&["restore", "--action", &action_id.to_string()]));
    assert_eq!(
        fs::read(&candidate).expect("restored retry payload"),
        payload
    );
    assert!(!quarantine_path.exists());

    let final_status: String = sandbox
        .connection()
        .query_row(
            "SELECT status FROM actions WHERE id = ?1",
            [action_id],
            |row| row.get(0),
        )
        .expect("query final restored status");
    assert_eq!(final_status, "restored");
}
