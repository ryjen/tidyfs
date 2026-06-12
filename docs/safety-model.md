# Safety Model

`tidyfs` should be safe before it is smart.

The safety model is built around conservative defaults, explicit risk tiers, reversible actions, and a strict AI trust boundary.

## Core rule

```text
AI is the analyst.
Policy is the authority.
Executor is the actor.
```

## Risk tiers

| Tier | Name | Examples | Default behavior |
|---:|---|---|---|
| 0 | Observe only | unknown directories, source repos, app data | report only |
| 1 | Low risk | old thumbnails, old temp files, known regenerable caches | dry-run / reversible cleanup |
| 2 | Medium risk | `node_modules`, Docker build cache, Rust `target`, package stores | interactive approval |
| 3 | High risk | databases, VM images, browser profiles, cloud sync data | report only |
| 4 | Forbidden | secrets, keys, password stores, `.git`, `.env` | never propose cleanup |

## Forbidden by default

Never clean these without explicit future opt-in support:

```text
~/.ssh
~/.gnupg
~/.password-store
~/.local/share/keyrings
~/.config/1Password
**/.git
**/.env
**/*.sqlite
**/*.db
**/*.vdi
**/*.vmdk
**/*.qcow2
browser profiles
mail stores
password manager data
wallets
source files
unknown user documents
```

## Cleanup actions

### MVP

Only dry-run.

```bash
tidyfs clean --dry-run
```

### Later

Reversible only:

```bash
tidyfs clean --safe --interactive
```

Actions:

- move to OS trash
- move to `tidyfs` quarantine
- run allowlisted tool-native cleanup command

No permanent delete in the MVP.

## Quarantine design

Quarantine path:

```text
~/.local/share/tidyfs/quarantine/
  2026-06-12T22-10-00Z/
    manifest.json
    files/
```

Manifest:

```json
{
  "created_at": "2026-06-12T22:10:00Z",
  "items": [
    {
      "original_path": "/home/rj/.cache/thumbnails",
      "quarantine_path": "...",
      "size_bytes": 3812000000,
      "rule_id": "linux-thumbnail-cache-old",
      "risk": "low"
    }
  ]
}
```

Restore:

```bash
tidyfs restore
tidyfs restore --action 42
```

## Policy validation

Every cleanup candidate must pass policy validation.

Validation checks:

- path is inside scan root
- path is not forbidden
- risk is within selected threshold
- action type is allowed
- action is reversible unless explicitly requested
- adapter command is allowlisted
- path still exists before execution
- path metadata has not changed unexpectedly since scan

## Path freshness

Before execution, re-stat every candidate.

Block execution if:

- path disappeared
- inode changed unexpectedly
- path became a symlink
- path moved outside allowed root
- size changed beyond configured tolerance
- ownership/permissions changed suspiciously

## Symlink policy

Default:

```text
record symlink
do not follow
do not clean symlink targets
```

Symlinks can expand scope unexpectedly and must not be followed during cleanup unless explicitly supported later.

## Mount policy

Default:

```text
do not cross filesystem boundaries unless requested
```

This avoids accidentally scanning or cleaning mounted drives, network shares, container mounts, or external disks.

## Adapter safety

Adapters must use allowlisted commands.

Bad:

```text
LLM generated command -> run shell
```

Good:

```text
known adapter -> known preview command -> known cleanup command -> policy -> approval -> execution
```

## AI safety boundary

AI can:

- explain
- rank
- summarize
- draft disabled rules
- suggest report-only classifications

AI cannot:

- create active cleanup candidates directly
- invent executable commands
- invent paths
- lower risk
- override blocked candidates
- execute actions

Structured AI output must refer to existing candidate IDs.

Example valid AI output:

```json
{
  "recommended_order": [
    {
      "candidate_id": 42,
      "reason": "Low risk and high reclaim value"
    }
  ],
  "warnings": [
    "Docker cleanup may slow future builds."
  ]
}
```

Invalid output:

```json
{
  "delete": ["~/Documents/old"]
}
```

## Audit log

Every action gets an audit record.

```json
{
  "timestamp": "2026-06-12T22:10:00Z",
  "action": "quarantine",
  "path": "/home/rj/.cache/thumbnails",
  "bytes": 3812000000,
  "rule_id": "linux-thumbnail-cache-old",
  "risk": "low",
  "status": "success",
  "restore_path": "..."
}
```

## Conservative defaults

Default config should be approximately:

```yaml
policy:
  default_max_risk: low
  require_interactive_for:
    - medium
    - high
  prefer_reversible: true
  permanent_delete: false
  follow_symlinks: false
  cross_filesystems: false
  ai_can_create_candidates: false
```
