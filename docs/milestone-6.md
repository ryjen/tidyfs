# Milestone 6: Tool-native Adapters

## Goal

Add read-only adapter inspection and cleanup planning for tool-owned data.

Adapters let `tidyfs` reason about systems that should not be cleaned by raw file deletion:

- systemd journal
- Docker
- Podman
- Nix
- pnpm
- pip
- uv
- Go

## Commands

Inspect adapters:

```bash
tidyfs adapters
```

Include adapter-generated cleanup candidates in a plan:

```bash
tidyfs plan --safe --include-adapters
tidyfs plan --risk medium --include-adapters
```

Preview adapter candidates:

```bash
tidyfs clean --dry-run --risk medium
```

## Safety boundary

Milestone 6 does **not** execute adapter cleanup commands.

Adapters only:

- detect whether a tool exists
- run allowlisted preview commands
- generate `tool_native` cleanup candidates
- include suggested cleanup commands in the reason text

No arbitrary shell is used. Commands are invoked through explicit argv arrays.

## Adapter candidates

Adapter-generated candidates use:

```text
action_type = tool_native
```

They are not executable by Milestone 5 quarantine execution.

`clean --safe --interactive` still only executes reversible quarantine/trash candidates.

## Example candidates

Docker:

```text
Rule: adapter-docker-system-prune
Risk: medium
Action: tool_native
Reason: Docker reports reclaimable data. Suggested command: docker system prune
```

Nix:

```text
Rule: adapter-nix-gc-30d
Risk: medium
Action: tool_native
Reason: Nix garbage collection should use nix-collect-garbage --delete-older-than 30d.
```

## Next milestone

Milestone 7 should add optional AI explanations over existing scan summaries and validated plans.
