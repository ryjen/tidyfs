# AI architecture and trust boundary

`tidyfs` is intended to use AI to improve filesystem understanding and cleanup recommendations without making model output authoritative for filesystem mutation.

## Mental model

```text
filesystem + adapters
        |
        v
 deterministic facts/index
        |
        v
 AI analysis/recommendation
        |
   versioned proposal
        |
        v
 deterministic validation
        |
        v
 policy + risk gates
        |
 explicit approval
        |
        v
 reversible quarantine/recovery
```

The model is an advisor. The deterministic planner and policy layer remain the authority.

## AI responsibilities

AI may eventually:

- classify ambiguous disk usage from bounded metadata and adapter facts;
- explain why space is being consumed;
- group related caches, build products, and generated artifacts;
- recommend cleanup candidates and risk levels;
- translate user cleanup intent into candidate constraints;
- propose new deterministic cleanup rules for review.

These are reasoning tasks. They do not grant mutation authority.

## Non-authority guarantees

AI-facing contracts deliberately do not expose permanent deletion or raw filesystem mutation operations. A model cannot directly invoke quarantine, restore, rename, delete, or adapter cleanup through the proposal contract.

The initial `AiCleanupProposal` action vocabulary is limited to:

- `ignore`
- `review`
- `quarantine`

Even `quarantine` is only a recommendation. It must still pass deterministic policy/risk validation and the existing explicit execution gates before any filesystem mutation occurs.

## Proposal validation

Every AI proposal is treated as untrusted input and must satisfy a versioned schema before it can enter later planning stages. The first contract validates:

- exact supported schema version;
- non-empty classification;
- finite confidence in `0.0..=1.0`;
- at least one non-empty rationale item;
- known risk and action enum values through typed deserialization;
- non-empty provider and model provenance.

Unknown schema versions and malformed values fail closed.

## Provenance

Recommendations carry provider/model provenance and may carry a request identifier. Future provider integration should additionally persist enough bounded input context or a digest of that context to reproduce and explain why a recommendation was made without unnecessarily retaining sensitive filesystem data.

## Privacy and data minimization

The preferred inference input is metadata already gathered by tidyfs: paths where necessary, sizes, ages, classifications, adapter facts, and deterministic rule results. Reading or transmitting arbitrary file contents is not required for the initial AI feature set and should remain opt-in if ever introduced.

Local inference or a supervisor/model gateway is preferred over coupling the mutation core to a hosted provider SDK. Provider credentials and network access should remain outside the filesystem mutation boundary.

## Provider boundary

A future provider abstraction should accept a bounded analysis request and return a serialized proposal. It should not receive references to `clean`, quarantine, recovery, database mutation, or filesystem mutation primitives.

This keeps the core model-provider independent and allows local models, a supervisor gateway, or hosted inference to be swapped without changing the safety path.

## Threats to test explicitly

Provider integration must include tests for:

- malformed/partial model output;
- unknown schema versions;
- confidence values outside the allowed range;
- hallucinated action names such as `delete` or `rm`;
- prompt-injected filesystem names or adapter output attempting to override policy;
- recommendations referring to stale/nonexistent candidates;
- provider failure/timeouts resulting in no cleanup authorization;
- provenance omission or ambiguity.

The safe failure mode for AI is always: no recommendation accepted, no filesystem mutation authorized.
