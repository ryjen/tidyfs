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
   versioned proposal/selection
        |
        v
 transport/schema/correlation validation
        |
        v
 authoritative observation/plan revalidation
        |
        v
 deterministic risk + byte calculations
        |
        v
 explicit approval
        |
        v
 reversible quarantine/recovery
```

The model is an advisor. Deterministic facts, policy, risk gates, byte calculations, explicit approval, and the existing reversible executor remain authoritative.

## Current AI responsibilities

AI may:

- classify ambiguous disk usage from bounded metadata;
- explain why space is being consumed;
- recommend `ignore`, `review`, or `quarantine` for an already-observed candidate;
- raise effective risk or make an otherwise eligible deterministic candidate review-only;
- provide rationale, caveats, confidence, and provider/model provenance;
- select and explain a subset of existing eligible persisted plan candidate IDs for a bounded numeric reclaim target.

These are reasoning tasks. They do not grant mutation authority.

## Non-authority guarantees

AI-facing contracts deliberately expose no permanent deletion or raw filesystem mutation operation. A model cannot directly invoke quarantine, restore, rename, delete, adapter cleanup, shell commands, or other executor primitives.

The current `AiCleanupProposal` action vocabulary is limited to:

- `ignore`
- `review`
- `quarantine`

Even `quarantine` is only advisory. AI is evaluated only after deterministic rules and protected-category policy have already made a candidate eligible for reversible quarantine. The model cannot:

- create a new executable candidate;
- select a goal candidate ID not supplied by tidyfs;
- lower deterministic risk;
- remove a deterministic block;
- convert `report_only` or `tool_native` work into filesystem mutation;
- broaden the selected root;
- determine authoritative reclaim-byte totals;
- persist a goal recommendation as executable authority;
- authorize permanent deletion;
- bypass `clean --safe --interactive` or any identity/recovery check.

## Candidate-level proposal validation

Every candidate-level AI proposal is untrusted input and must satisfy a versioned schema before later planning stages can consume it. Validation includes:

- exact supported schema version;
- non-empty bounded classification;
- finite confidence in `0.0..=1.0`;
- bounded non-empty rationale;
- known risk and action enums through typed deserialization;
- bounded provider/model/request provenance;
- rejection of unknown response fields.

Unknown schema versions, malformed output, unsupported actions, and provider failures fail closed.

## Candidate observation identity and freshness

Candidate-level recommendations are bound to the exact post-privacy facts sent to inference.

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

## Goal-oriented advisory planning

ADR 0001 accepts a second read-only AI task: selecting among already-eligible candidates in an existing persisted plan for a bounded reclaim target.

```text
persisted cleanup_candidates
        |
        v
filter: unblocked + reversible + quarantine + root
        |
        v
canonicalize one row per filesystem path using highest deterministic risk
        |
        v
apply requested risk threshold
        |
        v
bounded candidate IDs/facts + numeric reclaim target
        |
        v
opaque goal/plan freshness digest
        |
        v
POST /v1/recommend
        |
        v
strict request/provenance/digest + selected-ID validation
        |
        v
re-read persisted plan with identical rules
        |
        v
exact fact comparison + digest re-derivation
        |
        v
tidyfs sums selected bytes and computes target_met
        |
        v
read-only terminal output
```

The model returns selected candidate IDs plus rationale/caveats. It does not own the byte total. `tidyfs` calculates reclaim bytes from the revalidated current plan and determines whether the target is met.

Multiple deterministic rules may match one path. Goal recommendation canonicalizes to one candidate row per path before applying the requested risk threshold, using the highest deterministic risk among otherwise eligible reversible quarantine matches. This prevents both double-counting and a lower-risk duplicate from hiding a higher-risk rule for the same filesystem payload.

The goal/plan digest is an opaque correlation and freshness binding, not an authentication token or capability. The authoritative stale-state defense is the post-inference persisted-plan re-read and exact comparison of the facts supplied to inference.

Goal recommendations are not written back as cleanup authority. `clean` consumes the persisted deterministic plan under its existing risk, approval, identity, quarantine, and recovery rules.

## Provider boundary

`AiAnalysisProvider` remains the provider-neutral candidate-analysis port. It accepts bounded `AiAnalysisRequest` facts and returns an `AiCleanupProposal` or provider error. `analyze_validated` independently validates returned proposals.

The core provider interface contains no hosted-provider SDK, credentials, generic chat/messages API, MCP/tool invocation, or filesystem mutation capability.

`LoopbackGatewayProvider` supplies two narrow structured HTTP tasks:

```http
POST /v1/analyze
POST /v1/recommend
Content-Type: application/json
Accept: application/json
```

`/v1/analyze` handles one bound candidate observation. `/v1/recommend` handles a bounded goal plus an explicit set of already-eligible candidate IDs/facts.

Runtime constraints are shared:

- numeric loopback address only (`127.0.0.0/8` or `::1`);
- explicit non-zero port;
- HTTP only because transport never leaves the host;
- no DNS resolution;
- no redirects;
- no credentials;
- no transfer encoding/chunked response support;
- bounded request, header, and response sizes;
- connect/read/write timeouts;
- exact request/provenance and observation-or-plan correlation.

A non-loopback/remote provider is intentionally **not** a configuration toggle. Remote inference would introduce TLS, authentication/capability identity, endpoint-policy, credential, privacy, and operational obligations and should be designed as a separate security decision.

## Privacy and data minimization

The inference contracts contain metadata only. Arbitrary file contents are not sent.

Supported path modes:

- `full` — exact indexed path;
- `basename` — basename only;
- `redacted` — remove user/project-specific prefixes while preserving useful ecosystem structure such as `.cache`, `.gradle/caches`, `DerivedData`, `node_modules`, and `/nix/store`.

`tidyfs analyze` permits an explicitly selected loopback model to receive full paths. AI-enriched planning and goal-oriented `recommend` default to `redacted` because the model only needs enough structure to advise on candidates whose eligibility is already determined by tidyfs.

The path privacy transformation is shared between candidate analysis and goal recommendation so the two inference routes cannot silently diverge in redaction behavior.

## Provenance and explainability

Accepted candidate-level recommendations retain enough evidence to explain their origin and freshness:

- scan and stable candidate identity;
- path privacy mode;
- observation digest;
- classification and confidence;
- risk and recommended action;
- rationale and caveats;
- provider, model, and request ID;
- selected risk context.

`tidyfs explain <path>` re-derives current authoritative observation facts and reports stored candidate-level AI evidence as fresh or stale.

Goal recommendations are intentionally ephemeral in the first slice. Terminal output reports the request/plan binding, selected candidates, tidyfs-computed reclaim bytes, `target_met`, model provenance, rationale, and caveats without modifying cleanup/action authority.

Model-controlled terminal and bidirectional-control characters are escaped before display.

## Threats explicitly covered

Tests and integration behavior cover:

- malformed/partial model output;
- unknown schema versions and unknown response fields;
- invalid confidence/risk/action values;
- hallucinated/unsupported action names;
- prompt-injection/control text attempting to override policy or terminal rendering;
- stale/nonexistent/changed candidates;
- stale or changed persisted goal-plan facts;
- unknown or duplicate goal-selected candidate IDs;
- duplicate-path risk canonicalization and reclaim-byte deduplication;
- request-ID, provenance-ID, and digest mismatch;
- non-loopback/hostname endpoint rejection;
- redirects, transfer encoding, wrong content type, and oversized responses;
- provider failure/timeouts resulting in no cleanup authorization;
- deterministic risk not being lowered by AI;
- `review`, `ignore`, and low-confidence recommendations remaining non-executable;
- AI being unable to promote report-only/tool-native work;
- goal recommendation computing byte totals locally and performing no action/candidate mutation;
- end-to-end AI-enriched planning followed by `clean --dry-run` with no filesystem mutation.

The safe failure mode for AI remains: **no accepted recommendation and no additional filesystem authority.**

## Deferred decisions

Goal-oriented advisory planning is the accepted post-`v0.6.1` direction. The first slice intentionally accepts a structured numeric reclaim target rather than a free-form goal.

AI-generated disabled rule proposals, natural-language goal parsing, non-loopback transport, and tool-native cleanup execution remain separate future decisions.
