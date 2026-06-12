# Milestone 1: Analyzer Spine

## Goal

Build the first useful vertical slice:

```text
scan -> SQLite -> aggregate directory totals -> top report
```

## Commands

```bash
tidyfs scan ~
tidyfs top
tidyfs top --depth 2
tidyfs top --root ~/.cache
```

## Data model

Milestone 1 creates these tables:

- `scans`
- `entries`
- `directory_totals`
- `scan_errors`

## Design choices

### Read-only scanner

The scanner only records metadata. It does not inspect file contents.

### Symlinks

Symlinks are recorded but not followed.

### Allocated size

On Unix, allocated size is estimated from `metadata.blocks() * 512`.
On non-Unix platforms, allocated size falls back to logical length.

### Aggregation

Directory totals are calculated in memory during the scan and persisted at the end of the transaction.

### Errors

Permission and traversal errors are written to `scan_errors` and do not abort the whole scan.

## Next milestone

Milestone 2 should add deterministic classification:

```text
entries -> classifications -> explain
```
