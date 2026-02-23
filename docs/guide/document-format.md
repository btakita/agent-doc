# Document Format

## Structure

Session documents are markdown files with YAML frontmatter:

```markdown
---
session: 05304d74-90f1-46a1-8a79-55736341b193
agent: claude
model: null
branch: null
---

# Session: Topic Name

## User

Your question or instruction here.

## Assistant

(agent writes here)

## User

Follow-up. You can also annotate inline:

> What about edge cases?
```

## Frontmatter fields

| Field | Required | Default | Description |
|-------|----------|---------|-------------|
| `session` | no | (generated on first run) | Session ID for continuity |
| `agent` | no | `claude` | Agent backend to use |
| `model` | no | (agent default) | Model override |
| `branch` | no | (none) | Git branch for session commits |

All fields are optional and default to null.

## Frontmatter parsing

Delimited by `---\n` at the start of the file and a closing `\n---\n`. If frontmatter is absent, all fields default to null and the entire content is treated as the body.

## Interaction modes

### Append mode

Structured `## User` / `## Assistant` blocks. Each run appends a new assistant response.

### Inline mode

Annotations anywhere — blockquotes, edits to previous responses, comments in the body. The diff captures what changed; the agent addresses inline edits alongside new `## User` content.

Both modes work simultaneously because the run sends a diff, not a parsed structure.

## History rewriting

Delete anything from the document. On next run, the diff shows deletions and the agent sees the cleaned-up document as ground truth. This lets you:

- Remove irrelevant exchanges
- Consolidate scattered notes
- Restructure the conversation
- Correct earlier context
