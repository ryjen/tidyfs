# Cleanup Candidate Hierarchy Policy

`tidyfs` treats overlapping filesystem candidates as one safety domain. A cleanup plan must never count or execute the same physical payload through both an ancestor and a descendant path, and a permissive parent must never bypass stricter policy attached to data below it.

This policy applies to raw-filesystem cleanup candidates used by `plan` and `clean`. Tool-native adapter pseudo-paths such as `adapter://docker` are not filesystem paths and remain outside this hierarchy policy.

## Invariants

For raw filesystem candidates:

1. one exact filesystem path has one canonical cleanup decision;
2. exact-path canonicalization never lowers effective risk;
3. blocked, non-reversible, or non-quarantine/trash exact-path matches cannot be hidden by a more permissive match;
4. an ancestor with any independently classified descendant is not executable or reclaim-countable;
5. a blocked, protected, or above-threshold descendant still suppresses its ancestor;
6. the model-visible, dry-run, and executable filesystem sets are pairwise non-overlapping;
7. conservative omission or undercounting is acceptable; double-counting or authority expansion is not.

## Exact-path canonicalization

A path may match more than one deterministic rule. `tidyfs` groups those matches before optional AI enrichment.

The canonical decision:

- uses the highest effective risk among exact-path matches;
- chooses a stable representative by risk/restriction/rule identity;
- is blocked when any exact-path match is already blocked;
- is blocked when any exact-path match is non-reversible;
- is blocked when any exact-path match is not a raw-filesystem `quarantine` or `trash` action.

This prevents a low-risk or executable duplicate from hiding a stricter interpretation of the same payload.

## Ancestor and descendant candidates

After exact-path grouping, `tidyfs` walks filesystem parents component-by-component. If a canonical candidate has another canonical candidate below it, the ancestor is blocked.

Example:

```text
/cache             1.0 GiB  quarantine, low
/cache/sub         0.4 GiB  quarantine, low
```

Only `/cache/sub` remains reclaim-countable/executable. `/cache` is retained as a blocked plan record so the suppression is visible, but its size is not added to allowed reclaim totals.

The rule is deliberately the same when the descendant is stricter:

```text
/cache             quarantine, low
/cache/private     report_only, forbidden
```

`/cache/private` blocks itself and also suppresses `/cache`. The parent cannot become a shortcut around protected child data.

For deeper nesting, every candidate that has a candidate descendant is suppressed. The remaining allowed filesystem candidates are leaf-most and pairwise non-overlapping.

## Ordering and complexity

Exact paths are grouped in deterministic path order. Representative selection is deterministic using effective risk, restriction state, reversibility/action type, rule ID, and original position as tie breakers.

Ancestor suppression uses a component-aware parent walk and an ordered path index. Its cost scales with total candidate path depth rather than comparing every path to every other path.

## Planner enforcement

`plan` canonicalizes raw filesystem candidates after deterministic rule/policy evaluation and before optional AI enrichment.

Therefore:

```text
scan/classification
  -> rule matches + deterministic policy
  -> hierarchy canonicalization
  -> optional AI enrichment of canonical eligible leaves
  -> plan totals/display
  -> persisted cleanup_candidates
```

AI never receives overlapping filesystem payloads from this planning path. Persisted plans contain one exact-path representative and retain suppressed ancestors only as blocked records.

## Cleanup enforcement and legacy plans

`clean` does not assume that persisted rows were created by a hierarchy-aware version of `tidyfs`. It loads blocked and unblocked rows, reapplies the same canonicalization, and only then applies the requested risk/root/limit filters.

This defense-in-depth rule means both:

```text
clean --dry-run
clean --safe --interactive
```

operate on the same non-overlapping candidate set, including when reading an older database containing duplicate or ancestor/descendant rows.

Existing execution controls remain independent and unchanged: explicit safe/interactive approval, reversible quarantine, same-filesystem preflight, source device/inode and symlink checks, payload identity verification, durable action states, recovery, restore, and no permanent deletion.

## Adapter boundary

`adapter://...` candidates describe tool-owned cleanup/reporting workflows rather than raw filesystem rename targets. They pass through hierarchy canonicalization unchanged and do not suppress filesystem candidates based on string-prefix resemblance.

Tool-native execution remains outside the current executor authority boundary.

## Accepted trade-off

Leaf-most suppression can understate reclaimable bytes because an ancestor may contain bytes not represented by any descendant candidate. The current policy intentionally accepts that loss of accounting precision.

The safe direction is:

```text
under-count / omit
```

not:

```text
double-count / overlap / bypass stricter child policy
```

Any future attempt to attribute ancestor-only bytes more precisely must preserve the non-overlap and no-authority-expansion invariants above.