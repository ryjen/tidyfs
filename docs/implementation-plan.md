# Implementation Plan

The implementation should proceed vertically, with a working deterministic core before adapters or AI.

## Milestone 1: Analyzer spine

Goal: replace basic `du` for common developer-machine inspection.

Build:

- Rust CLI scaffold
- `scan` command
- SQLite index
- directory aggregation
- `top` command
- permission error handling

Commands:

```bash
tidyfs scan ~
tidyfs top
tidyfs top --depth 3
```

Success criteria:

- can scan `$HOME`
- can tolerate permission errors
- can show largest directories
- does not delete anything
- stores repeatable scan metadata

## Milestone 2: Classification

Goal: identify known filesystem categories.

Build:

- path classifier
- classification table
- `explain` command
- protected path detection

Initial labels:

```text
cache
thumbnail_cache
browser_cache
browser_profile
trash
node_cache
node_dependencies
python_cache
rust_cache
rust_build_artifacts
go_cache
gradle_cache
maven_cache
docker_data
podman_data
nix_store
systemd_journal
source_repo
git_repo
database
vm_image
secret_material
unknown
```

Commands:

```bash
tidyfs explain ~/.cache
tidyfs explain ~/src/foo/node_modules
```

## Milestone 3: Rule engine and planner

Goal: generate cleanup proposals.

Build:

- YAML rule loader
- path/glob matchers
- risk tiers
- policy config
- `plan` command
- blocked candidate reporting

Commands:

```bash
tidyfs plan --safe
tidyfs plan --risk medium
```

Initial rules:

- old trash
- old thumbnails
- old generic cache entries
- old pip cache
- old npm cache
- inactive `node_modules` with lockfile
- inactive Rust `target`
- report-only Docker data
- report-only Nix store opportunity
- report-only large DB/VM files

## Milestone 4: Dry-run cleaner

Goal: preview cleanup without touching files.

Build:

- `clean --dry-run`
- exact action preview
- candidate selection
- audit preview

Command:

```bash
tidyfs clean --dry-run
```

## Milestone 5: Reversible execution

Goal: safe cleanup with undo.

Build:

- OS trash support
- `tidyfs` quarantine
- manifest generation
- audit log
- restore command
- interactive approval

Commands:

```bash
tidyfs clean --safe --interactive
tidyfs restore
```

No permanent deletion.

## Milestone 6: Tool adapters

Goal: summarize and propose tool-native cleanup.

Initial adapters:

- systemd journal
- Docker
- Podman
- Nix
- pnpm
- pip
- uv
- Go
- Cargo report-only

Commands:

```bash
tidyfs adapters
tidyfs plan --include-adapters
```

## Milestone 7: Optional AI explainer

Goal: explain validated plans and answer scan questions.

Build:

- Ollama/OpenAI-compatible client abstraction
- redaction layer
- structured AI input
- structured AI output validation
- `ask` command

Commands:

```bash
tidyfs ask "why is my home directory so large?"
tidyfs ask "what can I clean safely to reclaim 20GB?"
tidyfs explain ~/.cache --ai
```

## Suggested initial Rust dependencies

```toml
[dependencies]
clap = { version = "4", features = ["derive"] }
jwalk = "0.8"
walkdir = "2"
ignore = "0.4"
globset = "0.4"
rusqlite = { version = "0.32", features = ["bundled"] }
serde = { version = "1", features = ["derive"] }
serde_yaml = "0.9"
serde_json = "1"
anyhow = "1"
thiserror = "1"
tracing = "0.1"
tracing-subscriber = "0.3"
humansize = "2"
humantime = "2"
dirs = "5"
trash = "5"
inquire = "0.7"
blake3 = "1"
```

## First vertical slice

Implement only this first:

```text
scan -> SQLite -> top -> classify -> plan --safe
```

No adapters, no AI, no delete.

That gives the project a stable spine before adding complexity.
