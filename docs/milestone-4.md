# Milestone 4: Dry-run Cleaner

## Goal

Add a non-mutating executor preview:

```text
cleanup_candidates -> clean --dry-run -> exact action preview
```

Milestone 4 still performs no filesystem mutation.

## Commands

```bash
tidyfs plan --safe
tidyfs clean --dry-run
tidyfs clean --dry-run --risk medium
tidyfs clean --dry-run --root ~/.cache
tidyfs clean --dry-run --limit 25
```

## Design boundary

`clean --dry-run` consumes **allowed cleanup candidates** from the most recent plan.

It does not:

- delete files
- move files
- call adapters
- run shell commands
- use AI
- alter candidate risk

## Required flow

Recommended usage:

```bash
tidyfs scan ~
tidyfs plan --safe
tidyfs clean --dry-run
```

If no candidates exist, run `tidyfs plan --safe` first.

## Safety behavior

Dry-run output includes:

- candidate id
- path
- size
- rule id
- risk
- action type
- reversibility
- reason

`report_only` actions are shown as informational actions. Real deletion/quarantine/trash support is intentionally deferred.

## Next milestone

Milestone 5 should add reversible execution:

```bash
tidyfs clean --safe --interactive
tidyfs restore
```

That milestone should still avoid permanent deletion.
