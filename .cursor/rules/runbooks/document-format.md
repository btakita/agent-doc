# Document Format

Session documents are markdown files with YAML frontmatter. Two modes: **inline** (legacy) and **template** (current default for new docs).

## Frontmatter

```yaml
---
agent_doc_session: <uuid or null>
agent: <name or null>
model: <model or null>
branch: <branch or null>
agent_doc_format: <inline | template>   # default: inline
agent_doc_write: <append | crdt>         # crdt required for stream mode
agent_doc_mode: <append | template>      # legacy alias for agent_doc_format
agent_doc_model_tier: <low | med | high> # optional; feeds model tier gate
---
```

`agent_doc_format: inline` is the canonical name for the old "append" format. `append` is accepted as a backward-compat alias. Similarly, `agent_doc_mode: template` is a legacy alias for `agent_doc_format: template`.

## Inline mode

The body alternates `## User` and `## Assistant` blocks. Inline annotations within any block — blockquotes, HTML comments, edits to previous responses — are valid prompts and show up in the next diff.

`agent-doc write --stream` appends `## Assistant\n\n<response>\n\n## User\n\n` on each cycle.

## Template mode

The body contains named components delimited by HTML comment markers:

```markdown
<!-- agent:exchange -->
<!-- /agent:exchange -->

<!-- agent:backlog -->
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->
```

### Conventional component names

- `<!-- agent:input -->` — user writes prompts here (read-only for the agent).
- `<!-- agent:output -->` — agent responds here when no other target is given.
- `<!-- agent:exchange -->` — shared conversation surface; user and agent both write inline. Default target for unmatched patches and the canonical location for the response boundary marker.
- `<!-- agent:backlog -->` — task tracker managed through granular pending ops (see `pending-ops.md`). Legacy alias: `<!-- agent:pending -->`.
- `<!-- agent:icebox -->` — project icebox for items parked outside the active backlog. Sits after `agent:backlog` in the default template.
- `<!-- agent:status -->`, `<!-- agent:log -->`, `<!-- agent:architecture -->`, ... — agent-managed via targeted patch blocks.

### Inline component attributes

```markdown
<!-- agent:output patch=append max_lines=50 -->
```

- `patch=<mode>` — one of `append`, `prepend`, `replace`. `mode=` is accepted as a backward-compat alias; `patch=` wins if both are present.
- `max_lines=<N>` — trim the component to the last N lines after patching. `0` or absent = unlimited.

Mode precedence (highest → lowest): stream overrides > inline attribute > `.agent-doc/config.toml` `[components.<name>]` > built-in default (`exchange`/`findings` default to `append`; everything else defaults to `replace`).

## Snapshot storage

- Location: `.agent-doc/snapshots/` relative to the project root.
- Filename: `sha256(canonical_path) + ".md"`.
- Contains the full document content after the last submit.
- The skill never reads snapshots directly — use `agent-doc read <FILE>` to fetch HEAD content.
