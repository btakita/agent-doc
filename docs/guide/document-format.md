# Document Format

## Structure

Session documents are markdown files with YAML frontmatter:

```markdown
---
agent_doc_session: 05304d74-90f1-46a1-8a79-55736341b193
agent: claude
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
| `agent_doc_session` | no | (written by `agent-doc init`) | Session ID for continuity. The older spelling `session` is still read. |
| `agent` | no | `claude` | Agent backend to use |
| `model` | no | (agent default) | Model override |
| `branch` | no | (none) | Git branch for session commits |

All fields are optional and default to null.

## Frontmatter parsing

Delimited by `---\n` at the start of the file and a closing `\n---\n`. If frontmatter is absent, all fields default to null and the entire content is treated as the body.

## Prompt presets

Prompt presets are named, reusable instructions. A session document can define
its own `prompt_presets:` (or the shorter `presets:` alias), or a project can
provide defaults for every session below its nearest `.agent-doc/` directory.
Put project defaults in `.agent-doc/project.md`:

```markdown
---
presets:
  "#review": Review the change and run focused tests.
  "#release":
    prompt: Prepare and publish the release.
    runbook: runbooks/release.md
    model: opus
    effort: high
---

# Agent Doc project defaults
```

Invoke a preset by writing its name wherever you would submit a prompt. For
example, add `#review` to the session's exchange, or add `- #review` to an active
`agent:run`. Agent Doc expands the name to its instruction during preflight;
an associated runbook is loaded only when that preset is invoked.

`agent:run` is the canonical runnable-work component. Existing
`agent:queue` components remain fully supported as a compatibility alias; Agent
Doc preserves their spelling, while new documents and generated components use
`agent:run`.
Structured presets may also declare `model` and `effort` for queue-dispatched
subagents. Queue-marker overrides win, then item-specific and queue presets,
then the session's `subagent_model` / `subagent_effort` defaults. Omitted policy
inherits the harness coordinator settings.

Project presets are live defaults, not a copied project template. Changes to
`.agent-doc/project.md` apply to existing session documents without rewriting
them. A session-local preset with the same exact name overrides the project
value; an unfinished or null local declaration also masks the inherited value
while it is being edited. Presets and `agent:rules` are inherited. Other
frontmatter fields stay document-local. Preset inheritance walks every ancestor
project file and overlays nearer names. Rules are different: preflight reads
every `agent:rules` body in the nearest `.agent-doc/project.md`, in file order,
then every `agent:rules` body in the session document, in file order. Session
rules add to those project rules. They do not replace them, and an ancestor
project file does not contribute rules. A markdown list is one rule per item.
Free text is included as written. Multiple `agent:rules` elements are allowed
in either file. The rest of the project file's body is not inherited.

## Interaction modes

### Append mode

Structured `## User` / `## Assistant` blocks. Each run appends a new assistant response.

### Inline mode

Annotations anywhere — blockquotes, edits to previous responses, comments in the body. The diff captures what changed; the agent addresses inline edits alongside new `## User` content.

Both modes work simultaneously because the run sends a diff, not a parsed structure.

## Components

Documents can contain **components** — named, re-renderable regions marked with HTML comment pairs:

```markdown
<!-- agent:status -->
| Field | Value |
|-------|-------|
| build | passing |
<!-- /agent:status -->
```

Components are updated via `agent-doc patch` or by agents/scripts. Their content can be replaced, appended to, or prepended to based on configuration in `.agent-doc/components.toml`.

Regular HTML comments (`<!-- like this -->`) remain a private scratchpad — they're stripped during diff and never trigger responses. Component markers look like comments but are structural and preserved.

See the [Components](components.md) guide for full details.

## History rewriting

Delete anything from the document. On next run, the diff shows deletions and the agent sees the cleaned-up document as ground truth. This lets you:

- Remove irrelevant exchanges
- Consolidate scattered notes
- Restructure the conversation
- Correct earlier context
