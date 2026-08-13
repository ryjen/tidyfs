# AI architecture and trust boundary

`tidyfs` uses AI to improve filesystem understanding and cleanup recommendations without making model output authoritative for filesystem mutation.

## Mental model

```text
filesystem + adapters
        |
        v
 authoritative scan/index facts
        |
        v
 deterministic classification + rules + protected-category policy
        |
        v
 optional bounded AI analysis/recommendation
        |
   versioned proposal
        |
        v
 transport/schema/correlation validation
        |
        v
 authoritative observation revalidation
        |
        v
 conservative conflict resolution
        |
        v
 selected user risk threshold
        |
        v
 explicit approval
        |
        v
 reversible quarantine/recovery
```

The model is an advisor. Deterministic facts, policy, risk gates, explicit approval, and the existing reversible executor remain authoritative.

## Current AI responsibilities

AI may:

- classify ambiguous disk usage from bounded metadata;
- explain why space is being consumed;
- recommend `ignore`, `review`, or `quarantine` for an already-observed candidate;
- raise effective risk or make an otherwise eligible deterministic candidate review-only;
- provide rationale, caveats, confidence, and provider/model provenance.

Future advisory capabilities may include grouping/ranking deterministic candidates, translating a bounded user reclaim goal into a recommendation over an existing deterministic plan, and drafting disabled rule suggestions for review.

These are reasoning tasks. They do not grant mutation authority.

## Non-authority guarantees

AI-facing contracts deliberately expose no permanent deletion or raw filesystem mutation operation. A model cannot directly invoke quarantine, restore, rename, delete, adapter cleanup, shell commands, or other executor primitives.

The current `AiCleanupProposal` action vocabulary is limited to:

- `ignore`
- `review`
- `quarantine`

Even `quarantine` is only advisory. AI is evaluated only after deterministic rules and protected-category policy have already made a candidate eligible for reversible quarantine. The model cannot:

- create a new executable candidate;
- lower deterministic risk;
- remove a deterministic block;
- convert `report_only` or `tool_native` work into filesystem mutation;
- broaden the selected root;
- authorize permanent deletion;
- bypass `clean --safe --interactive` or any identity/recovery check.

## Proposal validation

Every AI proposal is untrusted input and must satisfy a versioned schema before later planning stages can consume it. Validation includes:

- exact supported schema version;
- non-empty bounded classification;
- finite confidence in `0.0..=1.0`;
- bounded non-empty rationale;
- known risk and action enums through typed deserialization;
- bounded provider/model/request provenance;
- rejection of unknown response fields.

Unknown schema versions, malformed output, unsupported actions, and provider failures fail closed.

## Observation identity and freshness

AI recommendations are bound to the exact post-privacy facts sent to inference.

```text
scan/index candidate
      |
      v
canonical bounded observation
      |
      v
SHA-256 observation digest
      |
      v
AI request / response correlation
      |
      v
post-inference authoritative reconstruction
      |
      +---- digest/identity mismatch ---> reject recommendation
      |
      v
conservative deterministic planning
```

The observation digest is not a filesystem-content hash. It binds the metadata actually supplied to inference, including stable candidate identity, path representation, size/scan-relative age, labels, deterministic policy context, and adapter identity where applicable.

Persisted AI evidence is audit/explanation data. Stored digests are never treated as freshness authority; `tidyfs` re-derives the current observation from authoritative scan/index facts.

## Conservative planning policy

When an already-eligible deterministic quarantine candidate is enriched by AI:

1. deterministic blocks always win;
2. effective risk is the maximum of deterministic and AI risk;
3. `ignore` and `review` make the candidate non-executable;
4. low-confidence advice is review-only;
5. `quarantine` can only preserve an already-valid reversible quarantine action;
6. the selected user risk threshold is applied again after AI enrichment;
7. stale or failed AI analysis leaves no accepted enriched recommendation.

This creates a one-way authority rule: **AI may preserve or reduce existing deterministic authority, never increase it.**

## Provider boundary

`AiAnalysisProvider` is the provider-neutral port. It accepts bounded `AiAnalysisRequest` facts and returns an `AiCleanupProposal` or provider error. `analyze_validated` independently validates returned proposals.

The core provider interface contains no hosted-provider SDK, credentials, generic chat/messages API, MCP/tool invocation, or filesystem mutation capability.

The implemented v1 adapter is `LoopbackGatewayProvider`, which uses a narrow HTTP transport:

```http
POST /v1/analyze
Content-Type: application/json
Accept: application/json
```

Runtime constraints:

- numeric loopback address only (`127.0.0.0/8` or `::1`);
- explicit non-zero port;
- HTTP only because transport never leaves the host;
- no DNS resolution;
- no redirects;
- no credentials;
- no transfer encoding/chunked response support;
- bounded request, header, and response sizes;
- connect/read/write timeouts;
- exact request-ID, provenance-ID, and observation-digest correlation.

A non-loopback/remote provider is intentionally **not** a configuration toggle. Remote inference would introduce TLS, authentication/capability identity, endpoint-policy, credential, privacy, and operational obligations and should be designed as a separate security decision.

## Privacy and data minimization

The inference contract contains metadata only. Arbitrary file contents are not sent.

Supported path modes:

- `full` — exact indexed path;
- `basename` — basename only;
- `redacted` — remove user/project-specific prefixes while preserving useful ecosystem structure such as `.cache`, `.gradle/caches`, `DerivedData`, `node_modules`, and `/nix/store`.

`tidyfs analyze` permits an explicitly selected loopback model to receive full paths. AI-enriched planning defaults to `redacted` because the model only needs enough structure to advise on an already-deterministic candidate.

## Provenance and explainability

Accepted recommendations retain enough evidence to explain their origin and freshness:

- scan and stable candidate identity;
- path privacy mode;
- observation digest;
- classification and confidence;
- risk and recommended action;
- rationale and caveats;
- provider, model, and request ID;
- selected risk context.

`tidyfs explain <path>` re-derives current authoritative observation facts and reports stored AI evidence as fresh or stale.

Model-controlled terminal and bidirectional-control characters are escaped before display.

## Threats explicitly covered

Tests and integration behavior cover:

- malformed/partial model output;
- unknown schema versions and unknown response fields;
- invalid confidence/risk/action values;
- hallucinated/unsupported action names;
- prompt-injection text attempting to override policy;
- stale/nonexistent/changed candidates;
- request-ID, provenance-ID, and digest mismatch;
- non-loopback/hostname endpoint rejection;
- redirects, transfer encoding, wrong content type, and oversized responses;
- provider failure/timeouts resulting in no cleanup authorization;
- deterministic risk not being lowered by AI;
- `review`, `ignore`, and low-confidence recommendations remaining non-executable;
- AI being unable to promote report-only/tool-native work;
- end-to-end AI-enriched planning followed by `clean --dry-run` with no filesystem mutation.

The safe failure mode for AI remains: **no accepted recommendation and no additional filesystem authority.**

## Next decision

The current architecture is sufficient for candidate-level local AI analysis and conservative planning. The post-`v0.6.1` product direction is tracked in QART issue #51 rather than assuming that remote transport, rule generation, or tool-native execution is the next step.