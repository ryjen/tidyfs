# Recoverable actions

`tidyfs` records filesystem mutation intent in SQLite before moving a path. Cleanup and restore use explicit durable states so interrupted operations can be reconciled without deleting or overwriting user data.

## State model

```text
planned -> moving -> quarantined -> restoring -> restored
   |          |             |           |
   +----------+-------------+-----------+-> failed
```

- `planned`: the action row exists and the intended quarantine path has been recorded.
- `moving`: the quarantine manifest is written and the source rename is about to occur or may already have occurred.
- `quarantined`: the original path is absent and the verified quarantine payload exists.
- `restoring`: the reverse rename is about to occur or may already have occurred.
- `restored`: the verified payload exists at the original path and the quarantine payload is absent.
- `failed`: the observed state cannot be safely interpreted as a completed or retryable transition.

Legacy `running` rows are migrated to `moving` when the database is opened.

## Startup detection

When an existing database contains `planned`, `moving`, `restoring`, or legacy `running` actions, normal CLI startup performs a read-only inspection and warns that reconciliation is required. The warning includes the selected database path and the explicit recovery command.

Startup detection does **not** modify SQLite or the filesystem. Recovery remains an explicit operator action.

## Recovery command

Recover one interrupted action:

```bash
tidyfs recover --action 42
```

Recover every action in a transitional state:

```bash
tidyfs recover --all
```

Recovery is deliberately read-only with respect to the filesystem. It inspects the original and quarantine paths, verifies the recorded payload identity when available, and updates only SQLite action state.

`clean`, `restore`, and `recover` are serialized through a database-scoped advisory mutation lock. Read-only commands and dry-runs do not take the mutation lock.

## Payload identity

New quarantine actions store a deterministic `sha256-tree-v1` identity in both the database and per-action manifest.

The fingerprint covers files, directories, and symlinks without following symlinks. Directory entries are ordered deterministically before hashing.

Identity is verified:

1. before quarantine intent is committed;
2. after the source has been moved into quarantine;
3. before restore;
4. after restore; and
5. during interrupted-action recovery on whichever path represents the completed side of the transition.

An unsupported identity version or payload mismatch fails closed. Legacy action rows without a recorded identity cannot be automatically restored or reconciled by identity-aware recovery.

## Reconciliation matrix

For rows with a supported recorded identity, a path being `present` below also requires that payload to match the stored fingerprint before recovery advances the action.

### `planned` or `moving`

| Original | Quarantine | Result |
|---|---|---|
| absent | present | verify quarantine identity, then `quarantined` |
| present | absent | `failed`; move did not complete |
| present | present | `failed`; ambiguous, no path is changed |
| absent | absent | `failed`; payload location is unknown |

### `restoring`

| Original | Quarantine | Result |
|---|---|---|
| present | absent | verify original identity, then `restored` |
| absent | present | verify quarantine identity, then `quarantined`; restore did not complete |
| present | present | `failed`; ambiguous, no path is changed |
| absent | absent | `failed`; payload location is unknown |

Recovery never resolves an ambiguous state by deleting, copying, or overwriting either path.

## Cleanup transition

A real cleanup requires both `--safe` and `--interactive`.

Before durable action creation, `tidyfs` revalidates the candidate against the scan record:

- the path must still exist;
- the candidate must still be a non-symlink low-risk reversible action;
- its current device and inode must match the values recorded by the scan; and
- the source and quarantine destination must resolve to the same filesystem.

If any of those checks fail, cleanup refuses the candidate before mutation. A changed inode/device requires a rescan.

Once preflight succeeds:

1. Fingerprint the source payload.
2. Insert a `planned` action with the fingerprint.
3. Persist the intended quarantine path.
4. Create the per-action quarantine directory and manifest.
5. Persist `moving`.
6. Rename the source into quarantine.
7. Verify the quarantined payload identity.
8. Persist `quarantined`.

A crash or database failure after step 6 can leave `moving` while the payload already exists in quarantine. Startup detection surfaces the interruption and `recover` verifies the quarantine payload before advancing the row to `quarantined`.

Ordinary failures before the rename leave the source untouched and record a terminal `failed` action when an action row already exists. Permission failures while preparing the quarantine directory are covered by integration tests.

## Restore transition

Restore requires an action in `quarantined` state and refuses an already-present original destination.

1. Verify the quarantine payload identity.
2. Persist `restoring`.
3. Create missing parent directories.
4. Atomically rename the quarantine payload to the original path without replacement.
5. Verify the restored payload identity.
6. Persist `restored` and `restored_at`.

On Linux, step 4 uses `renameat2(RENAME_NOREPLACE)`. If another process creates the destination after preflight, the kernel rejects the rename and preserves both paths. The action remains retryable as `quarantined` with the restore error recorded.

Platforms without an equivalent atomic no-replace primitive fail closed rather than using an overwrite-prone fallback.

A crash or database failure after step 4 can leave `restoring` while the payload already exists at the original path. Startup detection surfaces the interruption and `recover` verifies the restored payload before advancing the row to `restored`.

A post-restore identity mismatch is terminal `failed`, because the filesystem has already moved and the observed payload cannot be trusted as the recorded quarantine object.

## Filesystem boundaries

Quarantine intentionally does not implement cross-filesystem copy-and-delete semantics.

Before creating durable action intent, `tidyfs` compares the source device with the nearest existing ancestor of the quarantine destination. A cross-filesystem candidate is rejected before mutation.

The later rename remains authoritative: if mount layout changes between preflight and rename, the operating system can still reject the rename with `EXDEV`. `tidyfs` does not fall back to copying and deleting.

This keeps the safety model based on atomic same-filesystem renames rather than introducing a second partially-completable mutation protocol.

## Threat model

The recovery model protects against:

- process termination between a filesystem rename and its success update;
- machine restart or power loss during cleanup or restore transitions;
- SQLite write failure after a successful filesystem rename;
- ordinary rename and permission failures;
- stale or replaced source paths between scan and cleanup;
- symlink candidates and symlink substitution at the selected cleanup path;
- concurrent cooperating `tidyfs` mutation commands through a database-scoped advisory lock;
- external writers racing restore destination creation on Linux through atomic no-replace rename;
- quarantined or restored payload substitution detected through deterministic SHA-256 identity verification;
- accidental cross-filesystem quarantine attempts, which are rejected rather than converted into copy/delete operations; and
- ambiguous path states, which are never resolved by deleting or overwriting either path.

The model assumes:

- the SQLite database and quarantine metadata have not been maliciously rewritten together by an attacker with equivalent user privileges;
- the operating system provides the documented filesystem and locking semantics; and
- operators do not manually move or delete recovery paths while an action is being reconciled.

## Current limitations

- Restore's atomic no-overwrite implementation is Linux-only; unsupported platforms fail closed.
- Cross-filesystem quarantine and restore are intentionally unsupported rather than implemented as copy/delete.
- Recovery updates durable state but does not automatically move files back to a preferred side of an ambiguous transition.
- Legacy actions without payload identity metadata require manual inspection rather than identity-aware automatic recovery.
- Startup performs detection and warning only; it does not automatically reconcile actions.

## Operator recovery procedure

1. Stop other `tidyfs clean`, `restore`, or `recover` processes.
2. Run `tidyfs actions` and identify `planned`, `moving`, or `restoring` rows. Startup will also warn if interrupted rows exist.
3. Run `tidyfs recover --all` using the same `--db` selection if a non-default database is in use.
4. Run `tidyfs actions` again and review the reconciled states.
5. For `quarantined` rows, retry `restore` only after confirming the original destination remains absent.
6. For `failed` rows, inspect both recorded paths and the stored error before taking manual action.
7. Never manually delete or overwrite one side of an ambiguous pair until its identity and provenance are understood.

Recovery itself does not write to the filesystem and never overwrites user data.
