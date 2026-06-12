# Milestone 2: Deterministic Classification

## Goal

Add the second vertical slice:

```text
scan -> SQLite -> classify -> explain
```

Classification is intentionally deterministic. It is based on path shape, known developer-tool conventions, file extensions, and simple project markers.

## Commands

```bash
tidyfs scan ~
tidyfs classify
tidyfs classify --summary
tidyfs explain ~/.cache
tidyfs explain ~/src/foo/node_modules
```

## Design boundary

Classification answers:

```text
What does this appear to be?
```

It does not answer:

```text
Should this be cleaned?
```

That belongs to Milestone 3: rules and planning.

## Classifier sources

Classification rows include:

- `label`
- `confidence`
- `source`
- `reason`

Initial source value:

```text
builtin_path_classifier
```

## Unknowns

If no specific classifier matches a path, `explain` reports:

```text
No classifications found.
```

For directories, this usually means Milestone 3 should treat the path as observe-only/report-only.

## Safety posture

No deletion. No cleanup. No AI.

Sensitive paths are classified so later policy code can block them.
