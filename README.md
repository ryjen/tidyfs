# tidyfs

`tidyfs` is a conservative disk-usage intelligence CLI.

Milestone 1 implements a read-only analyzer:

```bash
tidyfs scan ~
tidyfs top
tidyfs top --depth 2
tidyfs top --root ~/.cache --limit 25
```

The project goal is to build toward a safe cleanup planner, not an autonomous file deleter.

## Milestone 1 scope

Implemented:

- Rust CLI
- recursive filesystem scan
- SQLite index
- directory aggregation
- latest-scan lookup
- `top` reporting
- permission-error tolerance
- no deletion
- no AI
- no adapters

Not implemented yet:

- classification
- cleanup rules
- dry-run cleanup
- quarantine/restore
- adapters
- AI explanations

## Install / run

```bash
cargo run -- scan ~
cargo run -- top
cargo run -- top --depth 2 --limit 20
```

By default, the SQLite DB is stored at:

```text
~/.local/share/tidyfs/tidyfs.db
```

Override with:

```bash
tidyfs --db ./tidyfs.db scan ~
tidyfs --db ./tidyfs.db top
```

## Safety posture

Milestone 1 is read-only. It only scans metadata and writes to its own SQLite DB.

It does not:

- delete files
- move files
- follow symlinks
- run cleanup commands
- call AI providers
- inspect file contents
