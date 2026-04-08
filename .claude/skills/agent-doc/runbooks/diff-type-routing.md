# Diff-Type Routing

The `diff_type` field in preflight JSON classifies the user's changes before the LLM sees them. Use it to optimize response strategy and reduce unnecessary processing.

## Classification Types

| Type | Pattern | Example |
|------|---------|---------|
| `approval` | Single word: go, yes, do, ok, continue, approve, y, yep, sure, proceed, lgtm | `+go` |
| `simple_question` | Single added line ending with `?` | `+what is the release process?` |
| `boundary_artifact` | Only `(HEAD)` marker or boundary UUID changes | `-### Re: X (HEAD)` / `+### Re: X` |
| `annotation` | Inline edit to existing content (colon-append, >60% common prefix) | `-deployed` / `+deployed: confirmed` |
| `structural_change` | Only deletions, no additions | `-removed line` |
| `multi_topic` | Multiple separated added blocks or `---` separators | Two+ distinct insertion points |
| `content_addition` | General new content (default fallback) | `+implement feature X` |

## Routing Rules

### `boundary_artifact` -> Stop
No meaningful user change. Tell the user "no changes" and stop the cycle. These are git gutter artifacts from the `(HEAD)` boundary marker.

### `approval` -> Execute
The user approved something. Skip confirmation dialogs. Look at the preceding exchange context to determine what was approved, then execute it.

**"Approval" means skip the confirmation, NOT skip quality processes.** All runbooks still apply:
- Precommit runbook (tests, edge cases, clippy)
- Verification before done
- Plan mode for non-trivial tasks

Common approval patterns:
- "go" / "do" after a proposed plan -> execute the plan with full rigor
- "yes" after a question -> proceed with the suggested approach
- "continue" -> resume from where you left off

### `simple_question` -> Answer directly
The user asked a focused question. Answer concisely without deep document analysis. The question text is in the diff.

### `annotation` -> Respond to the edit
The user modified agent content inline (colon-appended note, correction, etc.). Read the specific edit and respond to it. The `diff_type_reason` will say "inline edit to existing content".

### `structural_change` -> Acknowledge
The user deleted or reorganized content. Acknowledge the change. Do NOT re-add deleted content — the user removed it intentionally.

### `multi_topic` -> Split response
Follow the [multi-topic response runbook](multi-topic-response.md). Use `### Re:` headers per topic with streaming checkpoints between them.

### `content_addition` -> Default behavior
New content added. Read the diff, understand intent, respond normally.

## Precedence

Classification priority (checked in order):
1. `boundary_artifact` (highest — avoids wasting tokens)
2. `approval`
3. `simple_question`
4. `annotation`
5. `structural_change`
6. `multi_topic`
7. `content_addition` (default fallback)

## When to override

The classification is heuristic-based. Override it when:
- An "approval" word appears in a longer sentence (should be `content_addition`)
- A question mark appears in code or a quote (not a real question)
- The context makes the classification wrong

Trust the classification for clear-cut cases; use judgment for ambiguous ones.
