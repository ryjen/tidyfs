# AI architecture and trust boundary

`tidyfs` uses AI to improve filesystem understanding and cleanup recommendations without making model output authoritative for filesystem mutation.

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

AI may:

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

Recommendations carry provider/model provenance and may carry a request identifier. Provider integration should additionally persist enough bounded input context or a digest of that context to reproduce and explain why a recommendation was made without unnecessarily retaining sensitive filesystem data.

## Privacy and data minimization

The inference boundary is designed around metadata already gathered by tidyfs: candidate identity, path, size, age, deterministic classification/rule context, and adapter source. Reading or transmitting arbitrary file contents is not part of the initial AI request contract.

A future privacy-hardening iteration may replace or redact raw paths for remote providers where semantic path information is unnecessary.

Local inference or a supervisor/model gateway is preferred over coupling the mutation core to a hosted provider SDK. Provider credentials and network access remain outside the filesystem mutation boundary.

## Provider boundary

`AiAnalysisProvider` is the provider-neutral port. It accepts an `AiAnalysisRequest` containing bounded filesystem facts and returns an `AiCleanupProposal` or a provider error.

Callers use `analyze_validated`, which rejects a provider response unless the proposal passes the independent proposal validator. Provider failure or invalid model output therefore produces no accepted recommendation.

The core port contains no HTTP client, hosted-provider SDK, credentials, or filesystem mutation capability. A concrete local/supervisor gateway adapter should:

1. serialize only the bounded request facts;
2. invoke the configured model gateway;
3. deserialize the response into `AiCleanupProposal`;
4. map transport/parse failures to `AiProviderError`;
5. return the proposal to `analyze_validated` for independent validation.

A hosted-provider adapter can implement the same port without changing planning or mutation code.

## Threats to test explicitly

Provider and planner integration must include tests for:

- malformed/partial model output;
- unknown schema versions;
- confidence values outside the allowed range;
- hallucinated action names such as `delete` or `rm`;
- prompt-injected filesystem names or adapter output attempting to override policy;
- recommendations referring to stale/nonexistent candidates;
- provider failure/timeouts resulting in no cleanup authorization;
- provenance omission or ambiguity.

The safe failure mode for AI is always: no recommendation accepted, no filesystem mutation authorized.
