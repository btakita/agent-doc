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
- `subagent_model`: Default model override for queue-dispatched subagents. A selected queue's `model=` attribute, then applicable structured preset metadata, takes precedence.
- `subagent_effort`: Default reasoning-effort override for queue-dispatched subagents. A selected queue's `effort=` attribute, then applicable structured preset metadata, takes precedence.
- `branch`: Reserved for branch tracking
- `terminal_host`: Terminal presentation override — `auto`, `ide`, `external`, or `none`. Resolution then falls back to project and global `[terminal].host`; it does not name the tmux session.
- `agent_args`: Additional CLI arguments for the active agent process (space-separated string)
- `claude_args`: Additional CLI arguments for the `claude` process (space-separated string, see §6.1)
- `codex_args`: Additional CLI arguments for the `codex` process (space-separated string, see §6.1)
- `opencode_args`: Additional CLI arguments for the `opencode` process (space-separated string, see §6.1)

All fields are optional and default to null. Resolution: explicit `agent_doc_format`/`agent_doc_write` > deprecated `agent_doc_mode` > defaults (template + crdt). The body alternates `## User` and `## Assistant` blocks (append format) or uses named components (template format).

## Frontmatter Parsing

Delimited by `---\n` at file start and closing `\n---\n`. If absent, all fields default to null and entire content is the body.

### Project preset defaults

An optional `.agent-doc/project.md` supplies project-wide prompt preset defaults
using the same YAML frontmatter syntax as a session document. Only its
`prompt_presets` map (including the `presets` alias) is inherited. Its body and
all other frontmatter fields are inert for session documents; project routing,
session identity, harness choice, and write policy remain document-local.

Project preset resolution is a read-time overlay, not template expansion or
synchronization:

1. Starting from the session document, use the established nearest
   `.agent-doc/` project-root walk. If no project root or no
   `.agent-doc/project.md` exists, preserve the document's current behavior.
2. Parse the project file's frontmatter and use its resolved presets as
   defaults. A present but unreadable project file or malformed frontmatter is
   an error; silently ignoring it would make authorization depend on which
   resolver happened to run.
3. Overlay every declaration from the session document. A local declaration
   with the same name wins, including a null/in-progress or malformed local
   value: declaration itself shadows the inherited default and prevents an
   older project authorization from resurfacing while the operator edits it.
   Within either file, the existing alias rule still applies:
   `prompt_presets` wins over `presets` on a same-file name collision.
4. Resolve consumers from the effective overlay on every read. Changes to
   `.agent-doc/project.md` therefore apply to existing project documents
   without rewriting them. Ordinary parse/write and closeout paths serialize
   only the session document's local frontmatter; inherited presets are never
   copied into it.

Structured inherited presets retain their project-relative runbook semantics.
This is intentionally limited to presets; sharing more frontmatter requires a
separate field-by-field contract rather than implicitly inheriting every key.

## Components

Documents can contain named, re-renderable regions called components:

```html
<!-- agent:status -->
content here
<!-- /agent:status -->
```

Marker format: `<!-- agent:{name} -->` (open) and `<!-- /agent:{name} -->` (close). Names must match `[a-zA-Z0-9][a-zA-Z0-9-]*`. Components are patched via `agent-doc patch`.

An opening marker may share a line with any ATX heading, for example
`## Run <!-- agent:run priority subagents -->` or
`## Queue: Main <!-- agent:run id="main" subagents priority go -->`.
This holds for every component kind, and the heading
text need not match the tag name. The rule stays strict:

- The prefix is an ATX heading and nothing else: up to three spaces of
  indentation (no tabs), one to six `#`, a space or tab, then any heading text,
  which may be empty (`## <!-- agent:run -->`). `##Queue`, `####### Queue`,
  list items, blockquotes, and other prose before a marker are not headings.
- The heading text holds no other HTML comment (`<!--` or `-->`) and leaves no
  inline code span or double-quoted span open at the marker. An open span would
  make the element parser treat the marker as prose while a line scanner still
  saw an opener, so such a prefix is refused for all of them.
- Only whitespace may follow the marker on its line.
- Only opening markers qualify. A closing marker after a heading
  (`## Done <!-- /agent:run -->`) is `non_standalone_component_marker:/agent:run:close:lineN`.
- A marker line inside fenced or indented code is content, heading or not
  (`#fencedmarkerscan`).
- The heading text is never part of the component body: the body starts after
  the opener, and the heading stays verbatim on its line through parse, the
  template-structure guard, merge, and commit.

Through 0.35.554 the heading text had to equal the tag name (`## Run` with
`agent:run`). On 2026-10-26 the JetBrains template-structure guard refused every
save of a document carrying `## Queue: Main <!-- agent:run id="main" ... -->`
363 times as `non_standalone_component_marker:agent:run:open:line836`, so the
editor and disk diverged until the operator split the line. Other prose welded
before or after a marker remains structural corruption.
One predicate owns this heading-anchor rule
(`agent_doc_element::element::component_heading_prefix_matches_tag`), and
every opener scanner calls it: the element parser, the structural-corruption
gate, the markdown-AST overlay (`agent_doc_markdown_ast::overlay::parse_open_marker_line`)
that `respond`, queue consumption, and node-keyed mutations resolve
components through, the document-cell merge body scan, and the CRDT relay's
marker-line removal guard (`#markerlineedit`). The JetBrains plugins'
template-structure guard calls the same Rust rule through the
`agent_doc_normalize_template_structure` FFI, so it keeps no Kotlin copy. A heading-anchored
opener therefore exists for all of them or for none: `respond` must never
report ``component `queue` was not found`` for a document whose
`read --component`, `patch`, and `lint` accept that component (GH #374).
Line scanners that match raw tag names and then look a component up in the
overlay map the raw name through `overlay::projected_component_name` first
(`run` and `loop` project to `queue`), so an `agent:run` component merges by
node identity exactly like `agent:queue` instead of being dropped.
The same holds for `--backlog-add` (GH #378). Its same-cycle backlog→queue
mirror has one target, `agent_doc_queue::backlog_sync::mirror_queue_component`
(the first non-repeating `agent:run`, never an `agent:loop`), and every step of
that mirror reads that same component: the `go`-mode gate, the pre/post id diff,
and the recovery-baseline adoption that the closeout commits from. Reading the
first `queue` component instead picks a sibling `agent:loop` above the run,
which silently dropped the mirror (or committed the backlog row without its
queue row) whether or not the openers are heading-anchored. The controller's
queue-completion projection, which every `commit_document` derives, resolves
components through the overlay, so a heading-anchored `agent:run` whose
heading text does not name the tag is found there too, and a document with no
runnable component projects no completed queue heads rather than failing the
commit with ``component `queue` was not found``.

**A component ends only at its own close (`#componentownclose`).** Each
`<!-- agent:NAME ... -->` opener's span ends at the next unmatched
`<!-- /agent:NAME -->` with the same raw name. A close that names a different
component never ends the open one, and the open one never extends past it into
a sibling. An opener with no close, a close that names another component
(`opened 'agent:loop' but closed '/agent:todo'`), and a close with no opener
are all component parse failures
(`agent_doc_element::element::component_parse_failure`). Every path that
rewrites, merges, or adopts a document fails closed on a parse failure. It never
guesses a span:

- The strict element parser returns the error with both marker lines. Every
  span rewrite built on it (the loop restore, queue writes, `patch`) refuses.
- The markdown-AST overlay finishes a component left open by a foreign close as
  dangling at that close. Rows after a sibling's close are never collected as
  its items.
- `agent_doc_merge::merge` refuses when the operator side, the agent side, or
  the engine's result has a parse failure (`MergePlan::structure_refusal`). It
  then sets `fell_back` and hands back the operator text unchanged. The span
  merge would otherwise pair the opener with end-of-document, buffer every
  following component as its body, and re-emit a sibling after them.
  Preflight admission logs `component_structure_refused` and proceeds on the
  authority.
- Document authority never moves to a structurally invalid buffer. A fresh
  controller's retained editor reseed (after a recycle) stays behind its
  reseed fence while the reseeded text has a parse failure. The text is not
  readable as current text and does not pass the commit barrier, so retained
  persistence, snapshots, and commits cannot adopt it. The hub keeps absorbing
  the editor's ops, so the reseed
  completes on the first op after the operator repairs the markers. A live
  update that removes a marker and breaks the pairing keeps the last valid
  canonical (`#replica-structure-guard`).
- Each refusal is surfaced to the operator at ingest, with the markers, lines,
  and repair: `crdt_replica_reseed_structural_refused` or
  `crdt_replica_update_component_markers_removed` in ops.log. The binary also
  writes `.agent-doc/state/editor-route-errors/<doc>.txt`, which the JetBrains
  plugin shows as the editor's `⚠ agent-doc:` status. Promotion clears it.
  agent-doc never reverts the operator's text to repair it. The editor buffer
  stays authoritative, and the guidance points to the marker lines or to
  `git show HEAD:./<doc>`.

On 2026-10-26 one editor transaction in `agent-doc-bugs.md` deleted 4,596
bytes. The span ran from just after the loop's Watch row up to
`<!-- /agent:todo -->`, and it took `<!-- /agent:loop -->`, `## Todo`,
`<!-- agent:todo -->`, and six todo items. The IDE autosaved the result. An
install recycle then started a fresh controller, and the re-registered editor's
reseed promoted that buffer to canonical (`retained_persistence_applied ...
proof=exact_disk`). The reseed happened even though the durable ingest was
`StructuralQuarantined`. The regression fixture
`agent_doc_element::sibling_span_fixture` replays that shape in both opener
spellings for the parser, overlay, merge, loop restore, preflight admission,
relay hub, and the recycle and re-registration sequence.

The document-cell merge pairs each component across base, agent, and operator
by **component identity**, never by name alone: the projected name, the
effective `id=`, and the occurrence among that side's components with the same
name and effective id. The effective id is the explicit `id=` value (a bare
`id`, an empty value, or `true` is no id); a lone unkeyed queue has the
legacy queue identity `default`, so `<!-- agent:run -->` and
`<!-- agent:run id=default -->` are one component; any other unkeyed
component has no id and pairs by document-order occurrence among the unkeyed
components of its name. Pairing is exact: a component is never merged against
a same-name component with a different id, and one with no counterpart is
classified on its own side (operator-only components keep their operator
items; agent-only components are carried over). Each operator opener resolves
to the overlay component that starts on that line, so in a document with
`agent:run id="main"` and `agent:loop id=issues` each queue's items merge
against their own queue's items, including when the operator reorders the
queues, and an item id shared by both queues stays two independent nodes.
Before, the merge took the first component of the projected name on every
side, so the second queue was merged against the first one's items.

**Fenced markers are content for every line scanner (`#fencedmarkerscan`).**
A component marker line that starts inside fenced or indented code is
content, never an opener or a closer. Line scanners that walk component
markers (the Markdown-AST overlay's `components`, the document-cell merge's
body scan, its exchange-note injector, and its exchange-turn scan) classify
marker lines through one shared rule, `overlay::ComponentMarkerScan`, instead
of keeping their own opener/closer recognizers. So a queue row followed by a
pasted report whose fence holds a bare `<!-- agent:run priority subagents -->`,
a heading-anchored `## Run <!-- agent:run ... -->`, or a bare
`<!-- /agent:run -->` line stays in the one queue the element parser and
overlay see, on both sides of the fence. The merge rebuilds a non-exchange
component by splicing each operator item node at its own source span (a
multiline queue prompt included) and keeping every other operator line
verbatim. Before, a fenced `<!-- /agent:run -->` line ended the operator's
queue early in the merge scan, so the merged item set (with the agent's
strike) was emitted there and the operator's remaining lines, including the
unstruck copy of the row, were passed through again; a fenced
`<!-- agent:queue -->` example in prose opened a component that swallowed the
real queue after it; and a multiline queue prompt was emitted twice.

**A queue row's node id is its work identity (`#queuerowworkidentity`).**
The Markdown-AST overlay derives an item's id from the row's work text:
`agent_doc_element_queue::strip_attempt_projection_prefix` first removes the
leading binary-owned attempt projection (lifecycle markers such as `☑️`,
`🔄`, `📥`, `❔`, pins, the `[#<owner>#<generation>]` owner annotation and
its legacy `⟦…⟧` spellings, and the bare `🙋` glyph), and the id is the first
`[#id]` of the remainder, else the content hash of the remainder. That helper
and `strip_priority_markers` share one projection-token grammar. So a claimed
`☑️ [#subagent:x#1] do [#foo]`, each lifecycle re-projection of it, and its
bare or struck `~~do [#foo]~~` copy are one node `foo`, and a claimed
free-text row has the same content id as its bare text and is not an
explicit id. `Item::text`, `raw`, `kind`, and the strike/pin flags are
unchanged; only the identity moved. Before, the first `[#...]` token of a
claimed row was its owner tag, so the claimed row and its struck copy were
different nodes in the document-cell merge, and an editor copy carrying
another marker survived beside the struck row or replaced it. The merge then
treats the projection as binary-owned, not operator-authored: an operator copy
that differs from base only in the projection (same work text, strike, and
pin flags) leaves the node unchanged, so the agent's strike, removal, or newer
projection stands. An unstruck operator copy that still carries a projection
and names the same work as a struck row is a stale pre-strike projection, so
the struck row (or the agent's drop of it) stands. An operator who un-strikes
a row writes the bare work text, and an edit of the work text is an operator
edit; both still win.

Compatibility: overlay ids are a pure function of the current text. Every
merge, node-keyed mutation, and event diff recomputes them on all sides with
one binary, and the persisted overlay projection stores markdown, not ids. A
row without a leading projection keys exactly as before. A row with one now
keys as its bare and struck forms already did, so the claim and strike phases
no longer change a node's key. The per-cell CRDT's `list_item_key` already
skipped owner annotations and stripped lifecycle markers, so persisted cell
keys (`queue:0:id:<id>`, `queue:0:txt:<work text>`) do not change and
replicas need no migration. A node patch addressed to an old owner-tag key
fails closed with `NodeNotFound`.

`agent:run` is the canonical runnable-work component. The former
`agent:queue` spelling remains a backward-compatible alias with identical queue
semantics, attributes, node identity, and mutation behavior. Agent Doc accepts
and preserves either spelling in an existing document, but every newly created
component and scaffold uses `agent:run`.

**Inline attributes:** Open markers support inline attribute overrides: `<!-- agent:name patch=append -->`. `mode=` is accepted as a backward-compatible alias; `patch=` takes precedence if both are present. `max_lines=N` trims component content to the last N lines after patching (0 or absent = unlimited). Precedence chain: inline attribute > `.agent-doc/components.toml` > built-in default (`replace` for patch, unlimited for max_lines).

**Attribute validation (preflight):** Recognized attribute keys are `patch`, `mode`, `max_lines`, `archive`, `transfer-source`, `timestamp`, `broken`, the status-only `sync` and `sync-mode`, the queue-only `auto`, `start`, `go`, `stop`, `id`, `after`, `clear`, and legacy `disable-auto-clear`, the queue schedule attributes `time` and `loop`, and the overloaded `interval` attribute on `agent:run` or a synchronized `agent:status` (`#queueschedule`; an invalid queue schedule value is a non-blocking warning and is dropped, see §2.5 Queue schedule), the prompt-component `preset`, `subagents`, and `fan-out` on `agent:run` or `agent:exchange`, the backlog/pending-only `queue` (see §2.5 backlog→queue sync), and the backlog/icebox/review/todo/queue `priority` ordering attribute (`#backlog-priority-attribute`, `#reviewpriority`; per-item priority lives in `priority=<1..9>` item tokens). Preflight emits a non-blocking `misplaced_component_attr` warning when a queue-only attribute appears on a non-`queue` component (e.g. `<!-- agent:backlog auto -->`), when `interval` appears outside `agent:run`/`agent:status`, when a prompt-component attribute appears outside `agent:run`/`agent:exchange`, when a status-only attribute appears outside `agent:status`, when `sync-mode` or status `interval` has no nonempty `sync` instruction, when the backlog `queue` sync attribute carries an unrecognized mode (e.g. `<!-- agent:backlog queue=nope -->`; valid: bare `queue` = append, `sync`, `append`, `prepend`), when `queue` appears on `agent:icebox`, or when an unrecognized key appears on any component (e.g. the typo `auot`). A misplaced recognized bare token is preserved and ignored, so the tagpath lint adapter applies the same warning-only verdict rather than reclassifying it as a malformed value attribute. Other malformed attributes remain blocking, and an opening-marker finding that closeout does not rewrite must fail the pre-write gate before response mutation. The attribute is never mutated by this check and the warning exists so a misplaced/misspelled attribute is surfaced instead of silently tolerated. This closes `#backlog-auto-marker-misfire`, where `auto`/`auot` on the backlog marker had previously been parsed and ignored without feedback. A legacy/manual queue can activate from `<!-- agent:run auto -->` (see §2.5), but binary-owned route writes must never add `auto` and should strip it from touched queue tags; the `queue` attribute on a backlog/pending component only populates `agent:run` (`#backlog-queue-sync-attr`), it does not start the loop. Icebox work must be moved to backlog, placed in `agent:run` manually, or marked with a per-item enqueue token before it can run.

**Persistent status synchronization.** `<!-- agent:status sync="Keep the published release current" interval=5m -->` is an operator instruction that preflight emits on every cycle as the structured `status_sync` contract. Claude Code and Codex also re-surface it once at the owner turn's final `Stop` boundary, after queue and response-persistence obligations have settled, because preflight cannot prove that a fact which changes during the turn is current at handoff. The final checkpoint reads the live editor-authoritative document cut, is bounded by the harness `stop_hook_active` recursion bit, and tells the agent to make no write when the answer is unchanged. If it changed after the response transaction closed, the agent uses `agent-doc status set <FILE> --text "<current result>"` followed by `agent-doc commit <FILE>` so the focused correction is durable; the recursive Stop then passes through without asking again. Periodic **mid-turn** checkpoints default to off: `sync` without `interval` is still checked at preflight and final Stop, but repeated owner-tool prompt injection stays disabled. When `interval` has a value, an open owner turn additionally re-surfaces the contract no sooner than that interval after preflight and at most once per following interval, so a long-running turn can reconsider status without paying per-tool prompt cost. This is hook-driven rather than a background timer or writer: it cannot interrupt one continuously running tool call, but when an interval is overdue, the next completed owner tool call surfaces the reminder immediately. `interval` accepts a unitless number of seconds or a compound `s`/`m`/`h`/`d` duration such as `5m` or `1h30m`; values below `1m` fail closed because a faster prompt cadence would amplify token use. The checkpoint is additive to ordinary mid-turn steering, is suppressed for subagent tool calls and closed/captured cycles, and only prompts the active agent; it never writes the document itself. Each completed owner-tool hook publishes a typed boundary fact to the controller-owned, process-scoped Lazily graph. Its Sources retain the current cycle/cadence observation and controller receipt, its Computed derives the cycle-and-cadence-keyed `status_sync_due` edge, and the controller transition atomically consumes that edge before answering the hook. SQLite is not the live timing or arbitration authority. Editing the cadence during a live cycle forms a new generation whose first due edge is anchored to the cycle start. The instruction is persistent marker metadata: updating the status body never removes or rewrites `sync`, `sync-mode`, `interval`, or any unrelated opening-marker attribute. When the answer changes mid-turn, the agent uses `agent-doc status set <FILE> --text "<current result>"`, a focused editor-aware write that attributes only the status delta to the document baseline and leaves the response cycle open (no capture, seal, response append, queue mutation, or commit). At closeout, `write --status` remains valid as part of the response transaction. An unchanged result performs no write. A bare, blank, duplicate, malformed, too-short, or overflowing status `interval`, or a status `interval` without `sync`, fails closed before status mutation.

The default `sync-mode` is `additive` (and `sync-mode="additive"` is accepted explicitly). Additive updates own exactly one neutral nested projection delimited by `<!-- status:sync -->` and `<!-- /status:sync -->`; the delimiters are not `agent:*` components. The update inserts the block when absent or replaces only its body when present, preserving every byte of unrelated status prose and binary-owned operational lines outside it. Repeating the same update is byte-idempotent. At most one balanced managed block is allowed.

**Structured status-sync writes (`#statussyncnest`).** Every status writer (`status set`, closeout `write --status`, and a response `patch:status`) treats the managed block as a node of the status body's markdown block AST (tree-sitter-md), never as text to splice around: a delimiter counts only as a top-level-flow `html_block` whose whole text is the marker, so delimiters inside fenced code, block quotes, list items, or inline prose are ordinary content. The incoming `--text` is parsed the same way. Plain text is the block payload. Text that carries its own `<!-- status:sync -->` block is a full status body: its block payload replaces the managed payload and the prose around it becomes the free-form status, while the existing binary-owned operational lines (`📤 Queue handoff:`; legacy `📥 Queue handoff:` migrates to it, the stale-supervisor marker, `[agent-doc sync|merge|repair]` lines) are retained and the caller's copies of them dropped, so the caller's delimiters are consumed and can never be nested inside the existing block (on 2026-10-20 an additive `write --status` whose text was the whole status body wrote that body, delimiters and all, as the payload of the existing block, nesting a second block and duplicating its paragraph). Writers self-heal: a nested, duplicated, unclosed, or orphaned structure already in the document is folded into exactly one block before the write lands, keeping the latest innermost payload, moving free-form prose that leaked inside a wrapping block back outside unless the outside prose already has its lines, and dropping superseded payloads and orphaned delimiters. Preflight itself stays fail-closed on such a structure (it never normalizes the document into a new baseline) and its refusal names the binary-owned repair `agent-doc status repair <FILE>`.

`sync-mode="replace"` is the explicit destructive alternative. It updates the same managed block and may remove ordinary status prose, but it retains binary-owned operational paragraphs, including the `📤 Queue handoff:` projection (and its legacy `📥` spelling until migration), the stale-supervisor `🔴 (restart/recycle your supervisor)` marker, and `[agent-doc sync]` / `[agent-doc merge]` / `[agent-doc repair]` (plus `[sync]` / `[merge]` / `[repair]`) warnings and their diagnostic paragraphs. A blank `sync`, an unsupported `sync-mode`, or a duplicate/unbalanced managed block fails closed before status mutation; the previous status remains unchanged.

**Code range exclusion:** Component marker detection uses pulldown-cmark for CommonMark-compliant code range detection, replacing the previous regex-based approach. Markers inside inline code spans or fenced code blocks are excluded and never treated as component boundaries. A same-line backtick pair of any run length always quotes what it encloses, independent of the surrounding block (HTML blocks, block quotes, list items, bold), so prose that names a marker — e.g. ``- **`<!-- agent:run disable-auto-clear ... -->`** turns it off`` — is never parsed as a component opener (`#inlinecodemarker`). Every marker scanner (component parser, boundary/patch scanners, structural-corruption gate, queue/backlog parsers, lint) consumes the same `find_code_ranges` / `find_marker_prose_ranges` exclusion instead of re-implementing it.

**Parse refusals name exact tags and lines:** an unclosed opener reports `unclosed component: <!-- agent:NAME --> without matching close (opening marker at line N)`, and invalid, unmatched, or mismatched tags report their exact source spelling (`agent:run`, not its internal `queue` projection) plus the offending line. Structural-marker refusals likewise derive their line from the marker in the merged target rather than from another component or stale source cut. Response landability is checked before capture (a live marker in a response body is refused with its payload line and the `quote marker text in backticks` remedy), and the commit gate refuses any document whose components fail to parse (`commit refused structurally corrupt document ...`), so a closeout never commits a document its own parser rejects.

**Structured semantic CRDT:** The markdown AST crate owns the component/node schema used by Lazily current state. Each component and item has stable semantic identity, spans, kind metadata, and operator/agent provenance. The live structure exists only in Lazily replicas; cold structured recovery checkpoints are stored in `state.db`. No legacy whole-document Yrs root or overlay file is imported, and no recovery checkpoint may become a merge base ahead of Lazily current text.

**Node-keyed mutations:** The markdown AST crate exposes component mutations keyed by semantic item identity plus occurrence, not absolute byte offsets: consume marks a node struck, dedup removes duplicate id-backed nodes while preserving free-text duplicates, reorder moves items by node key, and enqueue inserts relative to an existing node key. These APIs are the phase-4/5 bridge away from text-line heuristics; callers should treat pin markers and strike markers as node attributes, not identity text.

**Duplicate queue-effect annotation (`#queueeffectidentity`):** When lifecycle
convergence proves that two or more live, single-line free-text queue rows are
lawful operator-authored occurrences of the same normalized text, the binary
appends a visible `⟦effect:N⟧` suffix to each occurrence. The positive integer is
an occurrence identity, not operator prose. It remains part of exact claim,
worker, completion, and receipt identity; lifecycle/priority markers remain
cosmetic. Existing positive ids are stable, missing ids receive the lowest unused
integer in queue order, and a lone survivor is never renumbered. Id-backed and
multiline heads are outside this projection.
Annotating a claimed row never moves its claim to a text-identical sibling
(`#claimrowmigrate`): the claim follows its own row's annotated successor,
identified by its anchor and, where text alone is ambiguous, by the row's
binary-written `[#owner#generation]` projection. Full rule:
`07-orchestration-commands.md` § Duplicate queue-effect identity.

**Durable operation log (`#op-scoped-drift-1`):** Phase 1 of the operation-scoped drift model (`tasks/agent-doc/plan-operation-scoped-drift.md`) persists each cycle's node-keyed operations to derived tables in the controller's sole `.agent-doc/state.db`, tagged with **actor** and a **causal clock**. The actor (`agent_doc_turn::op_log::OpActor`) is one of `agent`, `user`, `foreign_supervisor`, or `live_buffer`; `classify_actor` maps an `OpSource` provenance signal to the actor. At preflight the diff is snapshot↔document, so every observed node op is a `user` edit — the agent's own committed output already lives in the snapshot. The causal clock (`agent_doc_turn::op_log::CausalClock`) carries a monotonic per-document Lamport tick plus the originating `agent_doc_session`. The durable store (`agent-doc-sqlite::op_log`) owns Lamport assignment: each appended op receives the next per-document tick so the log is totally ordered per document; a repeated preflight pass over the same uncommitted diff is idempotent (no duplicate row, clock unchanged) because append dedupes against the most recent op for the same node. Writes are best effort — the op-log tables are rebuildable derived state and a write failure never blocks a cycle. This op log is the substrate the later phases read: phase 2 emits the TurnScope read/write set, and phase 3's affectedness classifier intersects incoming ops against that scope so independent ops integrate and persist without affecting the current turn.

**TurnScope manifest (`#op-scoped-drift-2`):** Phase 2 emits an operation manifest for the current turn in preflight output as `turn_scope`. An `Address` (`agent_doc_turn::turn_scope::Address`) names a component occurrence, optionally narrowed to a `node_key` (a `node_key` of `null` addresses the whole component). A `TurnScope` has a `driver` (the queue node the turn answers, resolved from `prompt_targets`), a `read_set`, a `write_set`, and an `exchange_tail_floor`. `TurnScope::for_driver` builds the canonical scope: read `{driver, exchange tail}`; write `{exchange append, driver strike, backlog, status, review, done/archive, gitlink, editor writeback}` — the driver appears in the write set because the turn strikes the queue item it consumes. Queue remains node-scoped through the driver address, so sibling queue inserts are independent while edits to the running queue item affect the turn. The named output surfaces are component-scoped: unrelated component edits merge automatically, while same-component incompatible edits become visible conflicts. `done_archive`, `gitlink`, and `editor_writeback` are runtime surfaces addressed by the same classifier even when they are not markdown `agent:*` components. When no driver node resolves (a non-queue prompt, or an id absent from the queue) the manifest still lists the output components every turn touches but `driver` is `null`. The manifest is the substrate the phase-3 affectedness classifier intersects incoming ops against; emitting it never blocks a cycle.

**Exchange-tail node granularity (`#loop-guard-exchange-node-granularity`):** the `exchange` component is whole-component in `read_set`/`write_set`, but only its *active tail* affects the turn. `TurnScope::for_driver_with_exchange_tail` records `exchange_tail_floor` = the count of `exchange` item nodes present at turn start (computed by preflight from the document). An incoming `exchange` op affects the turn only when its within-component node index is at or above the floor — a tail append (a genuine mid-loop user prompt, which lands at index ≥ floor) or a tail edit. An edit to an OLD exchange block (index below the floor — committed history) classifies `independent` and must not preempt the auto-loop drain. The classifier receives each op's index via `DocumentOp::node_index` (after-index for inserts/replaces, before-index for removes; preflight fills it from the semantic node event). When no `exchange` nodes exist the floor is `null` and the classifier keeps its coarse whole-component behavior.

**Native choice records (`#choicecheckpoint`).** A harness question UI is mirrored
into `agent:exchange` before it opens by `agent-doc choice-checkpoint`. The record
uses the cycle-keyed `agent:salient-response` node and renders only a bounded,
single-line public context, the exact question, and 2–5 uniquely labelled
choices. It is an assistant-authored non-final node: exchange parsing must never
classify it as an operator prompt, and prompt/queue completion logic must never
use it as answer evidence or authorization. Final response insertion removes the
node; the normal response retains the presented choices and selected answer as
the durable decision record.

**Affectedness classifier (`#op-scoped-drift-3`):** Phase 3 replaces the coarse "any divergence affects the turn" assumption with a scope-intersection router, surfaced in preflight output as `op_affectedness`. `Address::overlaps` is the `conflict` primitive: two addresses overlap when they share a component **name** and either side is component-level (a whole-component address) or both name the same node key. The `occurrence` field is informational and is intentionally not part of the match — a node key already encodes the component index (`comp:index:id:n`), and the canonical write-set members built by `TurnScope::for_driver` are whole-component addresses that must match the component wherever it sits in the document (`#nm1x`). `classify_op(actor, op_kind, address, node_index, scope)` routes one op into the 5-class taxonomy — `affects(O,S) = conflict(O.target, read_set) ∨ conflict(O.target, write_set)`, with the exchange-tail floor narrowing `exchange` membership to the active tail (above):

1. **independent** — target ∉ scope: integrate + persist, no drift (a queue item inserted beside the running one, a comment/icebox edit).
2. **input_affecting** — target ∈ read_set: re-read the affected input (the user edits the driver item being answered).
3. **output_contended** — target ∈ write_set with a concurrent writer: CRDT merge (two supervisors append the exchange boundary).
4. **structural_dependency** — `remove`/`move` of a depended node: invalidate/adapt (the user deletes the running queue item).
5. **provenance_spoofed** — a recovery or reload projection presented as a user-originated operation: suppress it and re-apply Lazily current state.

`AffectednessClass::affects_turn` is true only for classes 2/3/4; classes 1 and 5 integrate or are suppressed without disturbing the turn. `classify_cycle` classifies every node operation with its explicit actor and intent provenance. Independent and provenance-spoofed operations no longer count against the turn; emitting the classification never blocks a cycle.

**Scope-aware finalize-path drift gate (`#nm1x`):** preflight persists the derived `TurnScope` in the document runtime row of `state.db`, so a later process can intersect incoming document ops against the same scope without a second filesystem authority.

1. **Commit/absorb gate** — the persisted `TurnScope` row classifies node-level changes between the captured semantic baseline and Lazily current state. Out-of-scope insertions integrate; read/write conflicts, structural dependencies, or unexplained structural drift rebase or fail closed. A missing scope row uses the conservative classifier without consulting a file sidecar.
2. **Lazily generation gate** — before apply, the binary compares the captured generation with Lazily current and rebases the node-keyed agent intent. Unsaved operator edits and deletions remain authoritative. Disk equality is projection evidence only and never substitutes for an attached editor generation.

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
| `notes` | merge/preserve | Operator-owned informational context — preserved for real turns but content-only edits never start a turn |
| `todo` | replace (guarded) | Operator-owned action checklist — unpublished drafts are a common item; content-only edits never start a turn or enter the agent queue |
| `output` | replace | Latest agent response only |
| `input` | replace | User prompt area |
| unregistered `agent:*` | merge/preserve | Unknown/pluginless components use the `agent-doc-element-unknown` fallback: preserve operator-visible content and do not perform semantic mutations until a descriptor is registered. |

Per-component behavior is configured in `.agent-doc/components.toml` (see §7.21).
Built-in element descriptors live in the `agent-doc-element-*` crate family;
the built-in registry is `agent-doc-element-registry`. Each descriptor owns an
explicit turn role. Unknown components remain turn triggers by default so an
unregistered prompt cannot be silently ignored; `notes` and `todo` are the
built-in informational exceptions. The registry also lists the binary-written
components that only ever appear nested inside another element and have no
descriptor of their own (`NESTED_BINARY_COMPONENTS`: `salient-response`, the
non-final checkpoint node inside `agent:exchange`, and `referral`, the
`transfer --referral` pointer block). Every marker-level gate, the tagpath
dialect lint above all, must accept every marker the binary itself writes; a
source-scan test enumerates the `agent:NAME` marker literals in production code
and fails when the lint gate rejects one that is not a documented in-memory
parse probe. `todo` is not runnable agent work: it has
no scheduling role and is never promoted into `queue` or `backlog`. When a turn
leaves a concrete operator action, the agent MUST add a brief, action-first
Markdown checklist item to `todo` in the same cycle (`#operatortodo`). The item
MAY refer to an existing `agent:review` or `agent:backlog` item (or an
`agent:icebox` item)
instead of duplicating its details, but that reference MUST visibly name the
source component and `#id`; a leading todo own-id alone is identity, not enough
decision context. The action itself must be understandable, and a compact
`(source: agent:review [#gate1])` locator is sufficient when the tracked item
holds the detailed choices. Publishing an unpublished draft may similarly name
its concrete draft file path. Such a reference does not transfer ownership or
scheduling out of the referenced component. The preferred write path is the
granular `--todo-add "<action>"` closeout flag, which appends one
`- [ ] [#<id>] <action>` line and never touches existing lines (an action already
present in any checkbox state is a no-op). Template patches may also update
`todo`, but the write path rejects replacements that reduce its checklist-item
count, so an agent MUST preserve every existing item when emitting `patch:todo`.

**Todo item identity.** A todo item's own id is the LEADING id token after
its checkbox, bracketed or bare: `- [ ] [#gate1] Verify [#gate1] in a live
pane` or `- [ ] #tdrename Rename the branch` (`#sdktododone`). A bare own id
must be followed by whitespace and contain at least one letter, so a row that
opens with a GitHub number (`- [ ] #157 needs review`) has no own id and cites
`#157`. Every rewrite (`--todo-done`, `--todo-edit`, `--todo-reopen`,
duplicate folding, the sticky operator check) keeps the spelling the row
already uses. An id cited later in the action (`[#gate1]`, `(#gate1)`) is a
reference to tracked work; only the leading token is the item's identity.
Operator-written and legacy items may have no own id. A todo own id is a tracked id everywhere a
backlog id is: the shadow-backlog guard never reports it as an open backlog
item outside `agent:backlog`, the coined-id guards (`session-check` warn and
the `PreToolUse` source/commit hook) resolve it, and a queue line citing it
(`- [#<id>]: note`) is a reference, not stray work. A todo that reuses a
review/backlog/icebox id (`#todoids` below) shares that id with its tracked
item by design; it is the operator prompt for that item, not a duplicate of it. The
keyed document merge splits `agent:todo` per item like the list components, so
a concurrent operator edit and agent write to the checklist merge item by item
instead of character by character (which could duplicate an edit both sides
made, `#tododupdone`).

**Todo id discipline (`#todoids`, `#todoidlayer`).** Todo ids LAYER on
tracked-work ids. A todo exists for visibility: it surfaces an operator
decision on a backlog, review, or icebox item, so it carries that item's id
rather than a new one. Because neither an own id nor an unqualified `#id`
citation identifies the source component, `--todo-add` makes every single
open tracked-work link visible in the action by appending
`(source: agent:<component> [#<tracked-id>])`. This applies to both explicit
`id=<tracked-id>` selection and implicit one-id citations. Every todo an
agent adds with `--todo-add` carries an
own id: it either REUSES the id of the one tracked item it is the operator
prompt for, or gets a SURROGATE id (minted, or a caller-chosen `id=` that names
no tracked work) — only when the action cites several open tracked items or
none. An id-less add is never written
(auto-mint, not rejection). The binary decides from the document's tracked
work (open `agent:review` / `agent:backlog` / `agent:icebox` ids; resolved ids
from `[x]` items, inline `agent:done` entries, and the `agent:done archive=`
file):

| `--todo-add` input | Own id |
|---|---|
| Action cites exactly one OPEN `agent:review`, `agent:backlog`, or `agent:icebox` id (`Verify [#gate1] live`; unknown ids such as `GH #267` beside it do not count) | Reuses it: `- [ ] [#gate1] Verify [#gate1] live`. The todo text is the actionable operator prompt for that item. |
| …and an OPEN todo already owns that id | No-op: duplicate todos for one id fold into the existing item (`--todo-edit` changes its text). |
| …and only a CHECKED todo owns that id | A follow-up action: surrogate id (minted, or the caller's `id=`), still citing the item. One id names one row, so the checked row keeps it. |
| Action cites several open tracked ids, or none (only resolved/unknown ids such as `GH #267`) | Surrogate: minted like a backlog id. |
| Explicit `id=<id>` / `[#<id>]` naming open review/backlog/icebox work | Reuses it. |
| Explicit id naming nothing tracked, action cites exactly one open tracked id | Layers on the cited id; the receipt prints `requested id=<id> not used`. |
| Explicit id naming nothing tracked, action cites several open tracked ids or none | A custom surrogate id. |
| Explicit id naming RESOLVED tracked work | Rejected: a todo for finished work is moot. |
| Explicit id already owned by a todo item | Rejected; use `--todo-edit`. |

Minted ids avoid every `#id` token in the document, the active tracked-work
ids, and every id in the done archive, so a minted id is never mistaken for a
link. A `patch:todo` is first merged while its id-less item is still keyed by
text; a strict closeout (`respond`, `finalize`, or `write --commit`) then runs
the authority-safe id maintenance against the converged document before the
commit boundary (`#todoidcloseout`). Thus re-keying cannot duplicate a
concurrent item, while a successful strict closeout never publishes a
structurally complete todo row without an own id.

**Tracked-work Todo backlinks (`#trackedworktodobacklinks`).** Every open
backlog or icebox item MUST resolve a backlink to at least one Todo that
actually exists. It may carry trailing `(todos: [#todo-a] [#todo-b])` metadata: a
durable reverse edge from that tracked-work id to one or more Todo own ids.
Ordinary `#id` prose is not a backlink. The binary also resolves the same edge
without rewriting legacy rows when a Todo shares the tracked-work id or carries
the canonical `(source: agent:<component> [#tracked-id])` locator. An id-backed
queue head therefore resolves through its backlog/icebox source id; free-text
queue work first joins the normal queue-to-backlog surrogate-id coining/mirror
path. An id-backed head that directly names a Todo also satisfies the queue
invariant. Any free-text head held from promotion, and any orphan id head, is
reported rather than silently accepted.

Preflight deterministically emits `tracked_work_missing_todo_backlink` for an
open backlog/icebox source when neither source metadata, same-id layering, nor
a canonical Todo source locator resolves to an existing Todo. Metadata naming
only absent Todo ids is dangling and receives the same finding. It also emits
that finding for an unpromoted free-text queue head or an orphan id head. This
is the compatibility/migration policy: legacy same-id and canonical source
locators remain valid without rewriting; pre-existing unlinked open rows are
flagged, not auto-created or rejected, because the binary cannot safely invent
the operator action. Authors repair them by adding/reusing a Todo and recording
one of the canonical edges. Terminal legacy rows are not retroactively flagged.

The relation is many-to-many. A source may clear several open Todos, while a
Todo backed by several sources is checked only after ALL linked sources resolve.
Done/archive text preserves `(todos: …)`, so same-closeout reap cannot lose the
edge. A checked Todo with an open linked source emits the existing component
operator-done signal (the prerequisite/unblock direction); the backlink never
silently gates, ungates, moves, or completes source work. Plain Todo citations
remain advisory and never become completion edges.

**Source qualification migration (`#todocontextmigration`).** Every preflight
(except `--probe`) audits legacy todo rows against current open review, backlog,
and icebox ids. A row whose own id and action identify exactly one such source
is rewritten through the same live-editor/detached-disk authority path to carry
the canonical component-qualified locator. Existing canonical rows are no-ops;
rows with zero or several tracked sources are left untouched. The typing gate
holds a row still being edited. Preflight emits a
`legacy_source_unqualified` `todo_signals` finding for the audited legacy row,
and stderr/ops.log record whether migration landed or was held for a later pass.

**Id backfill (`#todoidbackfill`, `#todoidcloseout`).** Every preflight (except
`--probe`) gives each id-less todo row an own id, the way queue maintenance
coins finished free-text queue lines into backlog ids. A strict closeout runs
that same maintenance after response and tracked-work projection and before
commit, so a `patch:todo` addition is addressable in the commit that introduces
it rather than only after another cycle. This also covers rows the operator
typed by hand and rows an older binary wrote. The rules match
`--todo-add`: an OPEN row whose action cites exactly one OPEN
`agent:review`/`agent:backlog`/`agent:icebox` id that no todo owns reuses that id (the row is
the operator prompt for that work, so checking it reports the prerequisite done
and resolving the work checks it off); every other row gets a minted id that
avoids every document, tracked-work, done-archive, and todo id. A CHECKED
id-less row always gets a surrogate: no decision is pending on it, and taking
the tracked id would leave the open row that surfaces the live decision
without it (`#todoidlayer`). The pass:

- never re-ids a row that already has an own id, bracketed or bare;
- inserts only the `[#id] ` token, keeping checkbox, marker, indentation, and
  text byte-for-byte;
- runs behind the free-text typing gate, so a row the operator is still typing
  stays id-less until a later pass;
- skips rows whose text duplicates another row's (duplicate folding owns them);
- runs before queue maintenance during preflight, and before publication during
  strict closeout, against the same editor authority with the same
  compare-and-swap write; carries the ids into the snapshot when its todo body
  matched, and logs `todo_id_assigned id=<id> linked=<component|none>`;
- fails closed: unreadable tracked-work evidence, an editor head that is not
  current, or a refused write assigns nothing that pass and never blocks
  admission.

A backfilled row is then completed like any other: `--todo-done <id>`, the
operator's checkbox, `--done <id>` (`#sdktododone`), or the own-id-resolved
closeout check.

**Operator done signal (`#todoids`).** The operator declares a todo done by
checking its box (`- [x]`) in the editor. No agent turn is needed: the
checkbox IS the record, the item stays in `agent:todo`, and a checklist edit
inside `agent:todo` is never a prompt. The binary reconciles it at the next
preflight with this mapping:

| Operator-checked todo | Preflight signal | What happens |
|---|---|---|
| Own id is an OPEN `agent:review` gate | `operator_done_review_open` | The agent resolves the gate that cycle (`--review-resolve <id>`) after reading any operator note, or records the follow-up. The binary never resolves a gate from a checkbox: which outcome the operator chose is judgement. |
| Own id is OPEN `agent:backlog` work | `operator_done_backlog_open` | The operator prerequisite is done; the item is unblocked (`--backlog-ungate <id>` when gated). |
| Own id is an OPEN `agent:icebox` item (`#todoidlayer`) | `operator_done_icebox_open` | The operator made the decision the todo surfaced for the parked item; the agent reads any operator note and acts that cycle (`--done <id>` to drop it, move it into active work, or record the follow-up). The binary never moves or closes parked work from a checkbox. |
| No link, or linked work already resolved | none | Nothing; the operator prunes checked items. |

An operator check is never reverted by an agent write. A `patch:todo` that
carries an item the document holds `[x]` as `[ ]`/`[/]` keeps it `[x]`
(matched by own id, else by exact text; the patch's text edits still land),
and the keyed merge keeps an operator check made concurrently with an agent
write. Only `--todo-reopen` unchecks an item.

**Todo link checks (`#todoids`).** Agents check todos at two opportune events,
using only deterministic, cheap evidence — the document's tracked-work state
and its done archive; no network, no clock, no prose heuristics:

- **Preflight** (read-only) emits `todo_signals` in its JSON and a
  `[preflight] todo:` stderr line per signal: the two operator-done signals
  above, `linked_work_resolved` (an OPEN todo whose OWN id is resolved
  tracked work), and `cited_work_resolved` (an OPEN todo that only CITES
  tracked work, all of it resolved). Signals are state, not events, so they
  repeat until the state changes.
- **Closeout** (any write that carries tracked-work flags) checks off every
  OPEN todo whose OWN id is resolved — including ids this closeout resolved with
  `--done` / `--review-resolve` — in the same transaction, prints
  `[todo] auto-done: #<id>`, and logs `todo_auto_done ... reason=linked_work_resolved`
  to `ops.log` once the envelope publishes. This is the only automatic todo
  completion. A todo that only cites resolved work is never auto-completed —
  its action may cover more than the resolved item, which needs human
  judgement — and keeps the `#tododone` advisory below.
- **`--done <todo own id>`** (`#sdktododone`). A minted todo id is not tracked
  work, so before this rule a closeout had no way to resolve it: `--done
  <id>` failed with `id not found in backlog/icebox`, and the auto-check above
  never fired for it. When `--done <id>` names no open or resolved tracked
  work but names an `agent:todo` item's OWN id, the closeout checks that item
  off in place (`[todo] done: #<id> checked off in agent:todo`; a checked item
  is a no-op) and strikes its `[#<id>]: ...` queue head in the same
  transaction, like any other done id. An id the todo only cites, or an id
  nothing owns, still fails. A queue head that drains without `--done` (a
  question or note about a decision todo) never completes the todo: only the
  explicit `--done` says the todo's work is finished.

Unreadable evidence is not absent evidence: when the document's components or
an existing done archive cannot be read, preflight emits no todo signal and the
closeout completes nothing (logging `todo_auto_done_skipped
reason=link_evidence_unreadable`); neither path treats the failure as "nothing
is open" or "nothing is resolved". An id that is both open and resolved counts
as open.

**Completing a todo (`#tododone`).** When the operator action is finished, the
agent MUST check the existing item off in the same closeout, never append a
second checked copy. When the agent itself finished a todo's work (typically a
queue head `[#<id>]: go ahead` naming the todo), `--done <id>` both checks the
todo off and strikes that head (`#sdktododone`). The granular flags address one existing item by a
selector: its own id; else the single item citing `#<id>` (narrowed to open
items for done and checked items for reopen when several cite it); else the
single item whose action text equals the selector. No match, or several, is an
error naming the candidates.

| Flag | Effect |
|---|---|
| `--todo-done <selector>` | `[ ]`/`[/]` → `[x]` in place; position, id, and text kept; a repeat is a no-op. Then folds duplicate copies of the item. |
| `--todo-edit "<selector>=<text>"` | Replace the action text; id, state, and position kept. The text may repeat but not change the id. |
| `--todo-reopen <selector>` | `[x]`/`[/]` → `[ ]` in place; a repeat is a no-op. |
| `--todo-remove <selector>` | Delete the item and its deeper-indented continuation lines; errors when nothing matches. |

Checked items stay in `agent:todo`; they are not archived to `agent:done`,
which records agent work, and the operator prunes them. All four flags are part
of the replayable closeout mutation plan; a resume drops a `--todo-remove`
whose item is already gone.

**Duplicate folding (`#tododupdone`).** After a `patch:todo` is applied, and
after `--todo-done`, the binary folds copies of one todo into a single item:
two items with the same own id, the same action text, or where one is the
other plus a completion note (`<action> — note`, also ` - `, ` -- `, `: `, `; `,
` (`) and at least one is checked. The folded item sits at the earlier copy's
position, is `[x]` when any copy is, and keeps the longer text. Distinct items
are never touched — two items that merely cite the same tracked id, and two
open items that differ by a trailing note, both survive.

**Auto-created `todo` component.** Most session documents predate `agent:todo`.
Every agent-side todo write (`--todo-add` or `patch:todo`) therefore creates the
component when the document has none, instead of dropping the action or routing
a `patch:todo` body into `exchange` as missing-component overflow. The binary
inserts an empty `## Todo` section with `<!-- agent:todo priority -->` /
`<!-- /agent:todo -->` markers at one deterministic, operator-facing location:
directly after the last `agent:run`; else after the last `agent:backlog` /
legacy `agent:pending`; else after the last `agent:exchange`; else at the end of
the document. `agent:review` is never an anchor, so in the canonical layout
`agent:todo` lands below `agent:run` and above `agent:backlog`; a document
without `agent:run` still gets Todo above any Review that follows its backlog
or exchange. Nothing outside the inserted section changes. A document that
already has `agent:todo` is left structurally unchanged, wherever its Todo
section sits.

**Default template order.** The default template scaffold (Claim for Tmux Pane,
Sync, and `claim` on an empty or component-less template document) emits the
canonical managed components in this order: `agent:status`, `agent:exchange`,
`agent:run`, `agent:todo`, `agent:backlog`, `agent:review`, `agent:icebox`,
`agent:done`, `agent:notes`. Thus `agent:todo` sits directly below
`agent:run` and above `agent:backlog`, matching the auto-created placement,
and its marker carries bare `priority` by default. Existing `agent:todo` markers
remain unchanged. To maximize visible content in a tmux view pane, each generated
level-two heading shares its line with its component opening tag (for example,
`## Run <!-- agent:run priority subagents clear -->`); the runnable-work heading
is `## Run`, not `## Queue`. Existing document headings and markers are not
rewritten.

**Operator-todo advisory.** When a closeout opens a human gate in
`agent:review` (`--review-add`, `--backlog-add-gated`, or `--backlog-gate`) and
the resulting `agent:todo` does not reference that id as `#id`, the write prints
a non-blocking `[todo] advisory (#operatortodo)` line naming the ids and the
`--todo-add` command, and logs `operator_todo_advisory` to `ops.log`. The check
is id-based; a gate blocked on CI or an external party rather than the operator
simply needs no todo.

After the response and same-cycle todo mutations converge, closeout also checks
the captured response for conservative, deterministic operator asks (GH #314,
GH #318):

- an explicit post/publish decision request (`say \`post\``, `needs your ok`,
  `should I publish`, ...), one ask per line;
- an unpublished/not-posted draft whose paragraph cites a concrete Markdown
  path;
- each top-level item of a list introduced by a bolded residual-work label
  ending in a colon: `Still stale`, `Also stale`, `Not done`, `Still open`,
  `Open`, or `Remaining`, optionally qualified (`**Also stale, not drafted:**`,
  `**Open questions:**`). A label with inline text and no list is one ask; an
  empty report (`**Remaining:** none.`) is none.

Signals match on word boundaries (`reply posted` is not `reply post`), and
Markdown headings and fenced code never count. Asks are counted, not
reduced to a boolean: an ask is recorded when at least half of its content
words appear in one of this closeout's `--todo-add` texts or in an open
`agent:todo` item, and a `--todo-add` that matches no ask is taken to record
one ask in other words. When more asks remain unrecorded than such unmatched
adds, the write prints the same non-blocking advisory class — the ask and
add counts, then each unrecorded ask quoted with its signal and the exact
matched text — and logs `operator_todo_response_advisory ... signal=<first
signal> asks=<n> todo_adds=<n> unrecorded=<n> matched=[...] items=[...]`.
Four asks and two unrelated adds still advise; an ask already open in
`agent:todo` does not re-fire. Neither advisory writes a todo by itself or
fails closeout.

**Blocking operator request todo (`#packettodo`).** A blocking
`[#subagent-operator-request-<unique-id>]` packet in a closeout response is a pending
operator decision, so the binary records it instead of advising. The packet is
parsed structurally: a request-specific marker line (outside fenced code, not
blockquoted, at most three leading spaces) followed without a blank line by
canonical list fields `- *kind:* question|permission`, `- *blocking:* true`,
and a concrete `- *request:*` line. Historical plain fields and emphasized or
bold key variants remain readable; the protocol's `<...>` placeholders never
count. Marker ids use the reserved
`subagent-operator-request-` prefix plus a request-specific alphanumeric/hyphen
suffix and MUST be unique among packets in one closeout; duplicate ids fail the
write before response capture. The fixed `[#subagent-operator-request]` spelling
remains read-compatible for historical packets but is not emitted. When the closeout supplies no
`--todo-add` and its response carries no `patch:todo`, the write appends one
derived `--todo-add` per packet to the same tracked-work transaction:
`Answer the blocking subagent question|permission request [for #<id>] in
agent:exchange: <first sentence of request> (choices: <labels>)`. `<id>` is the
one id this closeout gated (`--backlog-gate`, `--review-add`, a gated add), else
the one document tracked-work id the response cites; with several or none the
text cites nothing and `--todo-add` mints a surrogate (`#todoidlayer`). The text
is deterministic and the derived item is part of the replayable mutation plan,
so a retry, a resume, or a replayed packet folds into the existing open todo
instead of duplicating it. The write prints `[todo] blocking operator request
packet (#packettodo)` and logs `blocking_operator_request_todo_derived`; a
packet skipped because the closeout recorded its own todo logs
`blocking_operator_request_todo_skipped`. Non-blocking, fenced, and quoted
packets record nothing. Todo mutations run after the envelope's adds, ungates,
and gates, so a todo layered on an item this closeout gated names
`(source: agent:review [#<id>])`.

The current packet protocol also carries
`- *queue_item:* <origin-queue-item>` verbatim. Its derived todo owns
`[#<packet-id>]`; an explicit todo suppresses
derivation only when it owns that same id. While the todo is open, the matching
live queue row carries identity-neutral `🙋` immediately beside any worker
lifecycle marker (`🙋🚧` while running), while `[#<packet-id>]` is appended to
the row beside the secondary request instead of preceding the primary work.
The row is not drainable while `🙋` is present. The operator may answer locally
by extending the suffix to `[#<packet-id>]: <answer>`; the next closeout checks
off the same-id todo and removes `🙋` in the same transaction while preserving
the local request/answer evidence. Explicit todo completion has the same
unblocking projection. This request-linked rule supersedes the older
tracked-id/surrogate layering described above for newly emitted packets.

**Todo-done advisory.** When a closeout resolves tracked work (`--done`,
`--review-resolve`) and an OPEN `agent:todo` item still cites it (an item that
owns the id was already checked off by the `#todoids` closeout check), the
write prints a non-blocking `[todo] advisory (#tododone)` line
naming the ids and the exact `--todo-done <selector>` command, and logs
`operator_todo_done_advisory` to `ops.log`. Like the gate advisory it is
id-based, never writes, and never fails the closeout.

**Exchange prompt defaults.** `<!-- agent:exchange preset="#review" -->` applies
the named frontmatter prompt preset to each new exchange prompt, including the
prompt admitted at preflight and prompts surfaced as realtime steering. A bare
`subagents` attribute (alias `fan-out`) delegates each new exchange prompt to a
background subagent while the parent retains ownership of the response cycle,
reviews the worker result, and performs closeout. Unlike queue dispatch,
exchange dispatch has no claim/release lifecycle and no concurrency-valued
form: `subagents=N` / `fan-out=N` on `agent:exchange` is invalid. Mid-turn and
closeout steering guidance MUST preserve that source distinction: it MUST NOT
render a queue claim or release instruction for an exchange prompt.

Tracked backlog/review items may carry a machine-readable symptom de-duplication marker in their text: `[symptom-key invariant=<id> document=<doc-id> component=<component> content_hash=<hash>]`. `--backlog-add` (legacy `--pending-add`) and `--review-add` use this key to attach a repeated symptom as an indented `evidence:` continuation on the existing open/gated backlog or review item instead of inserting another tracked item. The key fields are field-safe tokens and intentionally match the binary outcome vocabulary: invariant id, document id, component, and content hash.

Tracked-work extraction is Markdown-structural. Checklist-shaped text inside fenced or indented code, inline code, HTML comments, or raw HTML blocks is literal evidence and never a live backlog/review/icebox item, even when the literal block is indented beneath a real item. The parser uses Markdown AST byte ranges for this classification; line-pattern matching must not independently promote literal examples into tracked work.

**Shadow-backlog guard (orphaned open items).** Preflight (`enforce_no_shadow_open_backlog`) refuses, and `session-check` interrupts on (`shadow_backlog_guard`), an open id-bearing checklist row (`- [ ] [#id] …` / `- [/] [#id] …`) that sits outside every live component and whose id is not open in `agent:backlog`: the "open backlog item(s) exist only outside live agent:backlog" refusal. The scan (`detect_shadow_open_items`) targets stray prose — HTML comments and non-component sections. Code exclusion is decided by the row head (list marker, checkbox, and leading `[#id]` token), not the whole line: a row is skipped when a fenced or indented code block covers it, or when a backtick span covers its marker or its `[#id]`; an inline code span later in the row text (`` - [ ] [#id] Fix `foo` ``) does not hide a drifted row. An id quoted inside backticks in prose (`` `[#id]` ``) or a whole row shown inside a code span is a citation, never a tracked row, and stays unflagged. It skips every live component that legitimately holds `[#id]` rows: `agent:backlog`, `agent:review`, `agent:icebox`, `agent:exchange`, `agent:run` (id heads such as `[#id]: note` / `do [#id]`), and `agent:todo` (the operator checklist, whose leading `[#id]` is the todo item's own id). A row outside those components whose id is open in `agent:backlog` is only a duplicate warning; a genuinely orphaned open row is still refused.

### §2.5 Queue Component

**Prompt-preset attribute alias (`#presetattralias`).** `presets=` is a recognized compatibility alias for the canonical prompt-component `preset=` attribute on `agent:run` and `agent:exchange`; it is not an unknown-attribute typo. Every preset consumer, including subagent authorization, queue selection, continuation, and realtime steering, reads the same effective value. When both spellings carry nonempty values, canonical `preset=` wins. An empty alias has the same no-default meaning as `preset=""` when the other spelling is absent. Binary marker rewrites preserve the operator's spelling.

One or more `agent:run` components hold batches of prompts consumed sequentially. The default template scaffolds one unkeyed `<!-- agent:run priority subagents clear -->` queue between `exchange` and `backlog`; it stable-sorts id-backed work by source priority, dispatches eligible heads to background subagents, and explicitly declares fresh-context clearing. The unkeyed shape remains the implicit `default` queue. Existing explicitly authored queue markers retain their attributes and behavior; this default applies when Agent Doc creates a new scaffold.

**Syntax:** hybrid list items and fenced prompts.

| Form | Example | Description |
|------|---------|-------------|
| Single-line | `- do #fix1` | Bare `- ` prefix at column 0. A single stray leading backtick (`` `- text ``, a common code-span mistype) is normalized to `- text` so the item parses as a prompt and self-heals on re-render instead of being silently skipped as inert text (`#queue-line-leading-backtick-drop`). |
| Bare paragraph | Column-zero prose at an item boundary, optionally followed by more prose or a blockquote | Distinct free-text prompt; rewrites normalize it to a fenced multiline prompt (`#queuefreetextsplit`) |
| Multi-line (tilde) | `~~~prompt`...`~~~` | Fenced with `~~~prompt` opener |
| Multi-line (dash) | `---`...`---` | Fenced with bare `---` |
| Recovered evidence task | Explicit request before or after a balanced Markdown code fence | One exact-span multiline prompt; list-looking evidence inside the fence is never a separate prompt |
| Start fence | `--- start [at <datetime>]` | Activation signal (consumed on use) |
| Stop fence | `--- stop` | Breakpoint (consumed when reached) |

**Queue item boundaries (`#queuecruft`, `#queuefreetextsplit`).** A free-text list item owns the indented lines directly beneath it and ends at the next blank line, list marker, column-zero prose paragraph, or queue-native line. CommonMark's unindented lazy continuation is intentionally not used: in an operational queue, column-zero prose starts a distinct bare free-text head. Consecutive prose lines and an immediately following blockquote belong to that bare head. A component marker, fence opener (```` ``` ````, `~~~`, `---`), thematic separator (`---`, `***`, `___`), `preset`/`dispatch`, `/command`, ATX heading, or id-bearing directive (`do [#id]`, `re [#id]`, a leading `[#id]`) ends the current item. The item's span in `document_queue::parse_spans` and its node in `markdown_ast` (`item_nodes`) both cover every owned line, so they agree on where an item ends. As a result:

Terminal diagnostic trees are the narrow exception (`#queueterminalwrap`). Once an indented continuation line begins with a terminal tree glyph (`└`, `├`, `│`, or a rounded-box equivalent), subsequent hard-wrapped prompt-content lines remain attached to that list item even when a terminal copy dropped their indentation. A blank line or queue-native boundary still ends the item. This prevents pasted hook/error evidence from manufacturing executable queue rows while preserving ordinary column-zero prose as a distinct head.

- List-item continuation lines are kept verbatim. A bare paragraph normalizes to the canonical `---` multiline prompt surface when rendered or mutated.
- Free-text admission promotes the whole item. The backlog text is the item's lines joined with single spaces, and the queue replacement (`do [#id]`) drops every line of the item. No continuation line is left behind as `Freeform` residue.
- Node-keyed remove, strike, and reorder, the backlog→queue mirror (`sync`/`append`/`prepend`), and every other entry-level reorder move or drop the item as a unit. None of them can detach a continuation line from its item.
- A strike that wraps the whole item (`- ~~first line` … `last line~~`) reads back as one completed item.

Only an unstruck free-text item takes continuation lines. An id-backed `do`/`re` directive never does: a bullet-less `do [#id]` line beneath it is a folded paste that `#qfoldedhead` splits back out into its own head. A line struck on its own (`- ~~text~~`) also keeps any bullet-less line below it as separate residue, so a completed item is never brought back as live text. Fenced multiline prompts (`~~~prompt`, `---`) keep their line structure when they are admitted into the backlog.

**Attributes:** `<!-- agent:run auto -->` is accepted as a legacy/manual immediate activation hint when the queue is non-empty. Binary-owned queue writes must not create `auto`; when route or maintenance touches a legacy `agent:run auto` tag, it strips `auto` while preserving other attributes and uses `queue_active: true`, `--- start`, or explicit exchange triggers for activation. `auto` is **queue-only**: placing it on any other component (e.g. `agent:backlog`) does not activate the auto-loop and triggers a `misplaced_component_attr` preflight warning (see §2.4). `preset` is shared by the two prompt components. On the queue tag (e.g. `<!-- agent:run preset="#spec-test-build-install-commit-push" -->`) it supplies default presets for queue items; on the exchange tag it supplies the defaults described in §2.4. The value is a whitespace-separated list of preset references, so values containing more than one reference must quote the whole attribute: `preset="#push #auth"`. Single- and double-quoted whole values are equivalent, and quote characters immediately around an individual reference are ignored. Every nonempty token is a preset name (the value is not prose); hashed and bare aliases may be mixed. Resolvers canonicalize each token independently, preserve first-occurrence source order, and collapse duplicate canonical names. Resolved scalar and structured presets both contribute their prompt bodies; each structured preset retains its own `runbook`. If any token is unresolved, the whole authorization fails closed while the ordered per-token resolution details remain visible. A body-level `preset #name` queue line remains supported and follows the attribute references. An empty value (`preset=""`) is the operator's explicit "no preset" (GH #227): every preset resolver treats it exactly like an absent `preset` (no default directive, so heads are classified as if the queue had no preset), the lint gate accepts it on `agent:run` and `agent:exchange`, and agent-doc never rewrites or strips it — every queue-tag rewrite (preflight normalization, control binding, route `go`, `auto` convergence) preserves the marker byte for byte.

**Queue-level subagent dispatch (`subagents`, alias `fan-out`).** `<!-- agent:run subagents -->` (or `subagents=N`, `fan-out`, `fan-out=N`) gives every queue line subagent intent, the same as a per-line `#subagent` tag, so it combines with `preset=` instead of competing for it. A bare flag dispatches every currently eligible head; `=N` sets an explicit positive concurrency cap, and when both spellings appear the most restrictive valid capacity wins. A line tagged `[inline]` or `[operator-verify]` opts out and drains in queue order. The bare spellings are also valid on `agent:exchange` under the §2.4 contract; concurrency-valued forms are queue-only. On `agent:run`, a non-positive or non-numeric value (`subagents=0`) warns and is ignored, so the queue drains inline. Mid-turn steering classifies those lines `subagent`. Unlike a per-line tag, which is offered only on the cycle after the line is added, dispatch under the attribute depends on the queue's current state. Every cycle, preflight lists in `queue_subagent_dispatch` the unclaimed eligible heads in queue order: all of them for a bare attribute, or up to the explicit cap minus the live claims for `=N`. A head whose claim was missed is therefore offered again rather than drained inline. An eligible head is *held* when an explicit cap is full or when an `after=` or ordered-list predecessor is still in the queue. A held head is out of the in-session loop, the same as a claimed head: it is never in `selected_queue_prompts` and does not count toward `queue_drainable_head_count` or `queue_continuation_required`. A later cycle offers it once a slot frees up or the predecessor closes. The coordinator keeps a running subagent's claim alive with `agent-doc queue claim <FILE> --item <ID> --owner <OWNER> --refresh [--ttl-secs N]`. This extends only that owner's live claim, and it refuses an unclaimed item, an expired claim, or another owner's claim (see `07-orchestration-commands.md`). If a claim expires under the attribute (because the subagent died or stopped refreshing it), the head goes back to the dispatch set. Preflight lists it in `queue_subagent_dispatch` again, or holds it if an explicit cap is full. It never enters `selected_queue_prompts`, so the in-session loop does not run work a subagent may still be doing. The per-line `#subagent` tag gives no such protection: once its claim expires, the line drains inline in queue order. Each `queue_subagent_dispatch` entry also carries `authorization`, the operator authorization the subagent needs (`#waypostauthorization`, see `07-orchestration-commands.md`).

**Subagent completion feedback (`#subagentcompletionfeedback`).** Background workers never edit the shared session document. As soon as a claimed worker reports back, the coordinator calls `agent-doc queue report-complete` with the worker's complete response; the binary publishes one deduplicated recovery response cell to `agent:exchange` while keeping the claim active for integration. Current completion reporting requires a non-empty response and has no placeholder-only mode. The exact contiguous grammar is a `### Re: Subagent completed: `<owner>` returned work for queue item “<item>” — agent-doc` heading, a response-digest marker, the visibly blockquoted worker response, then the keyed `agent-doc-subagent-completion` marker. The heading follows the ordinary `### Re: topic — attribution` shape with exactly one spaced em dash (em dashes inside the owner/item are neutralized): the TOPIC names the worker and item and the attribution is the binary. Through 0.35.503 the heading was `### Re: Subagent completed — agent-doc: <detail>`, which put the detail in the attribution slot, so every cell normalized to the same topic `subagent completed`; two or more uncommitted reports then made preflight recovery refuse every later turn as a repeated replay heading and made session-check call the cells a direct patchback. There is no blank separator inside the cell: the existing N−1 controller sees the whole added block beginning with the already-inert `### Re:` grammar, while the structural/session-check gate sees a real response body rather than a bodyless heading plus ignored HTML comment. The unique heading is also the exchange-cell identity, so concurrent notices compose instead of replacing one another. The artifact is never an operator prompt or an `exchange_subagent_dispatch` item. It is also never an agent response: recovery and closeout recognize a cell STRUCTURALLY — the current heading, digest-attested response body, and keyed marker, or the historical fixed-body/bodyless forms — never by matching its prose. A body edit invalidates the digest and removes completion authority. The same heading words without an attested body and marker remain an ordinary response heading. `report-complete` does not commit: an attached editor's write is still a retained intent when the command returns, and committing the whole document from the reporter would also commit unanswered operator edits. An open coordinator cycle may commit the cell while integration is pending; when closeout later proves and strikes that exact queue item, the queue-strike successor retires every structurally attested cell for the consumed item while preserving unmatched reports and heading-shaped prose. The same retirement applies on every binary strike path, not only closeout (`#strikeretires`, `specs/07-orchestration-commands.md`): the preflight residue/backlog/done strikes, the orphan `queue consume --id` strike, and the controller's answered-head projection each retire the struck row's cell in their own successor, so a cell can never paint `🔄` onto a later row with the same text. This retirement also applies when the matching cell exists only in an older retained controller target: queue-consume composition treats the authoritative strike as a tombstone before merging that target, and the delivery landing fence includes the hidden cell's key. A delayed retained write therefore cannot append a stale worker handback after closeout, dirty the just-committed document, or turn the next cycle into operator-diff recovery. The integrated response therefore replaces its temporary worker-response cell rather than leaving stale handback prose beside it. Between turns, the next preflight's binary-owned document-only auto-commit may land pending cells in `HEAD`, so a document carrying any number of cells — including historical cells — is admitted without migration. Its dedupe key excludes the mutable claim expiry, so a TTL refresh cannot duplicate it. `queue release` only releases a claim—abandon, failure, and requeue are not falsely reported as successful completion.

Operator removal wins during completion delivery (`#queue-removal-cancels`).
Removal and `report-complete` serialize through the same document-scoped owner.
Removal writes an audit tombstone for the exact claim identity+generation and
publishes a successor that retires the exact keyed completion cell even when it
exists only in an older retained target. Pending delivery is thereby
acknowledged/dropped and cannot replay the row, close it, or authorize automatic
integration. A later identical row starts above the tombstoned generation and
cannot consume the stale cell. Every document projection reader treats the
cancellation receipt as a terminal, non-visible fence: an old release receipt
with the same identity+generation and an old completion cell with its exact key
are non-governing even across a crash or controller restart. Neither may add
`☑️` (or legacy `📫` / `📤`), `🔄`, owner metadata, or terminality to the later row. Deleting a durably terminal struck row is only
presentation cleanup; strikethrough that conflicts with a live claim, reported
receipt, or completion outbox is a degraded mismatch and preserves/fences the
work instead of cancelling or integrating it.

Independent `report-complete` processes serialize through a document-scoped cross-process owner, then re-read authority and recompute their insertion inside that owner. An attached-editor write is accepted as a durable retained intent and projected by the long-lived controller; the one-shot reporter does not wait through typing, delivery, or native-save backpressure and never writes editor-owned disk. A detached document uses the ordinary synchronous CAS write. Concurrent reports therefore compose without losing notices or operator edits and cannot race coordinator closeout with stale full-document successors.

The short-lived bodyless v0.34 artifact (`### Re: Subagent completed — agent-doc: ...` immediately followed by its marker) has a narrow, idempotent binary migration: `agent-doc queue repair-completion-feedback <FILE>`. It rewrites only exact binary heading+marker pairs into the valid response-cell grammar through the same document owner/editor authority. It does not replay or resubmit the committed closeout, release claims, or change cycle state; running it again is a no-op.

**Queue auto-clear (`clear`, `#bgclearpolicy`).** An active queue drains each item in a fresh agent context by default. Bare queue-only `clear` positively declares that policy and appears in generated templates; omission remains enabled for backward compatibility. A direct queue clears once per completed turn. A `subagents`/`fan-out` queue defers the clear to the next safe idle boundary after in-flight claims settle. At that boundary and before the next head is dispatched, the document's supervisor resets the harness context once (`/clear` typed into the live Claude Code process, `/new` into OpenCode, a fresh restart for Codex), then dispatches the next head to that fresh context. The legacy bare `disable-auto-clear` flag (also `disable-auto-clear=true`; `=false` keeps it on) turns this off and wins if both spellings appear. Preflight reports the effective setting as `queue_auto_clear: true|false`. A context clear does not reduce session accretion, which measures the exchange size; only compaction does. Full rules: [Supervisor](supervisor.md).

**Queue schedule (`agent:loop`, `interval`, `time`, `#queueschedule`).** `agent:loop` is the canonical component for a repeating scheduled run: `<!-- agent:loop interval=30m time=09:00,17:30 -->`. It uses the ordinary runnable-queue body and attributes, and implies an unbounded `loop` schedule. `loop=N` on `agent:loop` retains the finite-pass form. The older `<!-- agent:queue ... loop -->` / `loop=N` spelling and `agent:run loop` remain backward-compatible aliases with identical runtime behavior; existing documents are not rewritten merely to change marker spelling. A scheduled run does not drain the moment it has a live head. It drains only inside a **pass window** that the project controller's queue scheduler opens at each due tick; the drain itself is the ordinary one (supervisor idle watch, in-session loop, `subagents` fan-out), so jobs keep running on the existing background workers.

- `interval=<duration>` is the poll interval: a bare number of seconds (`90`) or `d`/`h`/`m`/`s` segments (`30s`, `5m`, `1h30m`, `1d`). Values below the 60 s floor are clamped to 60 s with a preflight warning. `0` and unparsable values are fatal for that queue's schedule declaration: its heads remain held until the marker is corrected, while sibling queues remain unaffected.
- `time=HH:MM[,HH:MM...]` is one or more times of day in the controller's local time zone (`00:00`-`23:59`, sorted and deduplicated). `interval` and `time` combine: the next tick is the earliest of either.
- `agent:loop` keeps the items in the run by default. Each pass drains them normally; when the pass completes the controller restores every item the pass owned (a struck `~~item~~` copy is removed and the item re-appended before the closing marker, every other byte preserved) so it runs again next pass. The compatibility attribute `loop` (also `loop=true`) repeats without limit; `loop=N` (positive) runs exactly N passes and then holds the items. A loop without `interval`/`time` starts the next pass 60 s after the previous one completed. A head added while a pass runs joins that pass's roster and loops too. Realtime observations reconcile that roster against live heads and **drain evidence**: a roster item the pass's drain was observed acting on, either struck-complete (`~~item~~`) or live with a drain lifecycle marker (`🚧` claimed, `🟡`/`📥`/`⚠️`/`⏭️` subagent attempt, `⏳` awaiting operator; pin markers are not evidence), stays owned by the pass for the rest of it, so it keeps the pass open even if a later write erases its row. An older spelling that is neither still live nor carries drain evidence did not run and is removed before restore. Thus editor refreshes may observe intermediate keystroke states without turning those states into future queue items (GH #310). Every pass-owned drain keeps the struck row (below), so an item the drain consumed is restored from that row (GH #327).
- **A loop restore never resurrects a removed row (`#loopnoresurrect`).** Restore is a pure function of the loop body: the restored set is the pass roster restricted to identities that have a struck `~~item~~` row in **that same loop** when the pass completes, and the restore planner applies the same rule again when a retained `restore_pending` roster is written later. A roster item that is neither live nor struck in the loop was removed or moved out of it (an operator delete, a cut into `agent:run`, an archive to `agent:done`), and it stays removed; its drain evidence only keeps the pass open. A row live or struck in a sibling queue is never a restore source, and the restore never rewrites the loop's marker or any byte outside the restored rows. On 2026-10-26 a pass that had stayed open for three days accumulated 16 roster items; when the operator cut the loop's rows into `agent:run`, the pass completed and the old rule restored all 16, 12 of them rows struck and removed days earlier.
- **An idle loop self-heals a missed restore (GH #335, GH #348).** When a due tick finds a closed, non-held loop queue with no live head or pending restore but with struck rows, it reconstructs the roster from those rows, durably opens the next pass with that roster pending, and restores the rows before dispatch. The restore is derived from that exact struck observation and remains pending until a post-write authority read proves every roster identity live. While it is pending, those old struck rows cannot complete the newly opened pass. The recovered tick reports its next run only after restore success or failure updates schedule state, so it never exposes an unset backoff as Unix epoch. The same tick therefore recovers a post-pass restore write that was lost; `pause` / `stop` and exhausted `loop=N` queues remain inert.
- **A `loop` queue keeps its pass-owned struck rows when it drains.** Every pass-owned drain path that would otherwise erase a drained queue body (the closeout consume that empties the queue and preflight's drained-queue clear after a residue or done-id strike) leaves a `loop` queue's struck rows in place, in the document and the snapshot; only the loop restore removes them as it re-appends the items. Erasing them would leave a consumed item neither live nor struck, so the pass would complete with nothing to restore (`queue_schedule_pass_completed ... restore_items=0`) and the loop would silently lose its program (GH #327). The key is the presence of the `loop` attribute itself, so a malformed `loop=` value still takes the non-lossy direction. Queue control (`queue: go`, marker activation tokens) still clears on drain exactly as for any other queue.
- **Document-wide free-text auto-strike never targets a `loop` row (GH #361, GH #364).** A quoted `> **Queue prompt:**` echo or plain-prose preset expansion may close a matching row in an ordinary queue through the finalize `#ftstrike` projection, but it cannot strike an identical live row in a loop queue. The auto-strike projection therefore cannot terminalize the loop or add a `stop` control to its marker. Loop heads close only through their pass-owned consume path, which preserves the struck row as completion evidence and restores it before the pass is considered complete.
- **A `loop` head is answered only within its own pass (GH #327 pass 4).** The preflight `#qheadresidue` catch-up strike treats a live free-text head of a `loop` queue as a standing program: the committed exchange always holds earlier passes' echoes, and a sibling queue may hold the same text. It strikes such a head only on an echo that is newer than every one of these anchors (the newest one wins): (1) the `#qheadresiduescope` entry anchor (the newest commit in which the head was not live in this queue); (2) **the pass opening**, the exchange of the newest commit whose committer time is at or before the open window's `opened_secs`, so an echo committed before the current pass opened is history for this pass; (3) **another queue's echo**, the exchange of the newest version (committed, or the current uncommitted text) whose write added an echo of the head while it struck that head in another queue (a struck row that queue did not have before) or consumed it from another queue (a live row it no longer has), so an echo written about another queue's copy never answers the loop queue's copy. With no open pass (window `Closed`, no schedule row, or an unreadable row) the loop head is held and no echo can answer it, so the catch-up never strikes it; a strike then would lose the item, because only an open pass's roster is restored. The walk (`queue_head_entry::LoopPassEchoWalk`) is pure policy in `agent-doc-queue`; `agent_doc_git_io::revision::loop_pass_head_echo_anchor_exchange` feeds it at most 128 commits newest first with their committer times, starting from the current content; with no history every echo counts, and a head still live at the limit is anchored at the oldest version read. A skipped strike logs `preflight_freetext_residue_strike_skipped_loop_echo_outside_pass queue_id=<Q> heads=<n> anchors=<pass_opened|other_queue_echo|head_entry|history_limit|no_open_pass>`. Measured in GH #327 pass 4: a queue-1 cycle answered queue-1's `#upgrade` (echo committed 94 s before pass 4 opened), and the loop-dispatched cycle's preflight struck queue-2's live `#upgrade` with that echo, so the pass completed without running it. The `#syntheticstrike` continuation-head skip (`queue_continuation_required` / the post-closeout next head) applies the same scope through the same resolver (`agent_doc_queue_io::queue_schedule::head_echo_window` + `agent_doc_git_io::revision::head_echo_scope`): a `loop` head is skipped as already answered only when an echo in its open pass, not about another queue's copy, answers it, so a loop head the catch-up strike keeps is offered as the next head instead of being passed over unrun. Non-loop queues keep the `#qheadresiduescope` entry scope only, at both sites.
- **Queue-local identity and non-interference.** Let `D` be the canonical document identity and `Q` the canonical queue ID (`default` for the legacy single unkeyed queue). Every declaration, durable state row, pass, roster, due tick, hold, loop restore, and free-text admission write is addressed by the product key `(D,Q)`. For distinct IDs `Q1 != Q2`, a transition of `(D,Q1)` cannot read or mutate `(D,Q2)`'s schedule state or queue body. Thus an immediate `id=main` queue and an `id=issues interval=1h loop` queue coexist: the issues cadence neither gates nor replays main, and admitting a free-text head selected from `issues` cannot replace or reprioritize `main`. Lifecycle marker projection is not a queue transition: claims, report receipts, and completion cells are keyed by work identity, so every queue that displays an identity shows the same attempt state, and the projection rewrites only the lifecycle/owner prefix of a live row (never a struck sibling row, never a sibling's order, text, or schedule state; `#queuemirrormarkers`, [Orchestration commands](07-orchestration-commands.md)).
- **Holding.** While queue `Q` has no open window, only `Q`'s live heads are held exactly like claimed heads: they are excluded from the in-session loop, preflight selection (`selected_queue_prompts`, `queue_drainable_head_count`, `queue_continuation_required`), the Stop-hook continuation, supervisor idle-watch drain, and `queue_subagent_dispatch`. A declaration the controller has not admitted, whose state cannot be read, or whose syntax is invalid is held fail-closed for `Q`; no sibling queue is held by that failure.
- **Pass lifecycle.** A due tick with live heads opens pass `passes_completed + 1` and records its roster (the live heads). A tick with no live head is an empty poll; a tick while the queue is held by its control (`pause`, legacy `stop`) is skipped; a tick while a pass still runs is folded into it. A non-loop pass completes when the queue has no live head; a `loop` pass completes when none of its roster items is live. `loop=N` stops opening windows once N passes completed.
- **Realtime.** Editing queue `Q`'s attributes reschedules `(D,Q)` immediately, without polling the document: changing the cadence (`interval`/`time`) re-anchors it at the edit (the next tick is one interval later, not immediately), while editing only `loop` keeps the anchor; `Q`'s durable pass count survives every edit. Removing every schedule attribute from `Q` unschedules only `Q` and deletes only its state.
- **Anti-thrash.** Missed ticks (suspend, hot update, a long pass) are coalesced into one: the controller fires the latest missed deadline once and schedules the next strictly after now, never a catch-up burst. A tick is fired at most once across controller restarts and handoffs. A failed tick or restore backs off exponentially from 30 s to a 1 h cap.
- **State.** Progress is durable per `(document, queue_id)` in `state.db`: the legacy `default` row retains `state_kind = "queue_schedule"`; identified queues use `state_kind = "queue_schedule:<queue_id>"`. Each row independently stores its proven owner queue id, cadence anchor, last fired deadline, `passes_completed`, open window and roster, pending `loop` restore, coalesced-tick count, and backoff. Every transition is a `BEGIN IMMEDIATE` read-modify-write. A legacy document-global row may be adopted only when the document has exactly one queue, making the successor identity unambiguous. A multi-queue split discards that row; during rolling upgrade, any older keyed row without owner proof in a multi-queue document is restarted from that queue's current heads so sibling counters or a sibling roster can never gate, exhaust, restore into, or keep the queue permanently busy. `agent-doc queue schedule <FILE> [--json]` reports an ordered `queues` array whose entries always include `queue_id`; invalid declarations report errors and held-head count without a runnable status (see `07-orchestration-commands.md`).

**Multiple identified queues and dependencies.** A single unkeyed queue retains the legacy identity `default`. Once a document contains multiple queue components, preflight's binary-owned queue-maintenance projection gives every omitted `id=` a unique persisted surrogate before graph selection: the one-based document-order candidate is `queue-N`, and collisions with any explicit or already-generated ID are resolved by probing `queue-N-2`, `queue-N-3`, and so on. Existing valid IDs and their marker bytes are preserved; after the generated IDs are persisted, reordering existing queues does not rename them, and another normalization pass is a no-op. A bare `id`, empty `id=`, invalid explicit ID, or duplicate explicit ID remains malformed and fails closed rather than being silently migrated. `after=id-a,id-b` is an all-of dependency and blocks a queue until every referenced queue has no live work. References name queue IDs in the same document; missing predecessor IDs, self-dependencies, empty dependency elements, and dependency cycles are invalid. Ready queues are selected in document order. Selection is owner-relative: when every live head in a queue is unavailable to the current drain owner (claimed by a worker, held for subagent dispatch, or held by a closed schedule window), that queue remains live but is skipped as paused, so an independent later ready queue can run. It is not treated as empty: an `after=` dependent still waits for those externally owned heads to finish. This projection is shared by preflight maintenance, read-only probes, in-session continuation, and the supervisor, so opening a scheduled queue behind a delegated queue cannot wait for unrelated document activity. Struck/completed rows do not keep a dependency open. Queue attributes and ordered-list semantics are scoped to their block, so one queue may use `subagents` while another drains inline or uses a different `preset`.

Follow-up work produced while handling a queue head retains that head's queue id. Dispatch-time `queue_subagent_dispatch[].authorization.queue_id` and its verbatim preamble carry that provenance to the worker and back to the coordinator. Follow-ups are inserted immediately after the originating head, not into whichever sibling queue is currently selected and not after a document-wide mirror of the same text. `agent-doc queue follow-up <FILE> --id <QUEUE_ID> --after <ORIGIN> --item <TEXT>...` is the binary-owned mutation for this transition. It resolves both the queue id and origin exactly, writes through live editor/CRDT authority, preserves item order, and fails closed when the queue is absent or the origin is missing or ambiguous. This queue-local provenance rule applies even when a priority queue mirrors or exposes the originating head for dispatch.

```html
<!-- agent:run id=release-a subagents -->
- finish release A
<!-- /agent:run -->

<!-- agent:run id=release-b after=release-a preset=#release -->
1. organize release B
2. publish release B
<!-- /agent:run -->
```

Here `release-b` becomes runnable only after `release-a` drains. Existing documents with one unkeyed queue require no migration.

**Atomic recurring refill.** A producer that starts another run of a dependent sequence must append every stage through one generation-fenced `QueueAppendBatch`. The mutation validates the existing graph, resolves every target by queue ID, applies all appends to one in-memory proposal, validates the complete proposed graph, and publishes either one document generation or none. Creating a second queue beside a legacy singleton changes the original marker to `id=default` in that same proposal. For `build -> publish`, appending run N to both queues in one batch makes `build` nonempty in the only observable new generation, so `publish` cannot slip through between writes. Appending only to `publish` while `build` is empty intentionally makes `publish` eligible. Refilling `build` after one exact `publish` head was dispatched does not revoke that in-flight head; it blocks the next `publish` decision after the exact receipt arrives.

**Queue head work identity (`#claimdispatchidentity`).** Claims, `queue_subagent_dispatch`, mid-turn steering, in-session drainability and preflight head selection identify a queue head by one shared key, its *work identity*. Lifecycle and priority markers (`🚧`, `⏭️`, `📌`) and leading intent/preset tags (`#subagents`, `#subagent:`, `#gh-fix`, `#bug:`, meaning any leading `#tag`/`#tag:` token that starts with a letter and is followed by more text) are routing, not work, so they are ignored. The remainder keys as its `#id` when it is id-backed and otherwise as its normalized text (a URL or prose). Editing only the tags or markers of a claimed line keeps its claim. A claim is bound to its line rather than its text (`#claimfollowedit`): an in-place edit of a claimed free-text line (a corrected URL, reworded prose, an appended note) carries the claim to the edited line, by the same edit alignment mid-turn steering uses, and steering forwards the edit to the claim's owner. A line with a different `#id`, or a claimed line deleted while another is added elsewhere, is a different task and stays unclaimed. The narrow id-to-free-text exception is a backlog reconciliation that replaces an id-backed mirror with the exact tracked-item prose captured when the claim was made (`#claimtrackedalias`); a unique, unheld exact alias inherits the same claim so its running lifecycle marker does not disappear. Dispatch offers only live, executable heads (never a nesting parent's group label), which is the same set `agent-doc queue claim` resolves against. Per-line dispatch is offered when the head's work identity is new since the previous cycle's seed, or when an existing head gains subagent intent (`do [#a]` edited into `#subagents do [#a]`). A tag-only edit of a line that already had subagent intent is not new dispatch work. Full rules: `07-orchestration-commands.md` (queue-item claims).

Claim resolution and storage use the marker-free head text: a projected lifecycle prefix such as `⚠️` after requeue may select the claim attempt, but it is never copied into `QueueClaim.item_text`, CLI claim output, claim anchors, or generated start/release handles. Operator-authored priority pins remain part of the displayed head text.

**Activation resolution (preflight):** Preflight detects the `agent:run` component and resolves activation in priority order:

1. **Legacy `auto` attribute** — `<!-- agent:run auto -->` activates immediately when prompts exist, but binary-owned writes must strip it from any queue tag they touch.
2. **Start fence at head** — bare `--- start` is consumed and activates; `--- start at <time>` defers (emits `queue_deferred: true`, `queue_start_at`).
3. **Exchange trigger** — user writes `do queue` or `run queue` in the exchange.
4. **Persisted state** — `queue_active: true` in frontmatter (set on activation, cleared on drain).

On activation, preflight emits `queue_active: true`, `queue_prompts: [...]` (ordered prompt texts), and `queue_trigger` (how the queue was activated). The first prompt is the effective user edit for the cycle.

When the queue drains to empty: `auto` is stripped from the opening tag, `queue_active` is cleared in frontmatter.

**Consumption (Phase 3/4):** After a successful response write (via `finalize` or `write --commit`), strict closeouts first clear the remaining pending-maintenance / pending-guard gates. Only then is the consumed prompt marked complete in the `agent:run` block as `- ~prompt text~` before the commit boundary so the same git commit can capture both the response and the queue advance. Queue consumption requires one of three prompt proofs: the current diff contains the exact queue-head prompt, the run path supplied a queue-synthetic diff for the active head, or the closeout explicitly resolved the head `do #id` with `--done <id>`. An empty baseline/current diff, or an unrelated prompt such as `#next-steps` that was already present in the baseline, must not consume the active head by itself. Missing-response materialization repairs, including `write --commit` with empty stdin and Codex Stop-hook recovery, preserve the current queue head and `auto` attribute unless the repair carries explicit head proof such as a matching `--done <id>` or `### Re:` topic. If the live document head differs from the pre-turn snapshot head because the operator inserted a new queue prompt while the turn was running, that insertion is preserved as realtime future queue state; the current closeout consumes the snapshot/selected head if it still exists in the live queue, otherwise it no-ops/fails closed instead of consuming the operator's new line. If the closeout explicitly resolves multiple contiguous queued `do #id` prompts with repeated `--done <id>` flags, those head prompts are consumed together; consumption stops at the first unresolved or non-matching queue prompt. Queue-head proof normalizes leading priority markers (`:pushpin:`, `:round_pushpin:`, and their aliases) before classifying `do [#id]`, so a pinned id-backed head is not treated as free text and cannot be consumed merely because an unrelated response exists. The file and snapshot transforms are both proven first, then written in sync so change detection works on the next cycle.

- **Auto-strike of resolved head prompts (preflight maintenance):** Before dispatching the head, preflight walks leading `Prompt` entries and rewrites any whose `#id` is already resolved into `Completed` (`~text~`) entries. An id counts as resolved when present in `agent:done` (canonical) **or** in `agent:review` with the pending-gate marker `- [/]` (code-complete, awaiting an external gate). A newly admitted prompt or any currently-open backlog item is a fresh incarnation even when it deliberately reuses a historical done id (`#qrepeatid`), so adding the same queue task again executes it again; the live backlog checkbox keeps that incarnation authoritative across maintenance retries until closeout reaps it. A review-gated head carrying an explicit operator verdict after `[#id]:` is also a fresh incarnation: that queue item is the input that releases the gate, so it remains executable. The pending-gate inclusion unblocks queue stalls when a do-item lands a contained phase of a multi-phase plan but the parent item stays gated for remaining phases. Strike scanning stops at the first head prompt whose id is not resolved so the next live head stays intact for the normal consumption path. The strike telemetry log line records `source=done` vs `source=review_gated` so audit can see which path advanced the queue.
- **Spent prompt-preset pause repair (`#qpresetstrike`):** Controller dispatch must not replay a stale `queue_paused` receipt whose reason says `"<preset> preset head is spent"` after the live queue no longer contains that preset token. On dispatch it revalidates the pause against the current document: absent head clears the pause and proceeds; present registered preset head is consumed through the normal queue consumer, then the pause is cleared. Non-spent operator pauses remain fail-closed.
- **Stale-supervisor queue pause recovery (`#jbrestale`):** A `queue_paused` churn-stop caused by a stale route-owned supervisor is recoverable exactly once. Current controllers tag the bail with `supervisor_restart_redirect`; route also recognizes the legacy markerless `failed_stage=queue_paused reason=#qchurn ... stale host supervisor pid<N> ...` shape so an old supervisor can be restarted and retried instead of surfacing a JetBrains `Run Agent Doc` error. If the stale pause names a dead PID and the pause row predates the current system boot, dispatch clears that stale pause before blocking auto-start.
- **Backlog→queue sync is activation-scoped and non-destructive (`#backlog-queue-sync-pending-add-amplification`):** when an `agent:backlog` carries the `queue` attribute, preflight mirrors its active ids into `agent:run` only while the queue is **not yet a persisted-active go loop** (`queue_active` was not already `true` coming into the cycle). Once the loop is running, a freshly-added backlog id (e.g. a follow-up captured via `--backlog-add` mid-loop) is **held out** of the live queue — it joins on the next activation — so capturing follow-ups cannot grow the running queue unboundedly or trip `pending_done_guard` each finalize. `queue=sync` mirrors the id-backed backlog portion and may drop stale id-backed mirror entries, but it must preserve operator-authored no-id queue entries such as free-text bullets, fences, prose, and preset notes. `queue=append` and `queue=prepend` also preserve existing entries. Ids already present as queue heads are unaffected. `agent:icebox queue` is ignored with a warning; use per-item enqueue or move the item to backlog.
- **`go` opts into continuous-backlog-loop on a drained queue (`#backlog-queue-empty-active-repopulate`):** the activation-scoped hold above has one exception. When the queue carries the `go` control (frontmatter `queue: go` or marker-side `go`) **and** the live queue is **fully drained** (0 un-struck prompts), preflight repopulates from the full active backlog instead of holding — so a `go` queue keeps working the backlog continuously. Marker-side `start` is a start trigger, not a continuous-loop opt-in. Without `go` (a plain persisted-active queue), a drained queue stays drained (**drain-then-stop**). The repopulation is amplification-safe because the queue is empty, and self-terminating because only `Open` (`[ ]`) backlog items are mirrored: items processed and marked `[/]` (gated) or `[x]`/`agent:done` per the `do #id` closeout rule drop out, so the loop ends when no `Open` backlog item remains. If both queue and backlog are empty, `go` does not promote parked icebox work.
- **Boundary-displacement repair (`#queue-completed-items-escape-below-component`):** struck queue items must stay inside the `agent:run` component span (struck, until reaped to `agent:done`). A post-commit CRDT/boundary merge can displace a struck `- ~~…~~` queue line past `<!-- /agent:run -->` into the neighbouring parking-lot HTML comment, where it renders invisibly and accumulates as orphaned residue. `template::repair_queue_struck_items_escaped_below_marker` (run by preflight queue maintenance and by the visible-write normalizer) removes any struck queue-shaped line — body carrying a `:round_pushpin:`/`:pushpin:` marker or a `do`/`re` directive — that sits **outside every agent component span** below the queue close marker. Real component content, legitimately struck items still inside the queue, and ordinary scratch-comment text (generic `- ~~note~~` with no pushpin/directive) are left untouched.
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
3. Bare `---` (not followed by ` start` or ` stop`) opens a multi-line prompt fence; matching `---` closes it. A `---` line inside a balanced Markdown code fence (a pasted log) never closes it.
4. `--- start`, `--- start <time>`, `--- start at <time>`, `~~~start` → start fence.
5. `--- stop`, `~~~stop` → stop fence.
6. Blank lines between items are ignored.
7. Column-zero prose after a live free-text list item or thematic separator is a bare multiline free-text prompt. Consecutive prose and directly following blockquote lines stay in the same prompt; an attached blockquote also makes the paragraph self-identifying when it reaches the queue start after the preceding item is consumed. A thematic break (`---`, `***`, `___`) separates prompts. Other content outside list items, prompt fences, or control fences is inert `Freeform`; a balanced ordinary Markdown code fence is one inert block, so list-looking lines inside pasted evidence are never prompts.
8. A narrow recovery recognizes non-native prose plus a balanced Markdown code fence as one exact-span multiline prompt when an explicit request appears before or after the fence and no later non-empty queue-native item exists. Already-completed rows may precede it. A trailing empty list placeholder does not block recovery and is never runnable.
9. **Pasted multi-line heads (`#multilinequeuehead`).** A bullet-less title run (one or more non-indented prose lines) immediately followed by a balanced Markdown code fence is one multiline prompt when it is *introduced*: the previous non-blank line is a `---` separator, or it sits directly beneath another multi-line item (a `---` / `~~~prompt` / `~~~done` fence or another head of this shape). Further balanced fences separated only by blank lines belong to the same head. The head ends at its last fence closer, and the next non-blank line must be a boundary: the end of the queue, queue-native syntax, or another introduced title-plus-fence head. Each operator-pasted block is therefore its own head, in operator order (`#qauthorder`); a block pasted directly after another is never folded into it. A single `---` right after the last fence is the head's trailing separator when a boundary follows it; otherwise it stays to open the next `---` fence. An unpaired `---` directly above such a head is that head's leading separator, so it cannot later pair with a newly typed `---` and re-segment the queue. A lone title plus fence with no introduction stays inert evidence under rule 7 unless rule 8 or `#qfreetext-sep` claims it. The Markdown-AST overlay enumerates these heads with the same shared grammar (`overlay::queue_fenced_title_block`), so strike, claim, consume, and the residue guard address the same node. Only the head's first line can name its tracked id: a `[#id]` quoted inside the pasted log never makes the head id-backed. `queue claim` / `queue brief --item` accept the head's title line when exactly one live multi-line head starts with it; several heads with the same title are ambiguous and must be named by their full text.

Single-line list prompts and canonical multiline prompt fences are one ordered,
AST-addressable item sequence. Node-keyed consumption of a multiline live item
renders `~~~done` / `~~~` around the same prompt text; unstrike restores the
bare `---` / `---` live shape. A `~~~done` fence is the item's own completion
marker: the `#ftstrike` ` — auto-struck: …` note is appended only to a
single-line `~~text~~` strike, never to the fence's closing `~~~` line (an info
string there leaves the fence unclosed, swallowing the component's close marker
so every later projection fails with `unclosed component`). The strike also
drops the in-progress marker from the fence body's first line
(`#multilinequeuehead`). A narrowly recovered prose-plus-fenced-diagnostic
head that has no AST node key is consumed only through the exact byte range
returned by the queue parser; that range alone becomes a canonical `~~~done`
item, and later queue bytes are preserved. A malformed or unknown surface that
has neither an exact parser range nor an AST node key fails closed before
mutation.

**Single-line strike rendering.** Every single-line strike renders through
one formatter, `agent_doc_element_queue::render_struck_row_body`, as
`- ~~work~~`. This covers the Markdown-AST node-keyed consume
(`mutations::consume_nodes`, the structural `strike`/`consume` op that editors
apply through the native library), the bare-consume fallback of the orphan
`queue consume --id` strike, and the queue-entry strike paths that build a
completed entry. `work` is the row's trimmed text without its attempt
projection prefix (`strip_attempt_projection_prefix`): lifecycle markers
(`☑️`, `🔄`, `📥`, …), pins, exact-attempt owner annotations
(`[#subagent:x#1]`, legacy `⟦…⟧`), and the bare `🙋` glyph never land inside
a strike. A row that is only projection keeps its trimmed text. The closeout
`#ftstrike` note may follow *outside* the wrapper (`- ~~work~~ — auto-struck:
…`). Past the projection prefix, `work` is kept byte for byte: the strike
never escapes, re-pairs, or rewrites backticks, links, or images inside it.
Under GFM (cmark-gfm, markdown-it in VS Code, the `intellij-markdown` parser
in JetBrains, `pulldown-cmark`) that wrapper is exactly one strikethrough span
over the whole work text, including work text with inline code at either
edge, unbalanced backticks, `~~` inside a code span, links, and images. Code spans pair *inside* the strike and cannot move its
delimiters. Readers return `work` as the item text. Striking a struck row again
is a no-op, and the `#ftstrike` note is never added twice. A struck row whose
code spans look wrong is showing its own operator text. For example, a row that
quotes another row in single backticks, where the quoted row holds its own
backtick spans, pairs its backticks the same way before and after the strike.
Quote such text with a longer backtick run (```` `` … `` ````) instead. Known
gap: work text that begins or ends with `~`, or ends with `\`, does not render
as a strike (a leading `~` turns `~~~…` into a fenced code block).
