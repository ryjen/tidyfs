use rusqlite::Connection;
use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Output, Stdio};
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
            "tidyfs-concurrent-clean-{}-{nonce}",
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

fn wait_for_prompt(child: &mut Child, expected: &str) {
    let mut stdout = child.stdout.take().expect("open first clean stdout");
    let mut received = Vec::new();
    let expected = expected.as_bytes();
    let mut byte = [0_u8; 1];

    loop {
        let read = stdout.read(&mut byte).expect("read first clean stdout");
        assert_ne!(
            read,
            0,
            "first clean exited before prompt; stdout={}",
            String::from_utf8_lossy(&received)
        );
        received.push(byte[0]);
        if received.ends_with(expected) {
            return;
        }
    }
}

#[test]
fn second_concurrent_clean_is_rejected_by_database_scoped_lock() {
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

    let clean_args = [
        "clean",
        "--safe",
        "--interactive",
        "--root",
        candidate_arg,
        "--limit",
        "1",
    ];
    let mut first = sandbox
        .command()
        .args(clean_args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn first clean");

    wait_for_prompt(
        &mut first,
        "Proceed with quarantine? Type 'yes' to continue: ",
    );

    let second = sandbox.run(&clean_args);
    assert!(
        !second.status.success(),
        "second clean unexpectedly acquired the mutation lock\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&second.stdout),
        String::from_utf8_lossy(&second.stderr)
    );
    assert!(
        String::from_utf8_lossy(&second.stderr)
            .contains("another tidyfs mutation is already running"),
        "second clean did not report lock contention: {}",
        String::from_utf8_lossy(&second.stderr)
    );

    first
        .stdin
        .take()
        .expect("open first clean stdin")
        .write_all(b"no\n")
        .expect("abort first clean");
    let first_status = first.wait().expect("wait for first clean");
    assert!(first_status.success(), "first clean did not abort cleanly");

    assert_eq!(
        fs::read(&candidate).expect("read preserved candidate"),
        payload
    );
    let actions: i64 = sandbox
        .connection()
        .query_row("SELECT COUNT(*) FROM actions", [], |row| row.get(0))
        .expect("query action count");
    assert_eq!(actions, 0, "aborted first clean unexpectedly mutated state");
}
