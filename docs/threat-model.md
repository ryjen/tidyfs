# Threat Model

`tidyfs` is a filesystem tool with potential destructive capability. Its primary risks are data loss, privacy leakage, command injection, stale-state races, compromised advisory input, and misplaced trust in AI.

## Assets

Assets to protect:

- user documents
- source code and Git history
- secrets and keys
- password stores and browser profiles
- local databases and VM/container volumes
- cloud-sync data
- cleanup manifests and audit records
- private filenames, usernames, and project names

## Authority and trust boundaries

```text
filesystem
    |
    v
scanner / adapters
    |
    v
authoritative SQLite scan facts
    |
    v
deterministic classification + rules + protected-category policy
    |
    +----------------------------------+
    | optional bounded AI advisory     |
    | candidate analysis / goal select |
    | local numeric-loopback gateway   |
    +----------------+-----------------+
                     |
                     v
        strict schema/correlation validation
                     |
                     v
      authoritative observation/plan revalidation
                     |
                     v
      deterministic risk + byte calculations
                     |
                     v
          explicit interactive approval
                     |
                     v
        reversible quarantine executor
                     |
                     v
          durable recovery / restore
```

The filesystem executor is the highest-impact authority. AI and adapter inspection are deliberately outside that authority boundary.

AI is an **untrusted advisory principal**. A configured local gateway may classify, explain, recommend, or select among candidate IDs explicitly supplied by `tidyfs`, but it cannot create an executable candidate, lower deterministic risk, remove a policy block, execute adapter cleanup, invoke arbitrary shell commands, authorize permanent deletion, or call filesystem mutation primitives.

## Main threats

### 1. Accidental data loss

Causes:

- incorrect rules or broad matches
- stale scan data
- symlink/path substitution
- mount-boundary mistakes
- treating user-owned data as regenerable cache
- interrupted mutation or recovery errors

Mitigations:

- deterministic policy before optional AI enrichment
- blocked/protected categories
- dry-run preview
- explicit `--safe --interactive` execution gate
- reversible quarantine only; no permanent deletion
- source device/inode checks and symlink rejection
- same-filesystem quarantine preflight
- payload identity verification
- durable transitional action states
- explicit read-only recovery and explicit restore
- atomic no-overwrite restore on supported platforms

Residual risk:

A non-cooperating external writer can still race the final pathname metadata check and rename. The moved payload is verified afterward and a mismatch is detected rather than accepted as successful cleanup. See `recovery.md` for the exact limitation and reconciliation model.

### 2. AI overreach or misleading advice

Causes:

- hallucinated actions, candidate IDs, or paths
- prompt-injection text embedded in filesystem metadata
- a compromised or buggy local model gateway
- stale recommendations replayed against changed facts
- model attempts to lower risk or override deterministic policy
- persuasive explanation being mistaken for authorization
- model-authored reclaim totals being mistaken for authoritative facts

Mitigations:

- versioned typed proposal and goal-recommendation contracts
- candidate-level action vocabulary restricted to `ignore`, `review`, `quarantine`
- goal recommendations may select only IDs explicitly supplied by `tidyfs`
- strict response deserialization and bounded model-controlled text
- request ID plus observation-or-plan binding correlation
- candidate identity bound to authoritative scan/index or persisted plan facts
- post-inference authoritative observation/plan reconstruction
- deterministic blocks always win
- effective risk can only stay equal or increase
- duplicate rule matches for one filesystem path are canonicalized using the highest deterministic risk before applying the requested threshold
- low-confidence candidate advice is review-only
- provider failure or invalid/stale output creates no accepted recommendation
- reclaim totals and `target_met` are calculated by `tidyfs`, not trusted from model output
- goal recommendations are ephemeral advisory output and are not persisted as executable authority

Residual risk:

A malicious local gateway can lie in explanations, raise risk, omit useful candidates, or choose a poor subset. Under the current design it cannot broaden filesystem mutation authority. This is primarily an integrity/availability and path-privacy concern, not an executor-authority grant.

### 3. Goal-selection confusion or duplicate-path risk reduction

Causes:

- the same filesystem payload matching multiple deterministic rules
- a lower-risk duplicate match hiding a higher-risk match
- counting one filesystem payload more than once toward a reclaim target
- selecting unknown or duplicate candidate IDs
- changing the persisted plan during model inference

Mitigations:

- goal recommendation operates only on already-persisted, unblocked, reversible quarantine candidates
- candidate rows are canonicalized per filesystem path before inference
- canonicalization preserves the highest deterministic risk for the payload
- requested risk filtering happens after canonicalization
- request candidate IDs are explicit and bounded
- unknown or duplicate selected IDs are rejected
- the exact eligible candidate facts are re-read after inference
- a plan/fact mismatch fails closed
- selected reclaim bytes are summed from the revalidated unique-path set

Residual risk:

A bounded candidate limit can make a target unsatisfiable even when additional eligible candidates exist outside the request. The safe result is `target_met: false`; the model is not permitted to expand the candidate set.

### 4. Gateway and privacy leakage

Causes:

- disclosing usernames, project names, or sensitive path structure
- sending more filesystem data than classification requires
- accidentally enabling remote transport without appropriate controls

Mitigations:

- current runtime accepts numeric loopback addresses only (`127.0.0.0/8` and `::1`)
- no DNS resolution, redirects, credentials, or remote endpoint support in v1
- no arbitrary file contents in inference contracts
- explicit path modes: `full`, `basename`, `redacted`
- AI-enriched planning and goal recommendation default to `redacted`
- shared privacy transformation for candidate-analysis and goal-recommendation paths
- bounded request and response sizes
- explicit connect/read/write timeouts

Remote inference is not a transparent extension of the current trust model. HTTPS, authentication/capability identity, endpoint policy, path disclosure, operational logging, and credential handling require a separate security decision before non-loopback transport is added.

### 5. Command injection and external-tool authority

Causes:

- adapter commands built from untrusted text
- shell execution
- model-generated commands
- future tool-native execution treating preview text as authority

Mitigations:

- adapter inspection uses explicit allowlisted argv arrays
- no shell construction from model or filesystem text
- AI cannot generate executable commands
- adapter candidates remain `tool_native` and report-only
- AI cannot promote `tool_native` or `report_only` candidates into raw filesystem mutation

Executing tool-native cleanup is a distinct future mutation boundary and requires its own command contract, recovery/observability model, and security review.

### 6. TOCTOU and stale-state races

Causes:

- path changes between scan, planning, fingerprinting, and cleanup
- candidate facts change while AI inference is running
- persisted goal-plan facts change while recommendation inference is running
- old AI evidence is replayed against a new observation

Mitigations:

- scan candidate identity is stable and tied to authoritative indexed facts
- candidate analysis requests carry a deterministic observation digest
- goal requests carry an opaque digest of the selected scan, goal constraints, and supplied candidate facts
- planning/recommendation re-query authoritative facts after inference and re-derive the relevant binding
- exact post-inference goal candidate facts must match the facts supplied before inference
- cleanup rechecks source device/inode and rejects symlink transitions
- payload device/inode and hash are verified after quarantine
- stale or mismatched AI recommendations fail closed

The goal-plan digest is a correlation/freshness binding, not an authentication token or capability. The authoritative defense is the post-inference persisted-plan re-read and exact fact comparison.

### 7. Supply-chain and release risk

Causes:

- vulnerable or malicious Rust dependency
- compromised GitHub Actions dependency
- release artifact differing from validated build output
- compromised adapter tool

Mitigations:

- locked dependencies and RustSec audit gate
- formatting, Clippy, test, and Cargo package verification in CI
- tag/version binding for release builds
- deterministic release bundle construction and SHA-256 verification
- read-only build job and isolated write-scoped release publication job
- retained workflow artifacts for independent release verification
- adapter allowlists and no adapter mutation execution

Signing/SLSA-style provenance is not currently implemented and remains a future distribution-hardening decision.

### 8. Audit or recovery-state tampering

Causes:

- missing action records
- cleanup without payload identity or restore metadata
- database failure around filesystem mutation
- manually modified SQLite state

Mitigations:

- persist action intent before filesystem mutation
- durable `planned`, `moving`, `quarantined`, `restoring`, `restored`, and `failed` states
- payload SHA-256 identity evidence
- startup detection of interrupted actions
- explicit serialized `recover` / `restore`
- reconciliation from observed filesystem state plus recorded identity rather than optimistic state transitions

Goal recommendations intentionally do not create action records or modify cleanup candidates in the first slice.

## Current security posture

```text
No permanent deletion.
No arbitrary shell.
No AI mutation authority.
No AI candidate-creation authority.
No model-authored reclaim totals as authority.
No adapter execution authority.
No arbitrary file contents sent to AI.
No non-loopback AI transport.
No symlink following for cleanup.
No stale AI recommendation authority.
Explicit approval before reversible mutation.
```

## Regression checklist

The current implementation is expected to preserve all of these properties:

- [x] dry-run output does not mutate the filesystem
- [x] deterministic policy blocks protected/ineligible candidates before AI can influence them
- [x] symlink/path-substitution checks are covered
- [x] mount/device boundaries are covered
- [x] quarantine, restore, interruption, and recovery paths are integration-tested
- [x] stale source identity is rejected
- [x] adapter inspection is allowlisted and non-mutating
- [x] AI cannot create or promote executable actions
- [x] candidate-level AI recommendation freshness is re-derived from authoritative facts
- [x] goal recommendations can select only supplied eligible candidate IDs
- [x] duplicate-path canonicalization cannot lower deterministic risk or double-count reclaim bytes
- [x] goal-plan freshness is re-read and validated after inference
- [x] goal reclaim totals and `target_met` are calculated by `tidyfs`
- [x] goal recommendation does not mutate cleanup candidates, actions, or filesystem state
- [x] invalid/malformed/provider-failure AI output fails closed
- [x] action state records success, interruption, recovery, and failure context

Any change that adds permanent deletion, non-loopback inference, tool-native execution, broader AI authority, natural-language goal parsing with new trust implications, or a new mutation primitive should reopen focused threat analysis before implementation.
