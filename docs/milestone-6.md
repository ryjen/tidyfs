# Milestone 6: Tool-native Adapters

## Status

**Complete.**

## Goal

Add read-only adapter inspection and cleanup planning for tool-owned data without granting `tidyfs` command-execution authority over those tools.

Adapters let `tidyfs` reason about systems that should not be cleaned by raw filesystem mutation:

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

- detect whether a tool exists;
- run allowlisted preview commands using explicit argv arrays;
- generate `tool_native` cleanup candidates;
- include suggested native cleanup commands as explanatory text.

No arbitrary shell is used. Model output cannot generate executable adapter commands.

## Adapter candidates

Adapter-generated candidates use:

```text
action_type = tool_native
```

They are not executable by the quarantine executor. AI planning enrichment also cannot promote `tool_native` or `report_only` candidates into raw filesystem mutation.

`clean --safe --interactive` continues to execute only reversible deterministic quarantine candidates that satisfy every policy, risk, identity, and approval gate.

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

## Subsequent delivered work

The earlier version of this document said the next milestone should be an optional AI explainer. That roadmap is now historical.

After Milestone 6, `tidyfs` delivered:

- parallel subtree scanning;
- package and tag-driven Linux release automation;
- the validated `v0.6.1` release;
- versioned AI proposal and transport contracts;
- a numeric-loopback local/supervisor gateway;
- read-only `tidyfs analyze`;
- observation-digest freshness binding;
- conservative AI enrichment of already-eligible deterministic quarantine candidates;
- persisted advisory AI evidence and `explain` freshness reporting.

See `implementation-plan.md`, `ai-architecture.md`, and `threat-model.md` for the current architecture.

## Next decision

The post-`v0.6.1` product milestone is tracked in QART #51. Tool-native **execution** remains intentionally deferred because it would introduce a new external-command mutation and recovery boundary distinct from reversible filesystem quarantine.