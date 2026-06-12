# tidyfs

`tidyfs` is a conservative disk-usage intelligence CLI.

Milestone 2 implements:

```bash
tidyfs scan ~
tidyfs top
tidyfs classify
tidyfs explain ~/.cache
tidyfs explain ~/src/project/node_modules
```

The project goal is to build toward a safe cleanup planner, not an autonomous file deleter.

## Current scope

Implemented:

- Rust CLI
- recursive filesystem scan
- SQLite index
- directory aggregation
- deterministic classification
- `explain` command
- latest-scan lookup
- permission-error tolerance
- no deletion
- no AI
- no adapters

Not implemented yet:

- cleanup rules
- cleanup candidates
- dry-run cleanup
- quarantine/restore
- adapters
- AI explanations

## Install / run

```bash
cargo run -- scan ~
cargo run -- top --depth 2 --limit 20
cargo run -- explain ~/.cache
cargo run -- classify --summary
```

By default, the SQLite DB is stored at:

```text
~/.local/share/tidyfs/tidyfs.db
```

Override with:

```bash
tidyfs --db ./tidyfs.db scan ~
tidyfs --db ./tidyfs.db explain ~/.cache
```

## Safety posture

Milestone 2 is read-only. It only scans metadata, classifies known path patterns, and writes to its own SQLite DB.

It does not:

- delete files
- move files
- follow symlinks
- run cleanup commands
- call AI providers
- inspect file contents

## Classification model

Classification answers:

```text
What does this path appear to be?
```

It does not answer:

```text
Should this be deleted?
```

Initial labels include:

- `cache`
- `thumbnail_cache`
- `browser_cache`
- `browser_profile`
- `trash`
- `node_cache`
- `node_dependencies`
- `node_build_artifacts`
- `python_cache`
- `python_virtualenv`
- `python_bytecode_cache`
- `rust_cache`
- `rust_build_artifacts`
- `go_cache`
- `gradle_cache`
- `maven_cache`
- `docker_data`
- `podman_data`
- `nix_store`
- `systemd_journal`
- `source_repo`
- `git_repo`
- `database`
- `vm_image`
- `secret_material`
- `unknown`

Protected/sensitive labels like `secret_material`, `database`, `vm_image`, `browser_profile`, and `git_repo` are intended to be blocked by later policy/planning milestones.
