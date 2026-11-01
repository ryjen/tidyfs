# Read-only TidyFS MCP server (experimental)

The optional `tidyfs mcp serve` command exposes deterministic, **read-only** TidyFS index/plan facts to an explicitly launched MCP client over **stdio**. It does not start an HTTP listener and does not call a language model.

This is a tool-provider interface, not a replacement for TidyFS's existing `/v1/analyze` and `/v1/recommend` inference-client contracts. In a future Supervisor integration, the **Supervisor Orchestrator's approved MCP client/tool mediation** consumes this server; Supervisor Gateway `:8080` is an OpenAI-compatible **inference endpoint**, not an MCP transport.

## Run locally

From the canonical Nix development shell:

```bash
nix develop
mise run ci
tidyfs --db /path/to/already-created-tidyfs.db mcp serve
# or: cargo run --locked -- --db /path/to/already-created-tidyfs.db mcp serve
```

Launch from a trusted MCP client as a subprocess; stdout is reserved exclusively for newline-delimited JSON-RPC messages. There is no terminal menu. MCP legacy session negotiation is pinned to `2025-11-25`; the server returns this supported version when the client offers a different version. This small server does not yet implement the newer `2026-07-28` stateless discovery protocol. It requires `initialize` before serving tools.

## Tools

| Tool | Arguments | Result |
| --- | --- | --- |
| `tidyfs.list_scans` | optional `limit` (1–25) | Completed scan IDs and finish times |
| `tidyfs.list_candidates` | optional `scan_id` | Up to 100 existing eligible low-risk non-overlapping reversible candidates |
| `tidyfs.get_plan` | required `target_bytes`, optional `scan_id` | A bound `AiGoalRequest` with redacted paths and deterministic plan digest |
| `tidyfs.evaluate_selection` | `scan_id`, `target_bytes`, `plan_digest`, `selected_candidate_ids` | Fresh plan-digest and membership check; TidyFS-computed reclaim bytes and target status |

No tools create a scan or plan. Operators must explicitly run `tidyfs scan` and `tidyfs plan --safe` outside MCP first. The MCP server opens only the existing SQLite database with `SQLITE_OPEN_READ_ONLY` and does **not** migrate or create it. All tools are hard-limited to **low risk**, redacted path metadata, and 100 candidate IDs.

The plan digest binds the selected scan, all supplied candidate facts, redaction/constraint context and goal bytes. A call to `evaluate_selection` recomputes the current eligible candidate set and rejects stale, duplicated, or unknown IDs; results are advisory. It does not grant quarantine permission or guarantee that files still exist at their scanned sizes. Real cleanup requires the separate deterministic interactive execution path, including freshness and source-identity checks.

## Trust boundaries

- This server never reads arbitrary file contents, follows model-generated paths, invokes shell commands, or delegates to a model.
- The MCP client chooses whether to disclose the returned metadata to a local/remote model. Do not claim that local stdio automatically makes downstream reasoning local-only.
- The MCP tool catalog is advisory; **MCP tool discovery or invocation is not cleanup authorization**.
- The process runs with its caller's OS rights. Launch only through a trusted client and provide an intentional `--db` path. Never expose the stdio process through an unreviewed network bridge.
- Input framing is bounded to 64 KiB; a single result to 128 KiB; malformed inputs fail closed. Unknown tools are rejected.
- Paths are redacted using TidyFS's existing `AiPathMode::Redacted`. Some recognizable ecosystem structures may still be visible; choose model recipients accordingly.
- The service makes no durable model decisions and offers **no** cleanup, restore, recovery, scanning or arbitrary filesystem operations.
- Persisted plans are historical observations. New scans and fresh deterministic plans are required before taking action.

## Dubnium integration

1. Qualify this stdio tool server independently with a trusted local MCP client and synthetic SQLite fixtures.
2. Integrate into Dubnium's reviewed, explicit tool registry only when outbound MCP consumption (#1628) is implemented. Avoid an HTTP/stdio proxy merely to make this reachable at port 8080.
3. Supervisor Gateway at `127.0.0.1:8080/v1` remains the inference path with alias `supervisor`; it currently advertises `tools: false` and does not consume MCP tools itself.
4. Keep CLI-to-Supervisor inference work in TidyFS #103 separately scoped. This MCP server does not unblock #74's existing `eval-live` harness because that harness still expects `/v1/recommend`.

Neither merging the source nor installing the binary authorizes Dubnium host activation, network exposure or capability changes.
