# tidyfs

`tidyfs` is a conservative disk-usage intelligence CLI.

Milestone 4 implements a read-only cleanup preview:

```bash
tidyfs scan ~
tidyfs top --depth 2
tidyfs classify --summary
tidyfs explain ~/.cache
tidyfs plan --safe
tidyfs clean --dry-run
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
- YAML cleanup rules
- policy validation
- cleanup candidate persistence
- `plan` command
- blocked-candidate reporting
- `clean --dry-run`
- no filesystem mutation
- no AI
- no adapters

Not implemented yet:

- actual cleanup
- quarantine/restore
- adapter execution
- AI explanations

## Install / run

```bash
cargo run -- scan ~
cargo run -- top --depth 2 --limit 20
cargo run -- classify --summary
cargo run -- explain ~/.cache --children
cargo run -- plan --safe
cargo run -- clean --dry-run
```

By default, the SQLite DB is stored at:

```text
~/.local/share/tidyfs/tidyfs.db
```

Override with:

```bash
tidyfs --db ./tidyfs.db scan ~
tidyfs --db ./tidyfs.db plan --safe
tidyfs --db ./tidyfs.db clean --dry-run
```

## Safety posture

Milestone 4 is still read-only. It creates cleanup candidates and previews actions, but does not execute anything.

It does not:

- delete files
- move files
- follow symlinks
- run cleanup commands
- call AI providers
- inspect file contents

## Planning model

```text
scan facts
-> deterministic classifications
-> YAML cleanup rules
-> policy validation
-> cleanup candidates / blocked candidates
-> report
-> dry-run preview
```

Classification answers:

```text
What does this path appear to be?
```

Planning answers:

```text
Could this be proposed for cleanup under the selected risk threshold?
```

Dry-run answers:

```text
What exact actions would be attempted later?
```

## Risk tiers

| Risk | Meaning | Default behavior |
|---|---|---|
| low | known regenerable cache/temp data | shown by `plan --safe` |
| medium | regenerable but project/tool-impacting | requires `--risk medium` |
| high | risky user/application data | blocked/report-only |
| forbidden | secrets, DBs, VMs, git metadata, browser profiles | blocked |

## Example

```bash
tidyfs plan --safe
tidyfs clean --dry-run
```

Output shape:

```text
Dry-run cleanup preview

Would process:
  3.2 GiB  ~/.cache/pip
           Candidate: 42
           Rule: python-pip-cache-old
           Risk: low
           Action: report_only
           Reversible: yes
           Reason: Python package cache is normally regenerable.
```
