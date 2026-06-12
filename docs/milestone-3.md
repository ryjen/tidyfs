# Milestone 3: Rules and Planning

## Goal

Add cleanup planning without cleanup execution.

```text
entries + classifications + directory totals
-> rules
-> policy
-> cleanup_candidates
-> plan report
```

## Commands

```bash
tidyfs plan --safe
tidyfs plan --risk medium
tidyfs plan --root ~/.cache
tidyfs plan --include-blocked
```

## Scope

Milestone 3 is read-only. It does not move or delete files.

## Tables added

```sql
cleanup_candidates
```

Candidates include both allowed and blocked findings.

## Rule model

Rules live in YAML:

```yaml
- id: python-pip-cache-old
  label: Python pip cache older than 30 days
  category: python_cache
  risk: low
  action_type: report_only
  reversible: true
  match:
    labels_any: ["python_cache"]
    path_contains_any: ["/.cache/pip"]
    older_than_days: 30
  reason: >
    Python package cache is normally regenerable, but future installs may be
    slower or require network access.
```

## Policy model

Default policy blocks:

- secrets
- Git metadata
- browser profiles
- databases
- VM images
- Nix store raw deletion
- Docker/Podman raw deletion
- systemd journal raw deletion

Tool-owned stores are report-only until adapters are added.

## Next milestone

Milestone 4 should add:

```text
clean --dry-run
```

The dry-run should consume only already-generated, allowed cleanup candidates.
