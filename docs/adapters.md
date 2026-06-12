# Adapters

Adapters let `tidyfs` inspect and propose tool-native cleanup without manually deleting internal tool data.

## Adapter contract

```rust
trait Adapter {
    fn name(&self) -> &'static str;
    fn detect(&self) -> bool;
    fn inspect(&self) -> Result<AdapterReport>;
    fn plan(&self, policy: &Policy) -> Result<Vec<CleanupCandidate>>;
    fn execute(&self, candidate: &CleanupCandidate) -> Result<ActionResult>;
}
```

## Adapter principles

- Use allowlisted commands only.
- Prefer preview/inspection before cleanup.
- Treat destructive flags as higher risk.
- Never allow AI-generated commands.
- Convert adapter findings into normal cleanup candidates.
- Run policy validation before execution.
- Write audit logs.

## Nix adapter

### Inspect

```bash
nix-store --gc --print-dead
nix-store --query --roots
```

### Cleanup

```bash
nix-collect-garbage --delete-older-than 30d
```

### Safety rules

Never manually delete `/nix/store`.

Risk: medium.

Rationale: Nix cleanup is safe when live roots are respected by Nix tooling, but dangerous if paths are removed directly.

## Docker adapter

### Inspect

```bash
docker system df
```

### Cleanup

```bash
docker system prune
docker builder prune
```

### High-risk cleanup

```bash
docker system prune --volumes
```

Volumes should be high-risk and disabled by default.

Risk: medium/high.

## Podman adapter

### Inspect

```bash
podman system df
```

### Cleanup

```bash
podman system prune
```

Risk: medium.

## systemd journal adapter

### Inspect

```bash
journalctl --disk-usage
```

### Cleanup

```bash
journalctl --vacuum-time=30d
journalctl --vacuum-size=1G
```

Risk: low/medium.

## Node adapters

### npm

```bash
npm cache verify
npm cache clean --force
```

Risk: low/medium.

### pnpm

```bash
pnpm store status
pnpm store prune
```

Risk: low/medium.

### yarn

```bash
yarn cache clean
```

Risk: low/medium.

## Python adapters

### pip

```bash
pip cache info
pip cache purge
```

Risk: low/medium.

### uv

```bash
uv cache dir
uv cache clean
```

Risk: low/medium.

## Go adapter

### Inspect

```bash
go env GOCACHE
go env GOMODCACHE
```

### Cleanup

```bash
go clean -cache
go clean -testcache
go clean -modcache
```

Risk:

- build/test cache: low
- module cache: medium

## Rust adapter

Prefer report-only first.

Potential external tool:

```bash
cargo-cache
```

Project-local `target/` directories can be classified by filesystem rules.

Risk:

- `target/`: medium
- cargo registry/git cache: low/medium

## Adapter candidate format

Adapters should emit normal cleanup candidates:

```json
{
  "source": "adapter",
  "adapter": "docker",
  "rule_id": "docker-builder-prune",
  "risk": "medium",
  "action_type": "tool_native",
  "preview_command": ["docker", "system", "df"],
  "action_command": ["docker", "builder", "prune"],
  "reason": "Docker build cache can often be regenerated, but future builds may be slower."
}
```

## Execution guardrails

Before execution:

- verify command is allowlisted
- verify adapter is still detected
- run preview if configured
- require approval for medium+ risk
- record stdout/stderr summary
- write audit log
