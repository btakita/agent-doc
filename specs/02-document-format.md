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

**Attribute validation (preflight):** Recognized attribute keys are `patch`, `mode`, `max_lines`, `archive`, `transfer-source`, `timestamp`, `broken`, the queue-only `auto` and `preset`, the backlog/pending-only `queue` (see §2.5 backlog→queue sync), and the backlog/icebox/queue `priority` ordering attribute (`#backlog-priority-attribute`; per-item priority lives in `priority=<1..9>` item tokens). Preflight emits a non-blocking `misplaced_component_attr` warning when a queue-only attribute (`auto`, `preset`) appears on a non-`queue` component (e.g. `<!-- agent:backlog auto -->`), when the `queue` sync attribute carries an unrecognized mode (e.g. `<!-- agent:backlog queue=nope -->`; valid: bare `queue` = append, `sync`, `append`, `prepend`), when `queue` appears on `agent:icebox`, or when an unrecognized key appears on any component (e.g. the typo `auot`). The attribute is never mutated by this check and the warning exists so a misplaced/misspelled attribute is surfaced instead of silently tolerated. This closes `#backlog-auto-marker-misfire`, where `auto`/`auot` on the backlog marker had previously been parsed and ignored without feedback. A legacy/manual queue can activate from `<!-- agent:queue auto -->` (see §2.5), but binary-owned route writes must never add `auto` and should strip it from touched queue tags; the `queue` attribute on a backlog/pending component only populates `agent:queue` (`#backlog-queue-sync-attr`), it does not start the loop. Icebox work must be moved to backlog, placed in `agent:queue` manually, or marked with a per-item enqueue token before it can run.

**Code range exclusion:** Component marker detection uses pulldown-cmark for CommonMark-compliant code range detection, replacing the previous regex-based approach. Markers inside inline code spans or fenced code blocks are excluded and never treated as component boundaries.

**Structured overlay CRDT:** The markdown AST crate owns a structured Yrs schema for the agent component overlay. The root map `agent_doc_overlay` stores `schema_version`, the visible markdown projection, and a `components` array; each component is a map with byte span metadata and an `items` array; each item is a map with stable `id`, normalized text, raw text, pin/strike flags, and kind metadata. Runtime snapshot writes persist this state beside the legacy merge state as `.agent-doc/crdt/<hash>.overlay.yrs`. Template/CRDT merges derive their merge-base state from this overlay projection when it matches the active cycle baseline; if the overlay sidecar is absent, corrupt, or stale relative to the baseline, the merge falls back to the explicit baseline text and logs the reason. The overlay merge base is order-stable for the append case (`#ipc-drift-order-stable-merge`): because the overlay source is used only when its markdown projection is byte-identical to the cycle baseline (otherwise the merge falls back to that baseline text), the derived merge base can never reorder committed exchange content, so a concurrent foreign tail append during generation cannot reverse a new `### Re:` response's lines or hoist it above the prior committed response. New structured states must not rely on the legacy single `content` `Text` root for component identity, and legacy `content` states are migrated by reparsing their markdown projection into this schema.

**Node-keyed mutations:** The markdown AST crate exposes component mutations keyed by semantic item identity plus occurrence, not absolute byte offsets: consume marks a node struck, dedup removes duplicate id-backed nodes while preserving free-text duplicates, reorder moves items by node key, and enqueue inserts relative to an existing node key. These APIs are the phase-4/5 bridge away from text-line heuristics; callers should treat pin markers and strike markers as node attributes, not identity text.

**Durable operation log (`#op-scoped-drift-1`):** Phase 1 of the operation-scoped drift model (`tasks/agent-doc/plan-operation-scoped-drift.md`) persists each cycle's node-keyed operations to a derived sqlite database at `.agent-doc/op-log.db`, tagged with **actor** and a **causal clock**. The actor (`agent_doc_core::op_log::OpActor`) is one of `agent`, `user`, `foreign_supervisor`, or `live_buffer`; `classify_actor` maps an `OpSource` provenance signal to the actor. At preflight the diff is snapshot↔document, so every observed node op is a `user` edit — the agent's own committed output already lives in the snapshot. The causal clock (`agent_doc_core::op_log::CausalClock`) carries a monotonic per-document Lamport tick plus the originating `agent_doc_session`. The durable store (`agent-doc-sqlite::op_log`) owns Lamport assignment: each appended op receives the next per-document tick so the log is totally ordered per document; a repeated preflight pass over the same uncommitted diff is idempotent (no duplicate row, clock unchanged) because append dedupes against the most recent op for the same node. Writes are best effort — like the archive index, the DB is rebuildable derived state and a write failure never blocks a cycle. This op log is the substrate the later phases read: phase 2 emits the TurnScope read/write set, and phase 3's affectedness classifier intersects incoming ops against that scope so independent ops integrate and persist without affecting the current turn.

**TurnScope manifest (`#op-scoped-drift-2`):** Phase 2 emits an operation manifest for the current turn in preflight output as `turn_scope`. An `Address` (`agent_doc_core::turn_scope::Address`) names a component occurrence, optionally narrowed to a `node_key` (a `node_key` of `null` addresses the whole component). A `TurnScope` has a `driver` (the queue node the turn answers, resolved from `prompt_targets`), a `read_set`, a `write_set`, and an `exchange_tail_floor`. `TurnScope::for_driver` builds the canonical scope: read `{driver, exchange tail}`; write `{exchange append, driver strike, backlog, status, review, done/archive, gitlink, editor writeback}` — the driver appears in the write set because the turn strikes the queue item it consumes. Queue remains node-scoped through the driver address, so sibling queue inserts are independent while edits to the running queue item affect the turn. The named output surfaces are component-scoped: unrelated component edits merge automatically, while same-component incompatible edits become visible conflicts. `done_archive`, `gitlink`, and `editor_writeback` are runtime surfaces addressed by the same classifier even when they are not markdown `agent:*` components. When no driver node resolves (a non-queue prompt, or an id absent from the queue) the manifest still lists the output components every turn touches but `driver` is `null`. The manifest is the substrate the phase-3 affectedness classifier intersects incoming ops against; emitting it never blocks a cycle.

**Exchange-tail node granularity (`#loop-guard-exchange-node-granularity`):** the `exchange` component is whole-component in `read_set`/`write_set`, but only its *active tail* affects the turn. `TurnScope::for_driver_with_exchange_tail` records `exchange_tail_floor` = the count of `exchange` item nodes present at turn start (computed by preflight from the document). An incoming `exchange` op affects the turn only when its within-component node index is at or above the floor — a tail append (a genuine mid-loop user prompt, which lands at index ≥ floor) or a tail edit. An edit to an OLD exchange block (index below the floor — committed history) classifies `independent` and must not preempt the auto-loop drain. The classifier receives each op's index via `DocumentOp::node_index` (after-index for inserts/replaces, before-index for removes; preflight fills it from the semantic node event). When no `exchange` nodes exist the floor is `null` and the classifier keeps its coarse whole-component behavior.

**Affectedness classifier (`#op-scoped-drift-3`):** Phase 3 replaces the coarse "any divergence affects the turn" assumption with a scope-intersection router, surfaced in preflight output as `op_affectedness`. `Address::overlaps` is the `conflict` primitive: two addresses overlap when they share a component **name** and either side is component-level (a whole-component address) or both name the same node key. The `occurrence` field is informational and is intentionally not part of the match — a node key already encodes the component index (`comp:index:id:n`), and the canonical write-set members built by `TurnScope::for_driver` are whole-component addresses that must match the component wherever it sits in the document (`#nm1x`). `classify_op(actor, op_kind, address, node_index, scope)` routes one op into the 5-class taxonomy — `affects(O,S) = conflict(O.target, read_set) ∨ conflict(O.target, write_set)`, with the exchange-tail floor narrowing `exchange` membership to the active tail (above):

1. **independent** — target ∉ scope: integrate + persist, no drift (a queue item inserted beside the running one, a comment/icebox edit).
2. **input_affecting** — target ∈ read_set: re-read the affected input (the user edits the driver item being answered).
3. **output_contended** — target ∈ write_set with a concurrent writer: CRDT merge (two supervisors append the exchange boundary).
4. **structural_dependency** — `remove`/`move` of a depended node: invalidate/adapt (the user deletes the running queue item).
5. **provenance_spoofed** — a `live_buffer` actor touching an in-scope address: a lagging editor sidecar misread as a user edit, suppressed.

`AffectednessClass::affects_turn` is true only for classes 2/3/4; classes 1 and 5 integrate/persist or are suppressed without disturbing the turn. `classify_cycle` classifies every node op of a cycle (actor-tagged via phase 1's op model) and sets `turn_affected` when any op is turn-affecting. Independent and provenance-spoofed edits — the false-positive drift that produced the monsterrodholders queue-churn and the lagging-sidecar finalize blocks — no longer count against the turn. Emitting the classification never blocks a cycle.

**Scope-aware finalize-path drift gate (`#nm1x`):** Phase 4 wires the affectedness model into the two live gates that previously failed closed on any divergence. At turn start preflight persists the derived `TurnScope` to `.agent-doc/turn-scope/<hash>.json` (best effort; cleared when no driver resolves) so the later finalize-path commit — a separate process invocation — can intersect incoming document ops against the same scope.

1. **Commit/absorb gate** — `git::has_non_exchange_component_drift_scoped` loads the persisted scope and classifies each non-exchange component's node-level changes between snapshot and file (or snapshot and HEAD). A change composed entirely of out-of-scope node ops (`independent`/`provenance_spoofed`) — a queue item inserted beside the running one, an icebox/done edit — no longer blocks the narrow agent-owned absorb path; it integrates and persists in the working tree. A read/write-set conflict (`input_affecting`/`output_contended`), a `remove`/`move` (`structural_dependency`), structural drift (component count / name / patch-mode mismatch), or any content change the node differ cannot fully explain still blocks. When no scope sidecar exists the gate falls back to its coarse, conservative behavior (block on any non-exchange drift).
2. **Live-buffer divergence gate** — the visible-write reconcile path no longer fails closed on every editor-buffer divergence from the expected disk state. When the editor-visible buffer digest *matches the current on-disk content*, the editor holds no unsaved edits ahead of disk: the divergence is disk-vs-expected (an independent/foreign document edit), the reconcilable `DiskDrifted` case, not a pending user edit. Only a genuine unsaved editor buffer ahead of disk still blocks with "visible editor buffer differs". This is the actor-provenance half of the swap (the live-buffer actor is not diverging when it already equals disk), complementing the staleness suppression (`#ipc-crdt-response-drift`).

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
| Single-line | `- do #fix1` | Bare `- ` prefix at column 0. A single stray leading backtick (`` `- text ``, a common code-span mistype) is normalized to `- text` so the item parses as a prompt and self-heals on re-render instead of being silently skipped as inert text (`#queue-line-leading-backtick-drop`). |
| Multi-line (tilde) | `~~~prompt`...`~~~` | Fenced with `~~~prompt` opener |
| Multi-line (dash) | `---`...`---` | Fenced with bare `---` |
| Start fence | `--- start [at <datetime>]` | Activation signal (consumed on use) |
| Stop fence | `--- stop` | Breakpoint (consumed when reached) |

**Attributes:** `<!-- agent:queue auto -->` is accepted as a legacy/manual immediate activation hint when the queue is non-empty. Binary-owned queue writes must not create `auto`; when route or maintenance touches a legacy `agent:queue auto` tag, it strips `auto` while preserving other attributes and uses `queue_active: true`, `--- start`, or explicit exchange triggers for activation. `auto` and `preset` are **queue-only**: placing either on any other component (e.g. `agent:backlog`) does not activate the auto-loop and triggers a `misplaced_component_attr` preflight warning (see §2.4). `preset` on the queue tag (e.g. `<!-- agent:queue preset="#spec-test-build-install-commit-push" -->`) is recognized as a metadata annotation; the actual preset directive lives in the queue body as a `preset #name` line.

**Activation resolution (preflight):** Preflight detects the `agent:queue` component and resolves activation in priority order:

1. **Legacy `auto` attribute** — `<!-- agent:queue auto -->` activates immediately when prompts exist, but binary-owned writes must strip it from any queue tag they touch.
2. **Start fence at head** — bare `--- start` is consumed and activates; `--- start at <time>` defers (emits `queue_deferred: true`, `queue_start_at`).
3. **Exchange trigger** — user writes `do queue` or `run queue` in the exchange.
4. **Persisted state** — `queue_active: true` in frontmatter (set on activation, cleared on drain).

On activation, preflight emits `queue_active: true`, `queue_prompts: [...]` (ordered prompt texts), and `queue_trigger` (how the queue was activated). The first prompt is the effective user edit for the cycle.

When the queue drains to empty: `auto` is stripped from the opening tag, `queue_active` is cleared in frontmatter.

**Consumption (Phase 3/4):** After a successful response write (via `finalize` or `write --commit`), strict closeouts first clear the remaining pending-maintenance / pending-guard gates. Only then is the consumed prompt marked complete in the `agent:queue` block as `- ~prompt text~` before the commit boundary so the same git commit can capture both the response and the queue advance. Queue consumption requires one of three prompt proofs: the current diff contains the exact queue-head prompt, the run path supplied a queue-synthetic diff for the active head, or the closeout explicitly resolved the head `do #id` with `--done <id>`. An empty baseline/current diff, or an unrelated prompt such as `#next-steps` that was already present in the baseline, must not consume the active head by itself. Missing-response materialization repairs, including `write --commit` with empty stdin and Codex Stop-hook recovery, preserve the current queue head and `auto` attribute unless the repair carries explicit head proof such as a matching `--done <id>` or `### Re:` topic. If the closeout explicitly resolves multiple contiguous queued `do #id` prompts with repeated `--done <id>` flags, those head prompts are consumed together; consumption stops at the first unresolved or non-matching queue prompt. Queue-head proof normalizes leading priority markers (`:pushpin:`, `:round_pushpin:`, and their aliases) before classifying `do [#id]`, so a pinned id-backed head is not treated as free text and cannot be consumed merely because an unrelated response exists. The file and snapshot transforms are both proven first, then written in sync so change detection works on the next cycle.

- **Auto-strike of resolved head prompts (preflight maintenance):** Before dispatching the head, preflight walks leading `Prompt` entries and rewrites any whose `#id` is already resolved into `Completed` (`~text~`) entries. An id counts as resolved when present in `agent:done` (canonical) **or** in `agent:review` with the pending-gate marker `- [/]` (code-complete, awaiting an external gate). The pending-gate inclusion unblocks queue stalls when a do-item lands a contained phase of a multi-phase plan but the parent item stays gated for remaining phases. Strike scanning stops at the first head prompt whose id is not resolved so the next live head stays intact for the normal consumption path. The strike telemetry log line records `source=done` vs `source=review_gated` so audit can see which path advanced the queue.
- **Spent prompt-preset pause repair (`#qpresetstrike`):** Controller dispatch must not replay a stale `queue_paused` receipt whose reason says `"<preset> preset head is spent"` after the live queue no longer contains that preset token. On dispatch it revalidates the pause against the current document: absent head clears the pause and proceeds; present registered preset head is consumed through the normal queue consumer, then the pause is cleared. Non-spent operator pauses remain fail-closed.
- **Stale-supervisor queue pause recovery (`#jbrestale`):** A `queue_paused` churn-stop caused by a stale route-owned supervisor is recoverable exactly once. Current controllers tag the bail with `supervisor_restart_redirect`; route also recognizes the legacy markerless `failed_stage=queue_paused reason=#qchurn ... stale host supervisor pid<N> ...` shape so an old supervisor can be restarted and retried instead of surfacing a JetBrains `Run Agent Doc` error.
- **Backlog→queue sync is activation-scoped (`#backlog-queue-sync-pending-add-amplification`):** when an `agent:backlog` carries the `queue` attribute, preflight mirrors its active ids into `agent:queue` only while the queue is **not yet a persisted-active auto-loop** (`queue_active` was not already `true` coming into the cycle). Once the loop is running, a freshly-added backlog id (e.g. a follow-up captured via `--pending-add` mid-loop) is **held out** of the live queue — it joins on the next activation — so capturing follow-ups cannot grow the running queue unboundedly or trip `pending_done_guard` each finalize. Ids already present as queue heads are unaffected. `agent:icebox queue` is ignored with a warning; use per-item enqueue or move the item to backlog.
- **`go` opts into continuous-backlog-loop on a drained queue (`#backlog-queue-empty-active-repopulate`):** the activation-scoped hold above has one exception. When the queue carries the `go` control (frontmatter `queue: go` or a marker-side `go`/`start` token, both → `QueueControl::Start`) **and** the live queue is **fully drained** (0 un-struck prompts), preflight repopulates from the full active backlog instead of holding — so a `go` queue keeps working the backlog continuously. Without `go` (a plain persisted-active queue), a drained queue stays drained (**drain-then-stop**). The repopulation is amplification-safe because the queue is empty, and self-terminating because only `Open` (`[ ]`) backlog items are mirrored: items processed and marked `[/]` (gated) or `[x]`/`agent:done` per the `do #id` closeout rule drop out, so the loop ends when no `Open` backlog item remains. If both queue and backlog are empty, `go` does not promote parked icebox work.
- **Boundary-displacement repair (`#queue-completed-items-escape-below-component`):** struck queue items must stay inside the `agent:queue` component span (struck, until reaped to `agent:done`). A post-commit CRDT/boundary merge can displace a struck `- ~~…~~` queue line past `<!-- /agent:queue -->` into the neighbouring parking-lot HTML comment, where it renders invisibly and accumulates as orphaned residue. `template::repair_queue_struck_items_escaped_below_marker` (run by preflight queue maintenance and by the visible-write normalizer) removes any struck queue-shaped line — body carrying a `:round_pushpin:`/`:pushpin:` marker or a `do`/`re` directive — that sits **outside every agent component span** below the queue close marker. Real component content, legitimately struck items still inside the queue, and ordinary scratch-comment text (generic `- ~~note~~` with no pushpin/directive) are left untouched.
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
