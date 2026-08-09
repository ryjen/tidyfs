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
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before Unix epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "tidyfs-risk-threshold-{}-{nonce}",
            std::process::id()
        ));
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
            .env("HOME", self.root.join("home"))
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
fn plan_applies_rule_matching_and_risk_thresholds_end_to_end() {
    let sandbox = Sandbox::new();
    let bytecode = sandbox.scan_root.join("workspace/__pycache__/module.pyc");
    let node_module = sandbox
        .scan_root
        .join("workspace/node_modules/example/index.js");
    fs::create_dir_all(bytecode.parent().expect("bytecode parent"))
        .expect("create bytecode fixture");
    fs::create_dir_all(node_module.parent().expect("node module parent"))
        .expect("create node fixture");
    fs::write(&bytecode, b"generated-bytecode").expect("write bytecode fixture");
    fs::write(&node_module, b"module.exports = {};").expect("write node fixture");

    assert_success(&sandbox.run(&[
        "scan",
        sandbox.scan_root.to_str().expect("UTF-8 temporary path"),
    ]));
    assert_success(&sandbox.run(&["plan", "--safe"]));

    let conn = sandbox.connection();
    let (python_risk, python_blocked): (String, i64) = conn
        .query_row(
            "SELECT risk, blocked FROM cleanup_candidates WHERE rule_id = 'python-bytecode-cache' LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("query low-risk Python candidate");
    assert_eq!(python_risk, "low");
    assert_eq!(python_blocked, 0);

    let (node_risk, node_blocked, node_reason): (String, i64, Option<String>) = conn
        .query_row(
            "SELECT risk, blocked, blocked_reason FROM cleanup_candidates WHERE rule_id = 'node-modules-project-deps' LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("query medium-risk node_modules candidate");
    assert_eq!(node_risk, "medium");
    assert_eq!(node_blocked, 1);
    assert!(
        node_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("risk medium exceeds selected threshold low")),
        "unexpected block reason: {node_reason:?}"
    );
    drop(conn);

    assert_success(&sandbox.run(&["plan", "--risk", "medium"]));

    let conn = sandbox.connection();
    let node_blocked_at_medium: i64 = conn
        .query_row(
            "SELECT blocked FROM cleanup_candidates WHERE rule_id = 'node-modules-project-deps' LIMIT 1",
            [],
            |row| row.get(0),
        )
        .expect("query medium-threshold node_modules candidate");
    assert_eq!(node_blocked_at_medium, 0);

    let python_blocked_at_medium: i64 = conn
        .query_row(
            "SELECT blocked FROM cleanup_candidates WHERE rule_id = 'python-bytecode-cache' LIMIT 1",
            [],
            |row| row.get(0),
        )
        .expect("query medium-threshold Python candidate");
    assert_eq!(python_blocked_at_medium, 0);
}
