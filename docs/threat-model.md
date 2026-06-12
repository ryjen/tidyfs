# Threat Model

`tidyfs` is a filesystem tool with potential destructive capability. Its primary risks are data loss, privacy leakage, command injection, and misplaced trust in AI.

## Assets

Assets to protect:

- user documents
- source code
- Git history
- secrets and keys
- password stores
- browser profiles
- local databases
- VM/container volumes
- cloud sync folders
- audit logs
- cleanup manifests
- private filenames and project names

## Trust boundaries

```text
filesystem -> scanner -> SQLite facts
rules -> planner -> cleanup candidates
policy -> validation
AI -> explanation only
user approval -> executor
executor -> filesystem mutation
```

The highest-risk boundary is executor access to the filesystem.

## Main threats

### 1. Accidental data loss

Cause:

- incorrect rule
- broad glob
- symlink traversal
- stale scan data
- mount boundary crossing
- treating user data as cache

Mitigations:

- dry-run default
- reversible cleanup only
- policy blocked paths
- no symlink following
- one-filesystem default
- re-stat paths before execution
- quarantine manifest
- restore command
- audit log

### 2. AI overreach

Cause:

- AI invents cleanup path
- AI invents shell command
- AI misclassifies important data
- AI lowers risk

Mitigations:

- AI output is non-authoritative
- structured output validation
- candidate IDs must already exist
- no arbitrary shell execution
- no risk downgrades
- no direct executor access

### 3. Command injection

Cause:

- adapter commands built from untrusted text
- shell execution
- AI-generated commands

Mitigations:

- use `Command` with argv arrays
- never invoke shell for adapter commands
- allowlisted commands only
- no AI-generated commands
- validate arguments

### 4. Privacy leakage

Cause:

- sending path names or file contents to AI provider
- indexing sensitive directories
- verbose logs

Mitigations:

- local-first AI provider
- AI disabled by default
- no file contents by default
- redact sensitive path segments
- omit known secret/browser/mail/password-store directories
- safe logging defaults

### 5. TOCTOU filesystem races

Cause:

- file changes between scan and cleanup
- path replaced with symlink
- path moved outside allowed root

Mitigations:

- re-stat before execution
- verify inode/dev where possible
- reject symlink transitions
- verify canonical path remains inside allowed root
- block if metadata changed suspiciously

### 6. Supply-chain risk

Cause:

- malicious dependency
- compromised adapter tool
- imported cleaner rules

Mitigations:

- keep dependencies small
- review unsafe dependencies
- pin versions
- avoid executing imported CleanerML directly
- imported rules disabled by default
- adapter allowlists
- CI checks

### 7. Audit tampering

Cause:

- missing audit entries
- cleanup without restore metadata
- partial failures

Mitigations:

- write action records before and after execution
- include restore path
- include rule ID and risk
- record partial failures
- make restore manifest explicit

## Security posture

MVP posture:

```text
No permanent deletion.
No arbitrary shell.
No AI authority.
No file contents sent to AI.
No symlink following.
No high-risk cleanup.
```

## Review checklist

Before enabling real cleanup:

- [ ] dry-run output is exact
- [ ] policy blocks forbidden paths
- [ ] symlink tests pass
- [ ] mount boundary tests pass
- [ ] quarantine restore works
- [ ] stale scan detection works
- [ ] adapter allowlist tests pass
- [ ] AI cannot create executable actions
- [ ] audit log records success and failure
