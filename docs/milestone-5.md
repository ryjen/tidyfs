# Milestone 5: Reversible Execution

## Goal

Add a reversible execution path:

```text
allowed cleanup_candidates
-> interactive approval
-> quarantine move
-> action log
-> restore
```

Milestone 5 still avoids permanent deletion and adapter execution.

## Commands

```bash
tidyfs plan --safe
tidyfs clean --safe --interactive
tidyfs actions
tidyfs restore --action 12
tidyfs restore --latest
```

Dry-run remains available:

```bash
tidyfs clean --dry-run
```

## Safety behavior

Real execution requires:

```bash
tidyfs clean --safe --interactive
```

The command refuses to execute unless:

- `--safe` is provided
- `--interactive` is provided
- selected candidates are not blocked
- selected candidates are within the selected risk threshold
- selected candidates use a reversible action
- the path still exists
- the path is not a symlink
- the path is still inside the scanned root

## Quarantine layout

```text
~/.local/share/tidyfs/quarantine/
  action-42/
    manifest.txt
    payload
```

Each action moves one original path into its own quarantine directory.

## Restore

Restore by action id:

```bash
tidyfs restore --action 42
```

Restore latest successful quarantine action:

```bash
tidyfs restore --latest
```

Restore is conservative. It refuses to overwrite an existing destination.

## Still not implemented

- permanent deletion
- purge
- adapter execution
- AI
- batch restore UI
- cross-filesystem copy fallback

## Next milestone

Milestone 6 should add tool-native adapters:

- systemd journal
- Docker
- Podman
- Nix
- pnpm/pip/uv/go report/cleanup adapters
