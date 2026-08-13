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

AI may select, rank, group, and explain only candidate IDs that tidyfs explicitly supplies from the current eligible plan. A model-selected set is advisory evidence, not cleanup authorization.

The first implementation uses a read-only goal recommendation flow:

```text
persisted deterministic plan
        |
        v
bounded eligible candidate set
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
re-read authoritative persisted plan
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

- which persisted plan candidates are eligible;
- root and risk filtering;
- candidate identity and freshness;
- exact reclaim-byte calculation;
- whether the requested target is satisfied;
- all plan persistence;
- explicit approval;
- filesystem mutation and recovery.

## Freshness and correlation

The goal request is bound to a deterministic digest of:

- scan identity;
- requested reclaim target;
- selected maximum risk;
- optional root constraint;
- the exact eligible candidate IDs and bounded candidate facts supplied to the gateway.

The gateway response must correlate to both the request ID and this digest.

After inference, tidyfs re-reads the persisted plan and re-derives the digest. A changed plan, unknown candidate ID, duplicate candidate ID, malformed response, or provider failure causes the recommendation to fail closed.

## Reclaim totals

The model is not an authority for byte totals. The recommendation contract returns selected candidate IDs and explanation. `tidyfs` computes selected reclaim bytes from its own current plan records and determines `target_met` from that value.

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

Path disclosure remains explicitly bounded and defaults to redacted representation for recommendation workflows.

Non-loopback inference is a separate future decision because it adds TLS, authentication/capability identity, endpoint-policy, credential, privacy, and operational obligations.

## Consequences

### Benefits

- turns the existing AI foundation into direct user-visible cleanup guidance;
- preserves deterministic execution authority;
- reuses the existing loopback trust boundary and stale-observation pattern;
- keeps the first slice read-only and highly reversible;
- produces a clear seam for future recommendation UX without coupling the executor to model output.

### Costs

- adds a second structured gateway task and contract;
- requires plan-level digest/correlation validation in addition to candidate-level observation binding;
- requires the local/supervisor gateway to implement `/v1/recommend` before the feature is usable with AI.

### Accepted limitation

The first slice accepts a numeric reclaim target rather than free-form natural-language goals. Natural-language goal parsing is intentionally deferred until the structured contract and deterministic validation prove useful.

## Deferred decisions

The following are explicitly not implied by this ADR:

- AI-generated rule activation;
- remote/non-loopback inference;
- executable tool-native adapter cleanup;
- automatic cleanup after recommendation;
- permanent deletion.

Each requires its own evidence and, where trust or mutation boundaries change, focused security/architecture review.

## Reassessment triggers

Revisit this decision if:

- useful recommendation quality cannot be achieved from bounded existing-plan facts;
- users require remote inference rather than loopback/supervisor deployment;
- plan-level recommendation latency or candidate volume makes the bounded request model impractical;
- a future executor requires a different authority model.
