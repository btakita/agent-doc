> Extracted from SPEC.md — see [index](../SPEC.md)

# Document Format

## Session Document

Frontmatter fields:
- `agent_doc_session`: Document/routing UUID — permanent identifier for tmux pane routing. Legacy alias: `session` (read but not written).
- `agent_doc_format`: Document format — `inline` (canonical), `template` (default: `template`). `append` accepted as backward-compat alias for `inline`.
- `agent_doc_write`: Write strategy — `merge` or `crdt` (default: `crdt`).
- `agent_doc_mode`: **Deprecated.** Single field mapping: `append` → format=append, `template` → format=template, `stream` → format=template+write=crdt. Explicit `agent_doc_format`/`agent_doc_write` take precedence. Legacy aliases: `mode`, `response_mode`.
- `agent`: Agent backend name (overrides config default)
- `model`: Model override (passed to agent backend)
- `branch`: Reserved for branch tracking
- `agent_args`: Additional CLI arguments for the active agent process (space-separated string)
- `claude_args`: Additional CLI arguments for the `claude` process (space-separated string, see §6.1)
- `codex_args`: Additional CLI arguments for the `codex` process (space-separated string, see §6.1)

All fields are optional and default to null. Resolution: explicit `agent_doc_format`/`agent_doc_write` > deprecated `agent_doc_mode` > defaults (template + crdt). The body alternates `## User` and `## Assistant` blocks (append format) or uses named components (template format).

## Frontmatter Parsing

Delimited by `---\n` at file start and closing `\n---\n`. If absent, all fields default to null and entire content is the body.

## Components

Documents can contain named, re-renderable regions called components:

```html
<!-- agent:status -->
content here
<!-- /agent:status -->
```

Marker format: `<!-- agent:{name} -->` (open) and `<!-- /agent:{name} -->` (close). Names must match `[a-zA-Z0-9][a-zA-Z0-9-]*`. Components are patched via `agent-doc patch`.

**Inline attributes:** Open markers support inline attribute overrides: `<!-- agent:name patch=append -->`. `mode=` is accepted as a backward-compatible alias; `patch=` takes precedence if both are present. `max_lines=N` trims component content to the last N lines after patching (0 or absent = unlimited). Precedence chain: inline attribute > `.agent-doc/components.toml` > built-in default (`replace` for patch, unlimited for max_lines).

**Code range exclusion:** Component marker detection uses pulldown-cmark for CommonMark-compliant code range detection, replacing the previous regex-based approach. Markers inside inline code spans or fenced code blocks are excluded and never treated as component boundaries.

**Standard component names:**

| Component | Default `patch` | Description |
|-----------|----------------|-------------|
| `exchange` | append | Conversation history — each cycle appends |
| `findings` | append | Accumulated research data — grows over time |
| `status` | replace | Current state — updated at milestones |
| `pending` | replace | Task queue — auto-cleaned each cycle |
| `output` | replace | Latest agent response only |
| `input` | replace | User prompt area |
| (custom) | replace | All other components default to replace |

Per-component behavior is configured in `.agent-doc/components.toml` (see §7.21).
