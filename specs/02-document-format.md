> Extracted from SPEC.md — see [index](../SPEC.md)

# Document Format

## Session Document

Frontmatter fields:
- `agent_doc_session`: Document/routing UUID — permanent identifier for tmux pane routing. Legacy alias: `session` (read but not written).
- `agent_doc_format`: Document format — `inline` (canonical), `template` (default: `template`). `append` accepted as backward-compat alias for `inline`.
- `agent_doc_write`: Write strategy — `merge` or `crdt` (default: `crdt`).
- `agent_doc_mode`: **Deprecated.** Single field mapping: `append` → format=append, `template` → format=template, `stream` → format=template+write=crdt. Explicit `agent_doc_format`/`agent_doc_write` take precedence. Legacy aliases: `mode`, `response_mode`.
- `agent`: Agent backend name (overrides config default)
- `model`: Model override (passed to agent backend). Overridden by harness-specific fields when present.
- `claude_model`: Per-harness model override for Claude Code sessions. Takes precedence over `model` when running under Claude Code. The short alias `opus` is **deferred**: agent-doc passes it through verbatim as `claude --model opus` so Claude Code resolves its current latest opus, and response attribution is self-stamped by the running agent rather than a pinned version. Explicit concrete ids (e.g. `claude-opus-4-8`) pass through unchanged for users who want to pin a version.
- `codex_model`: Per-harness model override for Codex sessions. Takes precedence over `model` when running under Codex.
- `opencode_model`: Per-harness model override for OpenCode sessions. Takes precedence over `model` when running under OpenCode.
- `branch`: Reserved for branch tracking
- `agent_args`: Additional CLI arguments for the active agent process (space-separated string)
- `claude_args`: Additional CLI arguments for the `claude` process (space-separated string, see §6.1)
- `codex_args`: Additional CLI arguments for the `codex` process (space-separated string, see §6.1)
- `opencode_args`: Additional CLI arguments for the `opencode` process (space-separated string, see §6.1)

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

**Attribute validation (preflight):** Recognized attribute keys are `patch`, `mode`, `max_lines`, `archive`, `transfer-source`, `timestamp`, `broken`, the queue-only `auto`, and the backlog/icebox-only `queue` (see §2.5 backlog→queue sync). Preflight emits a non-blocking `misplaced_component_attr` warning when the queue-only `auto` attribute appears on a non-`queue` component (e.g. `<!-- agent:backlog auto -->`), when the `queue` sync attribute carries an unrecognized mode (e.g. `<!-- agent:backlog queue=nope -->`; valid: bare `queue` = append, `sync`, `append`, `prepend`), or when an unrecognized key appears on any component (e.g. the typo `auot`). The attribute is never mutated by this check and the warning exists so a misplaced/misspelled attribute is surfaced instead of silently tolerated. This closes `#backlog-auto-marker-misfire`, where `auto`/`auot` on the backlog marker had previously been parsed and ignored without feedback. The auto-loop activates only from `<!-- agent:queue auto -->` (see §2.5); the `queue` attribute on a backlog/icebox only populates `agent:queue` (`#backlog-queue-sync-attr`), it does not start the loop.

**Code range exclusion:** Component marker detection uses pulldown-cmark for CommonMark-compliant code range detection, replacing the previous regex-based approach. Markers inside inline code spans or fenced code blocks are excluded and never treated as component boundaries.

**Standard component names:**

| Component | Default `patch` | Description |
|-----------|----------------|-------------|
| `exchange` | append | Conversation history — each cycle appends |
| `findings` | append | Accumulated research data — grows over time |
| `status` | replace | Current state — updated at milestones |
| `queue` | (none) | Prompt queue — consumed sequentially (see §2.5) |
| `pending` | replace | Task backlog — auto-cleaned each cycle |
| `review` | replace | Code-complete tracked work awaiting human review; mutated through review/pending flags |
| `icebox` | replace | Project icebox — items parked outside active backlog |
| `output` | replace | Latest agent response only |
| `input` | replace | User prompt area |
| (custom) | replace | All other components default to replace |

Per-component behavior is configured in `.agent-doc/components.toml` (see §7.21).

### §2.5 Queue Component

The `agent:queue` component holds a batch of prompts consumed sequentially. It is scaffolded between `exchange` and `pending` in the default template.

**Syntax:** hybrid list items and fenced prompts.

| Form | Example | Description |
|------|---------|-------------|
| Single-line | `- do #fix1` | Bare `- ` prefix at column 0 |
| Multi-line (tilde) | `~~~prompt`...`~~~` | Fenced with `~~~prompt` opener |
| Multi-line (dash) | `---`...`---` | Fenced with bare `---` |
| Start fence | `--- start [at <datetime>]` | Activation signal (consumed on use) |
| Stop fence | `--- stop` | Breakpoint (consumed when reached) |

**Attributes:** `<!-- agent:queue auto -->` enables immediate activation when the queue is non-empty. The `auto` attribute is stripped when the queue drains. `auto` is **queue-only**: placing it on any other component (e.g. `agent:backlog`) does not activate the auto-loop and triggers a `misplaced_component_attr` preflight warning (see §2.4).

**Activation resolution (preflight):** Preflight detects the `agent:queue` component and resolves activation in priority order:

1. **`auto` attribute** — `<!-- agent:queue auto -->` activates immediately when prompts exist.
2. **Start fence at head** — bare `--- start` is consumed and activates; `--- start at <time>` defers (emits `queue_deferred: true`, `queue_start_at`).
3. **Exchange trigger** — user writes `do queue` or `run queue` in the exchange.
4. **Persisted state** — `queue_active: true` in frontmatter (set on activation, cleared on drain).

On activation, preflight emits `queue_active: true`, `queue_prompts: [...]` (ordered prompt texts), and `queue_trigger` (how the queue was activated). The first prompt is the effective user edit for the cycle.

When the queue drains to empty: `auto` is stripped from the opening tag, `queue_active` is cleared in frontmatter.

**Consumption (Phase 3):** After a successful response write (via `finalize` or `write --commit`), strict closeouts first clear the remaining pending-maintenance / pending-guard gates. Only then is the consumed prompt marked complete in the `agent:queue` block as `- ~prompt text~` before the commit boundary so the same git commit can capture both the response and the queue advance. Queue consumption requires one of three prompt proofs: the current diff contains the exact queue-head prompt, the run path supplied a queue-synthetic diff for the active head, or the closeout explicitly resolved the head `do #id` with `--done <id>`. An empty baseline/current diff, or an unrelated prompt such as `#next-steps` that was already present in the baseline, must not consume the active head by itself. Missing-response materialization repairs, including `write --commit` with empty stdin and Codex Stop-hook recovery, preserve the current queue head and `auto` attribute unless the repair carries explicit head proof such as a matching `--done <id>` or `### Re:` topic. If the closeout explicitly resolves multiple contiguous queued `do #id` prompts with repeated `--done <id>` flags, those head prompts are consumed together; consumption stops at the first unresolved or non-matching queue prompt. The file and snapshot transforms are both proven first, then written in sync so change detection works on the next cycle.

- **Auto-strike of resolved head prompts (preflight maintenance):** Before dispatching the head, preflight walks leading `Prompt` entries and rewrites any whose `#id` is already resolved into `Completed` (`~text~`) entries. An id counts as resolved when present in `agent:done` (canonical) **or** in `agent:review` with the pending-gate marker `- [/]` (code-complete, awaiting an external gate). The pending-gate inclusion unblocks queue stalls when a do-item lands a contained phase of a multi-phase plan but the parent item stays gated for remaining phases. Strike scanning stops at the first head prompt whose id is not resolved so the next live head stays intact for the normal consumption path. The strike telemetry log line records `source=done` vs `source=review_gated` so audit can see which path advanced the queue.
- **Drain:** When the last prompt is consumed, the queue body is cleared, `auto` is stripped, and `queue_active` is cleared.
- **Fail-closed proof for required closeouts:** When `queue_active: true`, required closeouts must be able to prove the same head prompt, or same contiguous done-backed head prompts, were completed or drained from both the live file and the snapshot before mutating either side. Missing/malformed queue state, missing snapshot state, or file/snapshot head mismatch aborts the closeout before commit.
- **Stop fence at head:** If the next entry is `--- stop`, preflight halts the queue (strips `auto`, clears `queue_active`), consumes the fence, and emits `queue_halted: "stop_fence"`. No prompt is dispatched.
- **Time gate at head:** If the next entry is `--- start at <time>` and the time hasn't arrived, preflight emits `queue_deferred: true` and skips the cycle. When the time arrives, the fence is consumed and the next prompt dispatches.
- **Item modified:** If the head prompt's text differs between snapshot and file for a queue that was already active in the snapshot, preflight halts with `queue_halted: "item_modified"`. The user must restart the queue explicitly.
- **New activation snapshot:** If the queue was inactive in the snapshot and the current file newly activates it with `auto`, a start fence, or an exchange trigger, the current queue body is the operator-authored input for this cycle. Preflight must not treat the changed head as an item-modified halt; it persists the newly activated queue body and `queue_active: true` to the snapshot so closeout can prove and consume the same head prompt.
- **Appended items:** New items added after the head prompt are not a halt — only the next-to-consume item triggers change detection.

**Parsing rules:**
1. Lines starting with `- ` at column 0 → single-line prompt.
2. `~~~prompt` opens a multi-line prompt fence; `~~~` closes it.
3. Bare `---` (not followed by ` start` or ` stop`) opens a multi-line prompt fence; matching `---` closes it.
4. `--- start`, `--- start <time>`, `--- start at <time>`, `~~~start` → start fence.
5. `--- stop`, `~~~stop` → stop fence.
6. Blank lines between items are ignored.
7. Content outside list items, fences, or control fences is a parse error.
