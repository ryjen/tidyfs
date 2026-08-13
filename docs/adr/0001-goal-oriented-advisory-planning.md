# ADR 0001: Goal-Oriented Advisory Planning

- **Status:** Accepted
- **Date:** 2026-08-12
- **Decision source:** #51
- **Implementation slice:** #53

## Context

`tidyfs` already has a deterministic cleanup planner, explicit protected-category and risk policy, reversible quarantine/recovery, candidate-level AI analysis, and conservative AI-enriched planning.

The remaining product gap is not additional model authority. It is a user-facing way to ask a bounded cleanup question such as “what existing low-risk plan candidates should I use to reclaim 20 GiB?” without weakening the safety model.

The alternatives considered in #51 were:

1. goal-oriented advisory planning over existing deterministic candidates;
2. AI-generated deterministic rule suggestions;
3. hardened non-loopback/remote AI transport;
4. executable tool-native adapter cleanup;
5. maintenance-only/no new feature milestone.

## Decision

`tidyfs` will add **goal-oriented advisory planning over an already-built deterministic plan**.

AI may select, rank, group, and explain only candidate IDs that tidyfs explicitly supplies from a conservatively canonicalized view of the current persisted plan. A model-selected set is advisory evidence, not cleanup authorization.

The first implementation uses a read-only goal recommendation flow:

```text
persisted deterministic plan
        |
        v
exact-path risk/policy canonicalization
        |
        v
leaf-most non-overlapping filesystem candidate set
        |
        v
bounded risk/root/target constraints
        |
        v
canonical goal + plan digest
        |
        v
loopback /v1/recommend
        |
        v
strict response correlation/validation
        |
        v
re-read and recanonicalize persisted plan
        |
        v
plan digest + selected-ID revalidation
        |
        v
deterministic reclaim-byte calculation
        |
        v
read-only recommendation output
```

## Authority boundary

AI does **not** gain authority to:

- create an executable candidate;
- select an ID that was not supplied in the request;
- lower deterministic risk;
- remove a protected-category or policy block;
- promote `report_only` or `tool_native` work into filesystem mutation;
- broaden the selected scan root;
- determine authoritative reclaim totals;
- persist executable cleanup state;
- invoke `clean`, quarantine, restore, adapter cleanup, shell commands, or permanent deletion.

`tidyfs` remains authoritative for:

- which persisted plan candidates are eligible for goal advice;
- exact-path and ancestor/descendant canonicalization;
- root and risk filtering;
- candidate identity and freshness;
- reclaim-byte calculation over the model-visible non-overlapping set;
- whether the requested target is satisfied;
- all plan persistence;
- explicit approval;
- filesystem mutation and recovery.

## Conservative hierarchy rule

The deterministic planner may persist multiple rule matches for the same path and may also persist both ancestor and descendant filesystem paths.

The first goal-recommendation slice applies these fail-safe rules before inference:

1. exact-path rule matches are grouped;
2. the exact path inherits the highest deterministic risk;
3. any blocked, non-reversible, or non-quarantine exact-path match makes that path unavailable to goal advice;
4. any filesystem path with a descendant candidate is suppressed;
5. only leaf-most, pairwise non-overlapping paths are eligible for risk filtering and model selection.

A descendant suppresses its ancestor even when the descendant itself is blocked or above the requested risk threshold. This prevents an ancestor recommendation from bypassing stricter policy attached to data it contains.

This hierarchy rule is intentionally conservative and specific to the read-only recommendation slice. It may undercount reclaimable bytes that belong only to a suppressed ancestor. Issue #62 tracks the shared deterministic hierarchy semantics needed for `plan`, dry-run, and execution.

## Freshness and correlation

The goal request is bound to a deterministic digest of:

- scan identity;
- requested reclaim target;
- selected maximum risk;
- optional root constraint;
- the exact non-overlapping candidate IDs and bounded candidate facts supplied to the gateway.

The gateway response must correlate to both the request ID and this digest.

After inference, tidyfs re-reads the persisted plan, repeats the same canonicalization, and re-derives the digest. A changed plan, unknown candidate ID, duplicate candidate ID, malformed response, or provider failure causes the recommendation to fail closed.

The goal-plan digest is a freshness/correlation binding, not an authentication token or authority capability.

## Reclaim totals

The model is not an authority for byte totals. The recommendation contract returns selected candidate IDs and explanation. `tidyfs` computes selected reclaim bytes from its own revalidated, pairwise non-overlapping candidate set and determines `target_met` from that value.

The first slice prefers conservative undercounting to double-counting. `target_met: false` can therefore be correct even when a future hierarchy-aware deterministic planner could attribute additional safe bytes from a suppressed ancestor.

## Transport and privacy

The first goal-recommendation transport inherits the current gateway security posture:

- numeric loopback only;
- no DNS;
- no redirects;
- no credentials;
- bounded request/response sizes;
- explicit timeouts;
- JSON-only responses;
- no arbitrary file contents.

Path disclosure remains explicitly bounded and defaults to redacted representation for recommendation workflows. Candidate analysis and goal recommendation share the same path privacy transformation.

Non-loopback inference is a separate future decision because it adds TLS, authentication/capability identity, endpoint-policy, credential, privacy, and operational obligations.

## Consequences

### Benefits

- turns the existing AI foundation into direct user-visible cleanup guidance;
- preserves deterministic execution authority;
- reuses the existing loopback trust boundary and stale-observation pattern;
- avoids model-visible overlapping payloads and duplicate reclaim-byte accounting;
- keeps the first slice read-only and highly reversible;
- produces a clear seam for future recommendation UX without coupling the executor to model output.

### Costs

- adds a second structured gateway task and contract;
- requires plan-level digest/correlation validation in addition to candidate-level observation binding;
- requires conservative hierarchy canonicalization before and after inference;
- may understate reclaimable space until #62 defines shared deterministic hierarchy semantics;
- requires the local/supervisor gateway to implement `/v1/recommend` before the feature is usable with AI.

### Accepted limitations

The first slice accepts a numeric reclaim target rather than free-form natural-language goals. Natural-language goal parsing is intentionally deferred until the structured contract and deterministic validation prove useful.

The first slice also accepts conservative undercounting from leaf-most hierarchy filtering. It does not attempt to solve deterministic planner/executor overlap semantics inside this AI-focused PR.

## Deferred decisions

The following are explicitly not implied by this ADR:

- deterministic planner/executor hierarchy changes beyond the read-only recommendation guard (#62);
- AI-generated rule activation;
- remote/non-loopback inference;
- executable tool-native adapter cleanup;
- automatic cleanup after recommendation;
- permanent deletion.

Each requires its own evidence and, where trust or mutation boundaries change, focused security/architecture review.

## Reassessment triggers

Revisit this decision if:

- useful recommendation quality cannot be achieved from bounded existing-plan facts;
- conservative hierarchy undercounting materially harms recommendation usefulness;
- users require remote inference rather than loopback/supervisor deployment;
- plan-level recommendation latency or candidate volume makes the bounded request model impractical;
- a future executor requires a different authority model.
