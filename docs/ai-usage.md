# AI Usage

AI is optional and non-authoritative.

The deterministic core must remain useful with AI disabled.

## Core rule

```text
scanner finds facts
rules create candidates
policy validates candidates
AI explains and ranks
executor acts only on validated candidates
```

## AI can do

- explain disk usage
- rank existing cleanup candidates
- explain risk and tradeoffs
- answer natural-language questions over scan summaries
- classify ambiguous paths as report-only suggestions
- draft disabled rule proposals

## AI cannot do

- invent paths to delete
- invent shell commands
- lower risk tiers
- override blocked candidates
- inspect file contents by default
- execute actions
- create enabled rules directly

## Modes

### Off

```yaml
ai:
  enabled: false
```

Everything works deterministically.

### Explain only

```yaml
ai:
  enabled: true
  mode: explain
```

AI receives validated plans and summaries only.

### Suggest classifications

```yaml
ai:
  enabled: true
  mode: suggest
```

AI can suggest labels for ambiguous directories. Suggestions are report-only.

### Draft rules

```yaml
ai:
  enabled: true
  mode: draft_rules
```

AI can draft disabled rules for review.

## Example: explaining disk usage

Input to AI:

```json
{
  "root": "/home/rj",
  "largest_directories": [
    { "path": "~/src", "size": "82 GB", "labels": ["source_projects"] },
    { "path": "~/.cache", "size": "41 GB", "labels": ["cache"] },
    { "path": "~/.local/share/Steam", "size": "36 GB", "labels": ["application_data"] },
    { "path": "/var/lib/docker", "size": "24 GB", "labels": ["docker_data"] }
  ],
  "protected_categories": [
    "source_repo",
    "secret_material",
    "database",
    "vm_image"
  ]
}
```

AI response:

```text
Most space is split between source projects, user cache, Steam application data, and Docker data.

The safest cleanup candidates are likely under ~/.cache and Docker builder/image cache. ~/src and Steam are large but should be treated as owned user/application data, not automatic cleanup targets.
```

## Example: ranking candidates

Input:

```json
{
  "cleanup_candidates": [
    {
      "candidate_id": 1,
      "path": "~/.cache/thumbnails",
      "size": "4.3 GB",
      "risk": "low",
      "reason": "Generated thumbnails older than 60 days"
    },
    {
      "candidate_id": 2,
      "path": "~/.cache/pip",
      "size": "3.1 GB",
      "risk": "low",
      "reason": "Python package cache older than 30 days"
    },
    {
      "candidate_id": 3,
      "path": "~/src/foo/node_modules",
      "size": "8.7 GB",
      "risk": "medium",
      "reason": "Regenerable dependency directory in inactive project"
    }
  ]
}
```

Valid AI output:

```json
{
  "summary": "The lowest-risk space recovery is from generated caches.",
  "recommended_order": [
    {
      "candidate_id": 1,
      "reason": "Low risk and fully regenerable."
    },
    {
      "candidate_id": 2,
      "reason": "Low risk, but future installs may be slower."
    },
    {
      "candidate_id": 3,
      "reason": "High reclaim value, but medium risk because it affects a project workspace."
    }
  ],
  "warnings": []
}
```

## Output validation

AI output must be validated.

Rules:

- candidate IDs must already exist
- paths cannot be invented
- commands cannot be invented
- risk cannot be lowered
- actions cannot be changed
- blocked candidates cannot become allowed

## Redaction

Before sending anything to AI:

- redact emails
- redact usernames if configured
- omit secret paths
- omit file contents
- summarize large directories
- avoid browser profile details
- avoid mail/password manager/wallet directories

## Providers

Planned provider model:

```yaml
ai:
  provider: ollama
  endpoint: http://localhost:11434
  model: qwen3:8b
  send_file_contents: false
```

The default should be local-first and content-free.
