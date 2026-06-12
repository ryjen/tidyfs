# Reuse Strategy

`tidyfs` should reuse aggressively, but not by making another cleaner its core.

The strategy is:

```text
reuse mechanics
reuse knowledge
reuse tool-native cleanup commands
own the safety boundary
```

## Reuse layers

### 1. Core libraries

Safe to link directly:

| Area | Reuse |
|---|---|
| CLI | `clap` |
| traversal | `jwalk`, `walkdir`, `ignore` |
| matching | `globset` |
| storage | `rusqlite`, SQLite |
| config | `serde`, `serde_yaml`, `toml` |
| logging | `tracing` |
| errors | `anyhow`, `thiserror` |
| trash | `trash` |
| hashing | `blake3` |

### 2. External adapters

Use existing tools through allowlisted subprocesses:

- `docker`
- `podman`
- `nix`
- `journalctl`
- `pnpm`
- `pip`
- `uv`
- `go`
- `cargo-cache`
- `czkawka` later

Adapters produce reports or cleanup candidates. They do not bypass policy.

### 3. Knowledge imports

BleachBit CleanerML can be used as source material, but not executed directly.

Flow:

```text
CleanerML -> importer -> tidyfs draft rule -> manual review -> enabled rule
```

Imported rules should start as:

```yaml
enabled: false
status: unreviewed
```

### 4. Reference implementations

Useful projects to study:

- `dua-cli` for Rust disk scanning and terminal UX
- `dust` for concise disk usage presentation
- `ncdu` for remote/headless UX
- `gdu` for Go-based high-performance scanning
- `czkawka` for duplicates, empty files, large files, and temp-file detection
- BleachBit for cleaner-domain knowledge

Reference these for ideas, but do not inherit their deletion semantics.

## What tidyfs should own

`tidyfs` should own:

- risk model
- policy model
- cleanup candidate schema
- adapter allowlists
- audit log
- quarantine/restore model
- AI trust boundary

These are the core product differentiators.

## Czkawka integration

Czkawka is useful for duplicate detection and similar-media detection.

Recommended path:

1. Add as optional report-only adapter.
2. Convert duplicate groups into reports, not cleanup candidates.
3. Require explicit user selection before any future cleanup.
4. Use quarantine only.

Duplicate deletion should never be automatic by default.

## BleachBit integration

BleachBit CleanerML contains valuable cleanup knowledge.

Use it to generate reviewed `tidyfs` rules.

Do not directly execute upstream cleaner definitions because:

- product goals differ
- risk model differs
- reversible cleanup semantics differ
- policy enforcement must remain local
- auditability must be consistent

## Adapter command allowlists

Never run arbitrary shell commands.

Good:

```yaml
adapter: docker
allowed_preview:
  - ["docker", "system", "df"]
allowed_actions:
  - ["docker", "system", "prune"]
  - ["docker", "builder", "prune"]
forbidden:
  - ["docker", "system", "prune", "--volumes"]
default_risk: medium
```

Bad:

```text
LLM suggests command -> shell executes it
```

## Recommended first reuse set

Start with:

```text
jwalk
walkdir
ignore
globset
rusqlite
serde_yaml
trash
inquire
tracing
```

Add external adapters only after the deterministic planner works.
