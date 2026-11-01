//! Read-only MCP stdio tools over the existing deterministic TidyFS index.
//! No model API, filesystem scan, cleanup, migration or background listener.
use crate::{ai_facts, db::Database, recommend, rules::Risk};
use anyhow::{bail, Context, Result};
use rusqlite::params;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::io::{self, BufRead, Write};
use std::path::Path;
use tidyfs::ai_contract::AiPathMode;
use tidyfs::ai_goal::{AiGoalRequest, AI_MAX_GOAL_CANDIDATES};

const PROTOCOL_VERSION: &str = "2025-11-25";
const MAX_MESSAGE_BYTES: usize = 64 * 1024;
const MAX_RESULT_BYTES: usize = 128 * 1024;
const MAX_SCANS: u32 = 25;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ListScansArgs {
    #[serde(default = "default_scan_limit")]
    limit: u32,
}

fn default_scan_limit() -> u32 {
    MAX_SCANS
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CandidateArgs {
    scan_id: Option<i64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlanArgs {
    scan_id: Option<i64>,
    target_bytes: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SelectionArgs {
    scan_id: i64,
    target_bytes: u64,
    plan_digest: String,
    selected_candidate_ids: Vec<i64>,
}

pub fn serve(db_path: &Path) -> Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    run_stdio(&mut stdin.lock(), &mut stdout.lock(), db_path)
}

fn run_stdio<R: BufRead, W: Write>(input: &mut R, output: &mut W, db_path: &Path) -> Result<()> {
    let mut initialized = false;
    while let Some(line) = read_frame(input)? {
        let request: Value = match serde_json::from_slice(&line) {
            Ok(value) => value,
            Err(_) => {
                emit(output, jsonrpc_error(Value::Null, -32700, "Invalid JSON"))?;
                continue;
            }
        };
        if request.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
            emit(output, jsonrpc_error(request.get("id").cloned().unwrap_or(Value::Null), -32600, "Invalid JSON-RPC version"))?;
            continue;
        }
        let Some(method) = request.get("method").and_then(Value::as_str) else {
            // Responses are not solicited by this server.
            if request.get("id").is_some() {
                emit(output, jsonrpc_error(request["id"].clone(), -32600, "Invalid request"))?;
            }
            continue;
        };
        let id = request.get("id").cloned();
        if id.as_ref().is_some_and(|id| !id.is_string() && !id.is_number()) {
            emit(output, jsonrpc_error(Value::Null, -32600, "Invalid request id"))?;
            continue;
        }
        // Ignore notifications, which never receive a JSON-RPC response.
        if id.is_none() {
            if method == "notifications/initialized" && initialized {
                continue;
            }
            continue;
        }
        let id = id.unwrap_or(Value::Null);
        let params = request.get("params").cloned().unwrap_or_else(|| json!({}));
        let response = match method {
            "initialize" => {
                if !params.is_object() || !params.get("protocolVersion").is_some_and(Value::is_string) {
                    jsonrpc_error(id, -32602, "Missing MCP protocol version")
                } else {
                    initialized = true;
                    jsonrpc_ok(id, json!({
                        "protocolVersion": PROTOCOL_VERSION,
                        "capabilities": {"tools": {"listChanged": false}},
                        "serverInfo": {"name": "tidyfs", "version": env!("CARGO_PKG_VERSION")},
                        "instructions": "Read-only snapshots of a pre-existing TidyFS index. Tool outputs are advisory, not cleanup authorization."
                    }))
                }
            }
            "ping" => jsonrpc_ok(id, json!({})),
            _ if !initialized => jsonrpc_error(id, -32000, "Initialize MCP first"),
            "tools/list" => jsonrpc_ok(id, json!({"tools": tools()})),
            "tools/call" => {
                if let Some(name) = params.get("name").and_then(Value::as_str) {
                    if !matches!(name, "tidyfs.list_scans" | "tidyfs.list_candidates" | "tidyfs.get_plan" | "tidyfs.evaluate_selection") {
                        jsonrpc_error(id, -32602, "Unknown tool")
                    } else {
                        let arguments = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
                        match invoke(db_path, name, arguments) {
                            Ok(value) => {
                                if value.to_string().len() > MAX_RESULT_BYTES {
                                    jsonrpc_ok(id, tool_error("Result exceeds the MCP response limit"))
                                } else {
                                    jsonrpc_ok(id, tool_success(value))
                                }
                            }
                            Err(message) => jsonrpc_ok(id, tool_error(&message)),
                        }
                    }
                } else {
                    jsonrpc_error(id, -32602, "Missing tool name")
                }
            }
            _ => jsonrpc_error(id, -32601, "Method not found"),
        };
        emit(output, response)?;
    }
    Ok(())
}

// Do not allow an attacker-controlled line to allocate unbounded memory.
fn read_frame<R: BufRead>(input: &mut R) -> Result<Option<Vec<u8>>> {
    let mut data = Vec::new();
    loop {
        let available = input.fill_buf()?;
        if available.is_empty() {
            if data.is_empty() {
                return Ok(None);
            }
            bail!("MCP input ended in a partial message");
        }
        let portion = available.iter().position(|byte| *byte == b'\n');
        let take = portion.map_or(available.len(), |at| at + 1);
        if data.len().saturating_add(take) > MAX_MESSAGE_BYTES {
            bail!("MCP input exceeds maximum message size");
        }
        data.extend_from_slice(&available[..take]);
        input.consume(take);
        if portion.is_some() {
            return Ok(Some(data));
        }
    }
}

fn emit<W: Write>(output: &mut W, response: Value) -> Result<()> {
    serde_json::to_writer(&mut *output, &response)?;
    output.write_all(b"\n")?;
    output.flush().context("sending MCP response")
}

fn jsonrpc_ok(id: Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn jsonrpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

fn tool_success(value: Value) -> Value {
    json!({
        "content": [{"type": "text", "text": value.to_string()}],
        "structuredContent": value,
        "isError": false
    })
}

fn tool_error(message: &str) -> Value {
    json!({"content": [{"type": "text", "text": message}], "isError": true})
}

fn tools() -> Vec<Value> {
    let read_only = json!({
        "readOnlyHint": true,
        "destructiveHint": false,
        "idempotentHint": true,
        "openWorldHint": false
    });
    vec![
        json!({
            "name": "tidyfs.list_scans",
            "description": "List up to 25 completed, previously indexed scan IDs. Does not scan the filesystem.",
            "annotations": read_only,
            "inputSchema": {
                "type": "object",
                "properties": {"limit": {"type": "integer", "minimum": 1, "maximum": MAX_SCANS}},
                "additionalProperties": false
            }
        }),
        json!({
            "name": "tidyfs.list_candidates",
            "description": "List eligible non-overlapping, low-risk reversible candidates from a persisted deterministic plan. Paths redacted.",
            "annotations": read_only,
            "inputSchema": {
                "type": "object",
                "properties": {"scan_id": {"type": "integer", "minimum": 1}},
                "additionalProperties": false
            }
        }),
        json!({
            "name": "tidyfs.get_plan",
            "description": "Return a read-only, goal-bound candidate snapshot and digest for selecting existing low-risk candidate IDs.",
            "annotations": read_only,
            "inputSchema": {
                "type": "object",
                "properties": {
                    "scan_id": {"type": "integer", "minimum": 1},
                    "target_bytes": {"type": "integer", "minimum": 1}
                },
                "required": ["target_bytes"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "tidyfs.evaluate_selection",
            "description": "Revalidate a plan digest and selected candidate IDs against current persisted low-risk eligibility, then sum bytes. Never cleans.",
            "annotations": read_only,
            "inputSchema": {
                "type": "object",
                "properties": {
                    "scan_id": {"type": "integer", "minimum": 1},
                    "target_bytes": {"type": "integer", "minimum": 1},
                    "plan_digest": {"type": "string", "minLength": 1},
                    "selected_candidate_ids": {
                        "type": "array",
                        "items": {"type": "integer", "minimum": 1},
                        "maxItems": AI_MAX_GOAL_CANDIDATES,
                        "uniqueItems": true
                    }
                },
                "required": ["scan_id", "target_bytes", "plan_digest", "selected_candidate_ids"],
                "additionalProperties": false
            }
        })
    ]
}

fn invoke(db_path: &Path, name: &str, arguments: Value) -> std::result::Result<Value, String> {
    if !arguments.is_object() {
        return Err("Tool arguments must be a JSON object".into());
    }
    // Opening read-only neither creates nor migrates TidyFS state. Hide database/OS
    // errors from tool content to avoid leaking machine-local path details.
    let database = Database::open_read_only(db_path)
        .map_err(|_| "TidyFS index unavailable. Run scan and plan outside MCP first".to_owned())?;
    match name {
        "tidyfs.list_scans" => {
            let args: ListScansArgs = parse_args(arguments)?;
            if args.limit == 0 || args.limit > MAX_SCANS {
                return Err("limit must be between 1 and 25".into());
            }
            let mut stmt = database.connection()
                .prepare("SELECT id, finished_at FROM scans WHERE status = 'completed' ORDER BY finished_at DESC, id DESC LIMIT ?1")
                .map_err(|_| "Cannot read completed scans".to_owned())?;
            let ids: Vec<Value> = stmt.query_map(params![i64::from(args.limit)], |row| {
                Ok(json!({"scan_id": row.get::<_, i64>(0)?, "finished_at": row.get::<_, Option<i64>>(1)?}))
            }).map_err(|_| "Cannot read completed scans".to_owned())?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(|_| "Cannot read completed scans".to_owned())?;
            Ok(json!({"scans": ids, "mutation_authority": false}))
        }
        "tidyfs.list_candidates" => {
            let args: CandidateArgs = parse_args(arguments)?;
            let scan_id = completed_scan_id(&database, args.scan_id)?;
            let candidates = eligible(&database, scan_id)?;
            Ok(json!({
                "scan_id": scan_id,
                "risk_threshold": "low",
                "path_mode": "redacted",
                "candidates": recommend::build_contract_candidates(&candidates, AiPathMode::Redacted),
                "mutation_authority": false
            }))
        }
        "tidyfs.get_plan" => {
            let args: PlanArgs = parse_args(arguments)?;
            if args.target_bytes == 0 {
                return Err("target_bytes must be positive".into());
            }
            let scan_id = completed_scan_id(&database, args.scan_id)?;
            let request = goal_request(&database, scan_id, args.target_bytes)?;
            Ok(json!({"plan": request, "mutation_authority": false}))
        }
        "tidyfs.evaluate_selection" => {
            let args: SelectionArgs = parse_args(arguments)?;
            if args.scan_id <= 0 || args.target_bytes == 0 || args.plan_digest.is_empty() {
                return Err("scan_id, target_bytes and plan_digest must be valid".into());
            }
            if args.selected_candidate_ids.len() > AI_MAX_GOAL_CANDIDATES {
                return Err("Too many candidate IDs".into());
            }
            let scan_id = completed_scan_id(&database, Some(args.scan_id))?;
            let request = goal_request(&database, scan_id, args.target_bytes)?;
            if request.plan_digest != args.plan_digest {
                return Err("Stale or mismatched plan digest; request a new plan".into());
            }
            let mut seen = BTreeSet::new();
            let mut sum = 0_u64;
            for id in &args.selected_candidate_ids {
                if *id <= 0 || !seen.insert(*id) {
                    return Err("Invalid or duplicate candidate ID".into());
                }
                let candidate = request.candidates.iter()
                    .find(|candidate| candidate.candidate_id == *id)
                    .ok_or_else(|| "Selected ID is not eligible in the current plan".to_owned())?;
                sum = sum.checked_add(candidate.size_bytes)
                    .ok_or_else(|| "Selected bytes overflow".to_owned())?;
            }
            Ok(json!({
                "scan_id": scan_id,
                "plan_digest": request.plan_digest,
                "selected_candidate_ids": args.selected_candidate_ids,
                "selected_bytes": sum,
                "target_bytes": args.target_bytes,
                "target_met": sum >= args.target_bytes,
                "risk_threshold": "low",
                "mutation_authority": false,
                "validated_against_persisted_plan": true
            }))
        }
        _ => Err("Unknown tool".into()),
    }
}

fn parse_args<T: for<'de> Deserialize<'de>>(value: Value) -> std::result::Result<T, String> {
    serde_json::from_value(value).map_err(|_| "Invalid or unsupported tool arguments".to_owned())
}

fn completed_scan_id(database: &Database, requested: Option<i64>) -> std::result::Result<i64, String> {
    let id = match requested {
        Some(id) if id > 0 => id,
        Some(_) => return Err("scan_id must be positive".into()),
        None => database.latest_completed_scan()
            .map_err(|_| "No completed scans; run tidyfs scan first".to_owned())?.id,
    };
    let status: String = database.connection()
        .query_row("SELECT status FROM scans WHERE id = ?1", params![id], |row| row.get(0))
        .map_err(|_| "Selected scan does not exist".to_owned())?;
    if status != "completed" {
        return Err("Selected scan is not completed".into());
    }
    Ok(id)
}

fn eligible(database: &Database, scan_id: i64) -> std::result::Result<Vec<recommend::PlanCandidate>, String> {
    let candidates = recommend::load_eligible_candidates(
        database, scan_id, None, Risk::Low, AI_MAX_GOAL_CANDIDATES,
    ).map_err(|_| "Cannot load an eligible deterministic plan".to_owned())?;
    if candidates.is_empty() {
        return Err("No eligible candidates. Run tidyfs plan --safe outside MCP first".into());
    }
    Ok(candidates)
}

fn goal_request(database: &Database, scan_id: i64, target_bytes: u64) -> std::result::Result<AiGoalRequest, String> {
    let candidates = eligible(database, scan_id)?;
    let request = AiGoalRequest::new(
        format!("mcp-scan-{scan_id}"),
        scan_id,
        recommend::build_contract_candidates(&candidates, AiPathMode::Redacted),
        target_bytes,
        "low".to_owned(),
        None,
    );
    request.validate().map_err(|_| "Invalid deterministic plan snapshot".to_owned())?;
    Ok(request)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_path() -> PathBuf {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)
            .expect("time").as_nanos();
        std::env::temp_dir().join(format!("tidyfs-mcp-{}-{nonce}.db", std::process::id()))
    }

    fn fixture() -> PathBuf {
        let path = test_path();
        let db = Database::open(&path).expect("db");
        db.migrate().expect("migrate");
        db.connection().execute(
            "INSERT INTO scans(id, root_path, started_at, finished_at, status) VALUES(42, '/secret/path', 1, 2, 'completed')", []
        ).expect("scan");
        db.connection().execute(
            "INSERT INTO cleanup_candidates(id, scan_id, path, size_bytes, rule_id, rule_label, category, risk, action_type, reversible, reason, blocked, created_at) VALUES(7, 42, '/secret/path/.gradle/caches', 1000, 'test-rule', 'test rule', 'build', 'low', 'quarantine', 1, 'fixture', 0, 1)", []
        ).expect("eligible");
        db.connection().execute(
            "INSERT INTO cleanup_candidates(id, scan_id, path, size_bytes, rule_id, rule_label, category, risk, action_type, reversible, reason, blocked, created_at) VALUES(8, 42, '/secret/path/.cache/persistent', 9000, 'test-blocked', 'blocked', 'data', 'low', 'quarantine', 1, 'fixture', 1, 1)", []
        ).expect("blocked");
        path
    }

    fn exchange(db_path: &Path, methods: &[Value]) -> Vec<Value> {
        let input = methods.iter().map(Value::to_string)
            .collect::<Vec<_>>().join("\n") + "\n";
        let mut output = Vec::new();
        run_stdio(&mut Cursor::new(input.as_bytes()), &mut output, db_path)
            .expect("run MCP");
        String::from_utf8(output).expect("utf8")
            .lines().map(|line| serde_json::from_str(line).expect("json")).collect()
    }

    fn initialize() -> Value {
        json!({"jsonrpc":"2.0", "id":1, "method":"initialize", "params":{"protocolVersion":PROTOCOL_VERSION, "capabilities": {}, "clientInfo":{"name":"test","version":"1"}}})
    }

    fn call(id: i32, name: &str, arguments: Value) -> Value {
        json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":arguments}})
    }

    #[test]
    fn stdio_handshake_and_tools_are_read_only() {
        let path = test_path(); // deliberately absent; listing must not create it
        let outputs = exchange(&path, &[initialize(),
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
            json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
        ]);
        assert_eq!(outputs.len(), 2);
        assert_eq!(outputs[0]["result"]["protocolVersion"], PROTOCOL_VERSION);
        let tools = outputs[1]["result"]["tools"].as_array().expect("tools");
        assert_eq!(tools.len(), 4);
        assert!(tools.iter().all(|tool| tool["annotations"]["readOnlyHint"] == true));
        assert!(!path.exists());
    }

    #[test]
    fn plan_selection_is_digest_bound_and_never_exposes_full_path() {
        let path = fixture();
        let calls = [initialize(),
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
            call(2, "tidyfs.get_plan", json!({"scan_id":42, "target_bytes":500})),
        ];
        let output = exchange(&path, &calls);
        let plan = &output[1]["result"]["structuredContent"]["plan"];
        assert_eq!(plan["candidates"].as_array().expect("candidates").len(), 1);
        assert!(!plan.to_string().contains("/secret/path"));
        assert_eq!(plan["candidates"][0]["candidate_id"], 7);
        let digest = plan["plan_digest"].as_str().expect("digest");
        let ok = exchange(&path, &[initialize(), call(3, "tidyfs.evaluate_selection", json!({
            "scan_id": 42, "target_bytes": 500, "plan_digest": digest, "selected_candidate_ids":[7]
        }))]);
        assert_eq!(ok[1]["result"]["structuredContent"]["selected_bytes"], 1000);
        assert_eq!(ok[1]["result"]["structuredContent"]["target_met"], true);
        let bad = exchange(&path, &[initialize(), call(4, "tidyfs.evaluate_selection", json!({
            "scan_id": 42, "target_bytes": 500, "plan_digest": digest, "selected_candidate_ids":[8]
        }))]);
        assert_eq!(bad[1]["result"]["isError"], true);
        std::fs::remove_file(&path).expect("remove fixture");
    }

    #[test]
    fn stale_selection_fails_closed() {
        let path = fixture();
        let result = exchange(&path, &[initialize(), call(2, "tidyfs.evaluate_selection", json!({
            "scan_id": 42, "target_bytes": 500, "plan_digest": "sha256:wrong", "selected_candidate_ids":[7]
        }))]);
        assert_eq!(result[1]["result"]["isError"], true);
        std::fs::remove_file(&path).expect("remove fixture");
    }

    #[test]
    fn frame_limit_rejects_oversized_input() {
        let data = vec![b'a'; MAX_MESSAGE_BYTES + 1];
        let mut input = Cursor::new(data);
        assert!(read_frame(&mut input).is_err());
    }
}
