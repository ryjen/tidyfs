# Recoverable actions

`tidyfs` records filesystem mutation intent in SQLite before moving a path. Cleanup and restore use explicit durable states so an interrupted process can be reconciled without overwriting or deleting data.

## State model

```text
planned -> moving -> quarantined -> restoring -> restored
   |          |             |           |
   +----------+-------------+-----------+-> failed
```

- `planned`: the action row exists and the intended quarantine path has been recorded.
- `moving`: the manifest is written and the quarantine rename is about to occur or may have occurred.
- `quarantined`: the original path is absent and the quarantine payload exists.
- `restoring`: the reverse rename is about to occur or may have occurred.
- `restored`: the original path exists and the quarantine payload is absent.
- `failed`: observed filesystem state cannot be safely interpreted as a completed transition.

Legacy `running` rows are migrated to `moving` when the database is opened.

## Recovery command

Recover one interrupted action:

```bash
tidyfs recover --action 42
```

Recover every action in a transitional state:

```bash
tidyfs recover --all
```

Recovery is deliberately read-only with respect to the filesystem. It inspects the original and quarantine paths and updates only the SQLite action state.

## Reconciliation matrix

### `planned` or `moving`

| Original | Quarantine | Result |
|---|---|---|
| absent | present | `quarantined` |
| present | absent | `failed`; move did not complete |
| present | present | `failed`; ambiguous, no path is changed |
| absent | absent | `failed`; payload location is unknown |

### `restoring`

| Original | Quarantine | Result |
|---|---|---|
| present | absent | `restored` |
| absent | present | `quarantined`; restore did not complete |
| present | present | `failed`; ambiguous, no path is changed |
| absent | absent | `failed`; payload location is unknown |

## Failure windows

### Cleanup

1. Insert `planned` action row.
2. Persist the intended quarantine path.
3. Write the per-action manifest.
4. Persist `moving`.
5. Rename the source into quarantine.
6. Persist `quarantined`.

A crash after step 5 leaves `moving` with the payload in quarantine. `recover` deterministically advances it to `quarantined`.

### Restore

1. Validate that the quarantine payload exists and the destination is absent.
2. Persist `restoring`.
3. Create missing parent directories.
4. Rename the quarantine payload to the original path.
5. Persist `restored` and `restored_at`.

A crash after step 4 leaves `restoring` with the payload at the original path. `recover` deterministically advances it to `restored`.

## Threat model

The recovery model protects against:

- process termination between a filesystem rename and its success update;
- machine restart or power loss during a cleanup or restore transition;
- database write failure after a successful filesystem move;
- ordinary rename failures, which are recorded as terminal cleanup failures or restorable restore failures;
- ambiguous path states, which are never resolved by deleting or overwriting either path.

The current slice does **not** yet provide:

- a process-wide mutation lock;
- atomic no-replace rename semantics against an external writer racing restore;
- automatic recovery on startup;
- recovery across cross-filesystem copy-and-remove operations;
- cryptographic payload identity verification.

Until those controls are implemented, use `recover --all` after an interrupted mutation and inspect `tidyfs actions` before retrying cleanup or restore.

## Operator procedure

1. Stop other `tidyfs clean`, `restore`, or `recover` processes.
2. Run `tidyfs actions` and identify `planned`, `moving`, or `restoring` rows.
3. Run `tidyfs recover --all`.
4. Review any `failed` rows and inspect both recorded paths manually.
5. Never manually delete one side of an ambiguous pair until its contents and provenance are understood.
