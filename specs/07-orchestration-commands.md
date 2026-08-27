> Extracted from [07-commands.md](07-commands.md)

# Orchestration Commands

FlowCore `orchestration_batch` owns batch-level invariants: freeze the source
task list at start, normalize each child response through the same patchback
contract used by closeout, stop before the next child when the task list changes
mid-run, and record each child result as structured proof. Existing
`orchestrate.rs` / `queue_dispatch.rs` behavior remains the execution surface,
but new queue/orchestrate fixes should map child closeout outcomes to FlowCore
instead of adding free-form batch state.

This file covers binary-owned planning/orchestration and the queue surface that reuses the same document lifecycle.

## plan

`agent-doc plan <FILE>`

- Emits the binary-owned planning record for the current cycle:
  - `prompt_targets`
  - `dispatch_candidate`
  - `task_class`
  - `risk`
  - `parallelizable`
  - `suggested_parent_tier`
  - `agent-doc-model-tier`
  - `dispatch_mode`
  - `context_budget_tokens`
  - `job_packet_budget_tokens`
  - `write_scope`
  - `required_proof`
  - `tsift_context`
  - `execution_scope`
  - `repo_actions`
  - `required_commands`
  - `pending_mutations`
  - `graph_evidence` when a materialized `.tsift/graph.db` exists and the prompt targets include queued `do #id` / `do [#id]` work
  - `handoff`
  - `blockers`
- The implementation reuses the same prompt/diff classifiers that power `preflight`.
- `pending_mutations.resolve_existing` signals that a matching open backlog/icebox item will need `--done <id>` if completed this cycle.
- For explicit `do #id` / `do [#id]` directives that resolve an existing tracked-work item, the planned finalize command includes the matching `--done <id>` argument so the harness closeout path does not rely on the agent remembering that flag manually.
- `pending_mutations.expect_add` signals that the response likely needs new backlog capture.
- `execution_scope=plan_backlog_only` suppresses repo implementation work for report/planning contracts such as `#agent-doc-bug`.
- Lower-agent routing fields are deterministic structural hints, not authority
  to skip parent review. `dispatch_candidate=true` means the prompt is shaped
  well enough for job packets; `agent_doc_dispatch: off` disables that
  candidate flag, while `agent_doc_dispatch: auto` records an opt-in for future
  automatic dispatch. `task_class`, `risk`, model tier, token budgets,
  `write_scope`, and `required_proof` come from prompt targets, backlog text,
  requested presets, and visible repo-work directives.
- `tsift_context` records the stable JSON handoff commands used by job packets:
  `tsift status --json`, `tsift context-pack <FILE> --json --budget normal`,
  `tsift source-read <handle>`, `tsift diff-digest --cached`, and
  `tsift test-digest --input`. Missing, stale, or failing context is advisory at
  the agent-doc turn boundary: continue without tsift graph/context evidence,
  but keep the diagnostic visible to workers and parent review.
- Copied `prompt_presets` frontmatter defines reusable prompts but does not invoke them; plan/preflight must ignore preset definition lines when expanding preset references from added diff text.
- **`#preset-item-id-collision`: each `#id` has one active meaning per document.** Preflight's `detect_identity_collisions` builds an identity registry over frontmatter `prompt_presets` keys plus active (not done) `agent:backlog` / `agent:review` / `agent:icebox` item ids (ids normalized by stripping a leading `#`; archived/done ids excluded as non-active lookup targets). When the same `#id` resolves under more than one source — e.g. a `#next-steps` preset *and* a `#next-steps` backlog item — `do #id`, queue generation, and "top backlog item: #id" are ambiguous, so preflight emits a `preset_item_id_collision` warning naming each colliding identity, its sources, and the rename repair. The same id duplicated across active tracked components also flags. Plan: `tasks/agent-doc/plan-preset-item-id-collision.md` (hard fail-closed dispatch block + mutation-time rejection are the follow-up `#preset-item-id-collision-enforce`).
- When graph evidence is active, `plan` calls `tsift graph-db --path <FILE> --json status`, refreshes the projection with `tsift graph-db --path <FILE> --json refresh`, then calls `tsift graph-db --path <FILE> --json evidence <id> --depth 3 --limit 8`, `tsift conflict-matrix --path <FILE> --json <id...>`, and `tsift dispatch-trace --path <FILE> --json <id...> --depth 3 --limit 8`. Stale/fail-closed graph freshness, unresolved targets, bad JSON, timeouts, or missing contract fields produce warnings and a no-graph/manual-packet fallback instead of blocking the whole turn. When collection succeeds, the emitted `graph_evidence` record carries the `conflict-matrix-v1` and `dispatch-trace-v1` planner contracts: evidence packet ids, graph node handles and edges, context-pack summary, cached diff and impact commands, ranked candidates, shared file/symbol/test/config conflicts, worker ownership block labels, worker feedback summaries, first-class worker prompt packets, token budgets, semantic ranking reasons, projection hashes, replay/repair commands, and warnings.

## orchestrate

`agent-doc orchestrate <FILE> --mode sequential|parallel|dag [--task TEXT ...] [--from-file TASKS.md] [--from-exchange] [--from-queue] [--resume-schedule ID] [--agent NAME] [--model MODEL] [--dry-run] [--plan]`

- Natural-language orchestration requests are normalized by the skill/runbook layer into this command; the CLI itself expects explicit tasks and mode.
- Task resolution combines repeated `--task` entries, optional task extraction from a file, and optional task extraction from the newest exchange tail.
- `--from-queue` extracts all active `agent:queue` prompt entries and queue-level `preset` / `dispatch` directives, rather than only the single queue head used by normal `agent-doc run` resumability.
- Batch-level `preset` / `presets` directives request frontmatter `prompt_presets` and are validated before execution. Exact preset keys win; a bare directive such as `preset review` also resolves to frontmatter key `#review` when only the hashtag form is defined.

### `--dry-run` and `--plan`

- `--dry-run` prints the task labels and exits before preset expansion.
- `--plan` prints the fully expanded per-task prompt after preset expansion and before execution.

### Shared subprocess arg rules

- Orchestration subprocesses inherit explicit harness arg resolution from document frontmatter and config.
- Streaming subprocess stderr is surfaced explicitly on failure instead of collapsing into an "empty response" error.
- When the live session has warn/block accretion and the current diff still contains prompt targets, fresh orchestration agent requests use a bounded response-context pack instead of replaying the full exchange tail. The pack must include the active prompt targets, the current `### Session Summary` block when present, the head of `agent:backlog`, and the `### Re:` turns anchored to the prompt positions in `exchange` (enclosing response for inline edits, immediately previous response for tail prompts).

### Pre-auto-run recovery tag (`#misfire-recovery-snapshot`)

Before a **queue-sourced** auto-run (`--from-queue`, sequential or dag) executes its first task — i.e., after the empty-batch / `--dry-run` / `--no-git` / `--plan` guards but before any task mutates the document or backlog — `orchestrate` drops a lightweight git tag at HEAD: `agent-doc/<doc-name>/pre-auto-run-N` (next unused ordinal). This mirrors `compact`'s `pre-compact-N` checkpoint via the shared `create_pre_mutation_tag` helper, so a misfiring multi-cycle auto-run is recoverable (`git reset`/inspect the tagged HEAD) without git/sidecar archaeology. The tag is best-effort and non-fatal — a tag failure logs a warning and the run continues. It is skipped for non-queue runs (`--task`, `--from-file`, `--from-exchange`), `--dry-run`, `--plan`, and `--no-git`. Keep this aligned in `orchestrate.rs`, `compact.rs` (`create_pre_mutation_tag`), and this spec.

### Command classification

- Queue/orchestration items starting with `/` are commands; everything else is a prompt.
- The managed idle-queue continuation path uses the same classification. When an active
  queue head is a slash command (for example `/clear` or `/model sonnet`), the owner
  pane receives that literal command at the next idle prompt instead of an
  `agent-doc <FILE>` reopen. After successful delivery, agent-doc marks that
  queue head complete, commits the queue mutation, and resumes draining the
  remaining queue.
- Codex Stop-hook and `session-check --codex-final-gate` diagnostics must report slash
  command heads as commands to submit, not as prompts to answer.
- A diff whose substantive added lines are only slash commands (for example a
  user appending `/clear` at the `agent:exchange` tail) is a command handoff, not
  prompt work: preflight must surface it in `slash_commands` / `builtin_commands`
  and must not open `preflight_started`, emit `user_intent_prompt_changes`, or require
  a finalize/write closeout for that diff.
- Dispatch priority is:
  1. Inline execution for binary-owned commands such as `/model` or `/compact`
  2. Supervisor IPC
  3. `tmux send-keys` through the authoritative pane
  4. Fail closed

## `--mode sequential`

Sequential orchestration reuses the same document lifecycle once per task:

1. Inject `❯ <task>` into `agent:exchange`.
2. Run `agent-doc preflight <FILE>`.
3. Expand requested presets into the concrete task prompt.
4. Send exactly one fresh backend request with no resume/fork carryover.
5. Persist the final response through `agent-doc respond <FILE> --stream --origin skill` (`finalize` compatibility alias).
6. Consume the strict finalize result as the terminal closeout report; it already includes the binary-owned session check. Do not spawn a second `agent-doc session-check <FILE>` from managed orchestration.

Additional rules:

- Sequential orchestration requires normal git-backed finalize; `--no-git` is invalid.
- CRDT streaming is provisional and non-authoritative until the one strict `finalize` closeout succeeds, including its internal session check. No partial chunk is patched into the session document.
- While a CRDT streaming step is still generating, partial output may be checkpointed only through the shared `state.db` recovery ledger so pane death does not discard all in-progress assistant text. The finalizer always receives the complete response, never a suffix computed from a document-visible prefix.
- Template-mode orchestrate closeout must persist through an explicit `patch:exchange` block. If a managed child returns a clean single plain assistant response, orchestrate wraps that body as an exchange patch before `finalize` so closeout does not enter the zero-template-patch write path.
- Child template responses may be either a clean single assistant response body, which the write path synthesizes into an `exchange` append, or explicit patch blocks that include `patch:exchange`. Orchestration must still fail closed for transcript-shaped output, full-document/component dumps, mixed patch plus unmatched text, or multi-component writes that omit explicit patch blocks.
- When `--from-exchange` resolves a sequential batch from a live markdown task list, orchestration freezes that source list at parent start. After each child closeout and before launching the next step, it rechecks the originating list; inserted, removed, or reordered tasks stop the run deterministically, write an exchange response explaining that the batch changed mid-run, leave remaining/new tasks open for the next explicit run, and return a non-success status after `session-check` passes.
- Batch freeze, mid-run source mutation, child closeout success/failure, and child template-response normalization must emit FlowCore `orchestration_batch` events. Clean single plain child responses are normalized into `patch:exchange` before finalize; transcript-shaped output, mixed patch/unmatched output, full-document dumps, and explicit patches without `exchange` stay rejected by the shared document-mutation patchback contract.
- Graph-backed child closeouts must add a hidden tsift-projectable `worker_result` line to the exchange patch before `finalize`. The line records completed/blocked status, the target id, lower-agent owned/touched files, expected tests as markdown code spans, and follow-up ids from the child response so the next graph-db projection can surface worker-result feedback without parsing CLI logs.
- Parent-owned lifecycle commands (`preflight`, `finalize`, and `session-check`) must spawn a launchable `agent-doc` binary without relying solely on ambient `PATH`; if the running `current_exe()` path is stale after a local install, orchestration falls back to the invoked command or `agent-doc` on `PATH` and includes binary/cwd/PATH context in spawn errors.

## `--mode parallel`

- Resolves tasks and presets first, then hands them to the existing worktree fan-out backend.
- The legacy `agent-doc parallel` entry is only a compatibility wrapper over this same dispatch path.
- If a materialized tsift graph database exists and resolved tasks include `do #id` / `do [#id]` items, orchestration attempts to collect the same graph evidence, conflict matrix, and dispatch trace as `plan`. Sequential and DAG child prompts receive a bounded `<tsift_graph_evidence>` JSON block outside the document mutation only when graph evidence is available. Parallel worktree job packets receive the same block in the task prompt when available, including a normalized `lower_agent_job_packet` derived from the worker prompt packet's owned files, read-only context, forbidden files, expected tests, expansion commands, token budget, fail-closed prompt text, dispatch-trace graph links, worker feedback, projection hashes, and replay/repair commands. If graph collection fails, orchestration warns and continues without the graph block; if collection succeeds, parallel mode still fails closed when the conflict matrix reports `can_parallel=false` or `fail_closed=true`.

## `--mode dag`

- Supports `id=` and `after=` / `deps=` metadata prefixes on task lines.
- Fails fast on unknown deps, duplicate ids, and dependency cycles.
- Executes in deterministic topological source order.
- Every ready node still runs through the normal single-document lifecycle: inject prompt -> `preflight` -> fresh agent request -> strict `finalize`.
- DAG mode is dependency-aware but not concurrent against one session document. Real concurrency belongs to `--mode parallel`.

### Auto-DAG from queue

`agent-doc orchestrate <FILE> --mode dag --from-queue`

- Builds a binary-owned `agent-doc-auto-dag-schedule-v1` schedule from the active `agent:queue` body.
- Expands compound `do [#a] [#b]` directives into one node per target and parses dependency text from `after=`, `deps=`, `after #id`, `depends on #id`, `blocked by #id`, and `requires #id`.
- Computes deterministic source-order antichain batches, persists the schedule under `.agent-doc/schedules/<schedule-id>.json`, and writes schedule-backed job packets plus an operation note before launching work.
- The persisted node record carries state, attempt count, replay commands, repair commands, dependency ids, graph status, and guard decision. `--resume-schedule <ID>` reloads that file, skips complete nodes, and refuses to launch dependents when any node is blocked or failed.
- If tsift graph evidence is available, auto-DAG validates current/fresh graph status, one evidence handle per target, one worker ownership packet per target, and conflict-matrix approval before dispatching multi-node antichain batches. Stale graph evidence, ambiguous ownership, or unsafe parallel conflict evidence fails closed before dispatch.
- Auto-DAG reads recent ops logs as scheduler input and classifies prompt-budget, cache-resend, restart-loop, and noop-closeout families. Prompt-budget/cache-resend gates the run into compact-first; restart-loop gates into restart/repair-first; repeated noop-closeout gates into fixture-fix-first. The schedule is written with the guard result so recovery has proof of why the run did not launch.
- Auto-DAG still uses the shared single-document lifecycle for each launched node. A downstream batch is not considered ready until every node in the prior dependency set has completed through `finalize` and `session-check`; failed children update the schedule and stop the run deterministically.

## jobs

`agent-doc jobs create <FILE> [--operation-doc] [--audit] [--budget N]`

- Reads the current `agent-doc plan` record and emits one markdown job packet
  per distinct target in the union of `resolve_existing` tracked-work mutations and
  all `#id` references inside supported `do #id` / `do [#id]` repo actions.
  A compound directive such as `do [#a] [#b]` therefore creates packets for both
  targets instead of silently dropping the later ids.
- Packets live under `.agent-doc/jobs/<cycle>/<job-id>.md` and are ignored by
  default. `--audit` records the preservation intent in the cycle index.
- Auto-DAG schedules may also create schedule-backed packets under
  `.agent-doc/jobs/<schedule-id>/`; these packets include
  `auto_dag_schedule_id`, `auto_dag_node_id`, node state, attempt count,
  replay commands, and repair commands in addition to the normal worker result
  contract.
- Each packet carries the `agent-doc-job-packet-v1` contract in frontmatter:
  parent document, cycle id, job id, prompt target, task class, model tier,
  risk, write scope, context budget, source snapshot, tsift status, and result
  sidecar path.
- Packet `write_scope` is target-specific: explicit `scope:` /
  `write_scope:` path references in the target backlog text win over the
  broader planning default, so cross-repo operation documents can dispatch
  packets to the files the item actually names.
- Packet body sections are stable: Goal, Allowed Commands, Required Context,
  tsift Handles, Acceptance Criteria, Output Schema, Escalation Conditions, and
  Worker Result.
- The worker result schema is `agent-doc-worker-result-v1` with:
  `status`, `changed_paths`, `commands_run`, `touched_files`,
  `expected_tests`, `follow_up_ids`, `findings`, `proof`, `confidence`, and
  `needs_parent_attention`. Collection fails closed when required fields are
  absent.
- When `tsift status --json` and `tsift context-pack --json` are available,
  create writes a compact `<job-id>.context.json` sidecar and links it from the
  packet. If tsift is missing or fails, the packet records the diagnostic and
  remains manual-review only.
- When a materialized tsift graph projection is available, packet creation runs
  the graph orchestration gate from `agent-doc plan`: `graph-db evidence`,
  `conflict-matrix`, `dispatch-trace`, worker prompt packets, worker-result
  feedback, replay commands, and repair commands must carry their contract
  fields before graph acceptance evidence is attached. Graph collection is
  bounded by `AGENT_DOC_TSIFT_GRAPH_TIMEOUT_SECS` (default 30 seconds per tsift
  command); timeout or any other tsift failure falls back to
  `manual_packet_only=true` with the diagnostic preserved.
- `--operation-doc` writes a retained operation note under
  `tasks/agent-doc/operations/` when that tracked directory exists, otherwise
  under `.agent-doc/operations/`.

`agent-doc jobs list <FILE> [--json]`

- Lists generated cycles and packet paths for the parent document.

`agent-doc jobs status <FILE> [--json]`

- Reports each packet as `open`, `result_sidecar`, or `embedded_result`.

`agent-doc jobs collect <FILE> [--cycle ID] [--json]`

- Reads `<job-id>.result.json` sidecars or embedded `## Worker Result` JSON
  blocks and returns a parent-review bundle. Collection never applies patches
  or marks backlog items done; the parent session still owns merge, tests, and
  `finalize`.

## `agent:queue`

The `agent:queue` component batches prompts inside the document.

### Syntax

- Single-line prompts use flush-left `- ` list items.
- Completed single-line prompts render as `- ~prompt text~` and are skipped by dispatch.
- Optional-`do` grammar (queue lines + closeout directive guards): the `do` verb is optional for id-backed kick-off, and a `re` verb references a task without running it.
  - `do [#id]` / `do #id` — execute, id-backed (back-compat).
  - bare `[#id]` / `#id` at the line head — execute, id-backed (`do` optional). A trailing `:` (`[#id]: note`) keeps the line inert prose, not a directive.
  - `re [#id]` / `re #id` — *reference only*: never executed, synced, or reaped (parsed as `Freeform`, preserved verbatim). The literal `re ` verb is required, so prose like `rebuild` / `re-run` / `reference …` is unaffected.
  - free text — execute as a prose prompt (unchanged).
- Batch-level preset directives may use `preset <name>` lines in the queue body. The queue parser also accepts `dispatch <name>` as a preserved batch directive so older queued batches can close out without parse failure.
- Multi-line prompts use `~~~prompt ... ~~~` or bare `--- ... ---` fences.
- Control fences:
  - `--- start` / `~~~start`
  - `--- start at <time>` / `--- start <time>`
  - `--- stop` / `~~~stop`

### Data model

- Queue entries are parsed as `Prompt`, `Completed`, `Preset`, `StartFence`, or `StopFence`.
- Activation resolution considers `auto`, inline start fences, exchange-triggered `do queue` / `run queue`, and persisted `queue_active`.

### Canonical queue control (`queue:` frontmatter, `#queue-state-unify`)

The `queue:` frontmatter key is the canonical, harness-agnostic queue activation
control. It subsumes the deprecated `queue_active:` boolean and the deprecated
`auto` attribute on the `agent:queue` marker:

| `queue:` value | meaning | subsumes |
|----------------|---------|----------|
| `start` (alias `go`) | activate — drive the queue | `queue_active: true` + marker `auto` |
| `stop` | deactivate — halt | `queue_active: false` |
| (absent) | unmanaged / inactive | absence of both |

- The value is parsed leniently (case- and whitespace-insensitive). An
  unrecognized value (typo) is ignored and never flips activation.
- `frontmatter::normalize_queue_control` folds a recognized `queue:` value onto
  `queue_active` at parse time, so every existing reader (preflight activation,
  Claude `/loop` continuation, Codex Stop-hook, OpenCode auto-loop) honors it
  without a separate read path. The canonical key wins over a stale
  `queue_active:` line in the same frontmatter.
- `queue_active:` and the `auto` marker attribute remain accepted as deprecated
  input for backward compatibility; new documents should use `queue: start` /
  `queue: stop`.

#### Marker-side control (`start` / `go` / `stop` on `agent:queue`)

`start`/`go`/`stop` are also accepted as **marker** control tokens on the
`<!-- agent:queue ... -->` opening tag — the ephemeral gesture spelling of the
frontmatter control:

- `<!-- agent:queue go -->` / `<!-- agent:queue start -->` fresh-activates the
  queue, identical to the legacy `auto` attribute (routed through the Auto
  trigger). `go` is an alias for `start`.
- `<!-- agent:queue stop -->` forces the queue inactive this cycle.
- The control token is stripped from the opening tag once the queue drains or a
  `stop` halts it, so it never re-triggers on the next cycle.

These marker tokens are recognized queue-only attributes (no
`misplaced_component_attr` typo warning).
They are canonicalized as bare tokens, never `key=true`: preflight repairs
boolean-serialized forms such as `priority=true` / `go=true` and the malformed
`preset="#name"=true` suffix back to `priority` / `go` and `preset="#name"` in
both the visible document and snapshot before continuing queue maintenance.

Queue maintenance binds the marker gesture and `queue:` frontmatter
bidirectionally (`#qactsync`). It compares the current editor-authoritative
document with the stable pre-turn baseline: when exactly one representation
changed, that operator edit wins and is projected to the other representation.
An explicit marker token also wins when no baseline is available, so a fresh
`go`/`start` gesture can activate a document whose durable frontmatter still
reads `queue: stop`. If both representations changed incompatibly in one
window, canonical frontmatter wins deterministically and the conflict is
logged. A second convergence pass over the synchronized document is a no-op.

The route/dispatch path honors marker-side control identically to preflight: an
inactive document (no `queue: start` / `queue_active: true`) whose `agent:queue`
opening tag carries `go`/`start` is recognized as an activatable head, so
`agent-doc route` (and the JetBrains `Run Agent Doc` action it backs) starts the
queue even when the frontmatter still reads `queue: stop`. A marker-side `stop`
keeps the queue inert on that path and wins over `auto`/`go`/`start`.

#### Writer emits canonical `queue:` (phase 4)

Queue-maintenance write paths persist the canonical control directly:
`frontmatter::merge_queue_state` writes `queue: start` on activation and
`queue: stop` on drain/halt, clearing any deprecated `queue_active:` line in the
same write. Both fields are normalized away together by the replay-hash /
boundary-compare paths (`strip_queue_active_frontmatter`,
`strip_route_queue_state_for_boundary_compare`), so a legacy `queue_active:` and
a migrated `queue: start|stop` compare equal and do not regenerate the
snapshot/HEAD drift loop. Reads continue to resolve through
`normalize_queue_control` (`queue:` → internal `queue_active`), so the deprecated
field remains accepted as input for backward compatibility.

- **Remaining (`#queue-state-unify` follow-ups):** harness-parity doc rewrites
  (Claude `/loop`, Codex Stop-hook, OpenCode auto-loop instruction surfaces) are
  tracked separately.

### Backlog→queue sync (`queue` attribute, `#backlog-queue-sync-attr`, `#queue-enqueue-action`)

An `agent:backlog` component may carry a `queue` attribute so the binary keeps
`agent:queue` populated from the active backlog instead of requiring a manual
`--backlog-reorder` + hand-regenerated queue each cycle. `agent:icebox` is a
parking lot, not an automatic scheduling source: a drained queue and drained
backlog remain terminal until the operator moves work to backlog, adds a manual
queue item, or marks a specific parked item with a per-item enqueue token.

- `<!-- agent:backlog queue -->` (bare token) — **append** (the default).
- `<!-- agent:backlog queue=append -->` — add `do [#id]` for active backlog ids
  not already referenced in `agent:queue`; existing entries and order are
  preserved. Struck (`Completed`) and active (`Prompt`) `do [#id]` entries both
  count as already-present, so a consumed id is never re-appended.
- `<!-- agent:backlog queue=prepend -->` — like `append`, but new prompts are
  inserted at the front of the queue, in backlog order.
- `<!-- agent:backlog queue=sync -->` — the queue body is **fully regenerated**
  as the active-backlog `do [#id]` list, in backlog order. Any other queue
  content (manual presets, fences, struck items) is dropped; use `append` /
  `prepend` to preserve manual queue content.
- Per-item enqueue markers — an open backlog/icebox item containing
  `:inbox_tray:`, `/enqueue`, or a Markdown-decorated `enqueue` token such as
  `**enqueue**` is appended to `agent:queue` as `do [#id]` even when the component
  does not carry the `queue` attribute.

Semantics:

- "Active" backlog items are open `[ ]` items with an id. Gated (`[/]`) and done
  (`[x]`) items are excluded — they are not actionable queue targets. An open
  item whose `not-before=YYYY-MM-DD` scheduling precondition is still in the
  future is also excluded until its date arrives (`#backlog-not-before`; see
  `specs/pending-system.md`), so a `queue`-attributed backlog never enqueues work
  scheduled for later.
- Per-item enqueue markers are idempotent and opt in only the marked open item:
  unmarked siblings are ignored, gated/done marked items are excluded, and
  existing active or struck `do [#id]` queue entries are not duplicated.
- The sync runs in `run_queue_maintenance` **before** activation resolution, so a
  freshly synced queue can activate on the same cycle when the queue opening tag
  carries the legacy/manual `auto` trigger, a start fence is ready, or
  `queue_active: true` is already persisted. The `queue` attribute
  populates/maintains the queue; it does **not** itself start the loop.
- The sync is idempotent: when the queue already matches the requested shape it
  mutates nothing, so it is safe to run on every preflight cycle.
- **Editor-safe persistence (`#fccqueue`).** When queue maintenance mutates the
  document (queue body sync, opening-tag activation-token strip, `queue:`
  frontmatter state), it must not raw-write the session document to disk behind a
  live editor. With a JB editor IPC listener active it routes the queue shape
  through the editor convergence (`converge_live_buffer_queue_shape` → plugin
  `setText` + `saveDocument`, no IntelliJ `File Cache Conflict` dialog) and skips
  the `std::fs::write`, recording `write_authority action=routed
  surface=queue_maintenance` in `ops.log`. With no listener and no live editor
  owner it writes to the detached disk replica as before, so non-IDE behavior is
  byte-identical. The legacy live-buffer sidecar is not a document-authority
  source. This brings queue maintenance to
  the same `#fcc0` converge-or-disk discipline the pending/review maintenance
  sites already use, and closes the 08b gap where preflight queue maintenance
  bypassed the write-authority routing the finalize/response path uses. The
  private `.agent-doc/` snapshot is still written directly (never open in the
  IDE, so it cannot conflict).
- **Remaining session-doc write-site audit (`#fccaudit`).** Extending `#fccqueue`,
  every session-document disk-write site is audited against an active editor
  listener and either routed through the `#fcc0` converge gate
  (`converge_or_disk_write` / `converge_document_or_disk`) or documented as
  editor-safe:
  - *Normal-path writes routed through the converge gate:* the
    `agent-doc write --status` component replace (`status_set`) and the
    post-commit ephemeral guard-marker strip (`strip_guard_markers`, removing
    `<!-- no-pending-capture -->` / `<!-- no-pending-done-guard -->`). With a
    listener active they converge through editor IPC (no `File Cache Conflict`);
    with no listener and no live editor sidecar they fall back to the same
    byte-identical detached disk write.
  - *Already editor-safe:* exchange compaction (`#w42v`), the post-commit
    boundary reposition (skips the working-tree write while a listener is active
    and lets the IPC reposition signal own the buffer), and the `#pcwc`
    post-commit worktree reconcile. The `#pcwc` reconcile was promoted to
    listener-aware in `#pcwcdiskfree`: with a JB editor listener active it first
    tries to converge HEAD content through the editor-buffer IPC refresh,
    skipping the working-tree disk write only when that refresh is acked. If the
    refresh no-acks/errors, it falls back to the authoritative HEAD disk write
    and logs `transport=disk_after_failed_editor_refresh`, because leaving the
    corrupted working tree in place would re-seed the next drift cycle. With no
    listener (headless / CI), it still writes HEAD to disk authoritatively so
    committed content is restored even when no editor is attached.
  - *Authoritative must-hit-disk by design:* the recovery / scaffold / migration
    writes (`claim` scaffold before any editor attaches, `repair` orphan
    recovery, preflight format migration/repair, `session-check`
    over-application remedy, `reset` resume-clear). These restore correctness
    when the editor or IPC path may itself be wedged, so they must write disk
    authoritatively rather than fail closed on a live but unresponsive listener.
  The `(HEAD)` response-heading annotations and `agent:boundary` markers are
  *not* in scope: the working tree and editor buffer deliberately preserve them
  so the user sees which headings are new.
- **Active-loop population (`#backlog-queue-sync-pending-add-amplification` /
  `#backlog-queue-attr-populates-in-go-mode`).** While a queue is *persisted-active*
  (`queue_active: true`) the population rule depends on the loop mode:
  - **plain persisted-active** (no `go`) — freshly-added backlog ids are
    *held* out of the running loop (only ids already present as queue heads sync),
    so an agent capturing follow-ups mid-loop cannot amplify the queue unboundedly
    or churn `pending_done_guard`. Held ids join on the next activation. Explicit
    per-item enqueue markers bypass this hold for the marked id.
- **go-mode** (`queue: go` or marker-side `go`, the continuous-backlog-loop opt-in) — fresh
    backlog `queue`-attr ids and pre-existing gated ids explicitly ungated by the
    closeout are mirrored immediately (not only when the queue drains), so the
    `queue` attribute populates the live queue as intended. Append/Prepend
    stay idempotent and processed items drop out of `active_item_ids` once marked
    `[/]`/`[x]`, so the queue stays bounded by the open backlog. Context-accretion
    thresholds must not stop this self-driving loop: direct `run` starts the next
    queue item in a fresh backend session, while managed pane go-mode interleaves
    the harness-native clear command (`/clear`, or OpenCode `/new`) at an idle gap
    and then continues draining the same head.
- **Same-closeout actionable reconciliation (`#backlogqueuepopulation`).**
  Adds and ungates record a mutation-scoped `pending_actionable_ids` set. After
  current-head consumption, closeout insert-only mirrors exactly that set into
  the explicit queue, ordering the new block by hard `after=#id` dependencies
  before priority and preserving every existing queue byte. Equal-priority items
  retain backlog document order, which is also the caller's top-down order for a
  repeated `--backlog-add` batch; generated ids are never scheduling tie-breakers.
  It never sweeps all open backlog ids at closeout, because doing so would
  resurrect unrelated operator-deleted queue entries.
- `agent:backlog` and the legacy `pending` alias may carry the attribute; the
  first queue-tagged component's mode wins and active ids from every queue-tagged
  source are taken in document order. `agent:icebox queue` warns and does not
  sync parked items.
- The pure logic lives in `queue::sync_backlog_into_queue` (with
  `queue::BacklogQueueSyncMode`) and `pending::active_item_ids` /
  `pending::active_enqueue_item_ids`. The `queue` key is a recognized
  backlog/pending attribute (the `tagpath` agent-doc lint accepts bare `queue`
  and `queue=sync|append|prepend`, warning `agent-doc/invalid-attr-value` on an
  unrecognized mode; preflight's `misplaced_component_attr` mirrors this).
- **Priority interplay (`#backlog-priority-attribute`).** A bare `priority`
  attribute on the source backlog/icebox marker stable-sorts its items by their
  per-item `priority=<1..9>` token before the sync runs (in `run_pending_maintenance`,
  earlier in the pipeline), so the synced queue inherits the prioritized order.
  A queue-tagged backlog source that also carries `priority` additionally runs
  the synced queue through the same priority/auto-DAG recompute as
  `agent:queue priority`, so append-built queues are reordered immediately even
  when the queue marker itself has no `priority` token. Automatically promoted
  queue prompts are annotated with `:round_pushpin:`; manual priority dispatches
  use `:pushpin:`. A bare `priority` attribute on the `agent:queue` marker still
  stable-sorts `do [#id]` prompts by their source item's priority after the sync
  (`queue::sort_prompts_by_priority`), covering manually edited queues.
  `priority` is recognized on backlog/icebox/queue by both the lint and
  preflight. See `specs/pending-system.md` for the per-item token grammar.
- **Append-stable backlog scheduling (`#backlog-queue-append-stable`).** A
  `do [#id]` queue prompt whose id is an **active backlog id** is *backlog-sourced*:
  the `queue` attribute appends it at the tail. Under `priority`, the recompute
  holds backlog-sourced prompts in a group **after** the pre-existing unpinned
  prompts (manual operator lines and free-text prompts already in the queue)
  instead of interleaving them by backlog rank — so a scheduled item **appends,
  not prepends, even when non-annotated items are already in the queue**. Within
  the backlog-sourced group, backlog-priority rank then document order are
  preserved. Operator (`:pushpin:`) and agent (`:round_pushpin:`) pins are exempt
  — a pin is an explicit position signal that outranks append-stability (a brand-new
  operator-typed queue line is operator-pinned by `#7r2s`, so it keeps its authored
  slot). The group is keyed off the active backlog id set, not a "new this cycle"
  diff, so a previously-synced item stays append-stable on later cycles rather than
  floating up once it is no longer fresh. Implemented in `sort_prompts_by_priority`
  / `sort_prompts_by_dag` (the `backlog_sourced` group key). `queue=prepend` remains
  the explicit opt-in for front-insertion.

### Preflight queue behavior

Preflight owns queue activation/deactivation and emits queue state fields such as:

- `queue_prompts`
- `queue_active`
- `queue_deferred`
- `queue_start_at`
- `queue_trigger`
- `queue_halted`

Before emitting queue state, preflight may:

- consume a bare start fence
- strip `auto` on drain
- persist `queue_active`
- halt on a stop fence
- halt on a future time gate
- halt when the current head prompt was **edited in place** since the snapshot for a queue that was already active — i.e. the snapshot head prompt is no longer present anywhere in the current queue (`detect_head_prompt_modified`). A new item inserted (or reordered) ahead of a still-present in-flight head is a re-prioritization, **not** an `item_modified` edit: the queue advances to the new head and stays active rather than halting and stranding the remaining live prompts as inactive residue (`#completed-queue-residue-regression` / `#queue-auto-no-continue`).
- collapse only duplicate durable markdown-AST queue node keys; preflight must not deduplicate active `do [#id]` or free-text prompts by raw text/id, because repeated prompts can be intentional queue work (`#md-ast-document-model`, `#queue-dedup-destroys-intentional-duplicates`). The subsequent lifecycle convergence pass (`converge_queue_via_lifecycle`) then caps EVERY head shape — free-text, bare `[#id]` reference, AND `do [#id]` directive — at its **snapshot-authored multiplicity** (min 1): an "intentional duplicate" is one the operator committed in the snapshot, so a copy beyond that count is a CRDT/backlog re-emit artifact and collapses (`#qdedup-directive-twin`). A `do [#id]` directive shape must NOT get an unbounded allowance, or a byte-identical CRDT live-edit re-emit twin masquerades as intentional and survives forever ("restored the queue items I deleted"). Free-text convergence identity and the CRDT cell-merge free-text key are marker-invariant (`strip_priority_markers` covers `:pushpin:`/`📌`/`📍`/`🚧`/`⏭️` + word forms) so a marker-decorated re-emit keys to the same identity as the operator's bare line and cannot dodge the dedup or the base-delete guard.
- snapshot a newly activated queue body as the closeout baseline instead of treating the changed head as an edit to in-flight work

### Post-commit queue consumption

- After a successful response closeout, required closeouts mark the first prompt complete in the queue in the same locked read/parse/write cycle. If a manual closeout resolves multiple contiguous queued `do #id` prompts with repeated `--done <id>` flags, it may consume that contiguous done-backed head batch in one closeout; it must stop before the first unresolved queued prompt.
- Explicit done IDs also mark matching queued prompts completed in place when they are not part of the contiguous active-head consumption range. This preserves the current head while striking opportunistically completed queued work, and the document/snapshot must be updated together before commit.
- Queue-head identity checks strip leading priority markers (`:pushpin:`, `:round_pushpin:`, markdown pin/prioritized aliases, and pin emoji) before comparing or classifying the head. A pinned `do [#id]` remains an id-backed directive that requires explicit completion proof; it is not a free-text head completed by any response body.
- Completed prompts remain visible as `- ~prompt text~` while later prompts remain queued.
- The consumed prompt range must be completable or drainable from both the live document and the snapshot in a provably identical way; otherwise strict closeouts fail before commit. Non-draining consumption strikes queue items by markdown-AST node key so intentional duplicate prompt text is not conflated.
- IPC payloads keep legacy component `patches` but annotate each patch with an explicit `op`; append-mode boundary patches also carry `node_id`. When before/after document states are available, the payload includes `node_patches` entries shaped as `{component, node_key, op}` plus operation-specific anchors/content for item-level insert, strike, unstrike, replace, remove, and move.
- Queue-consume editor IPC is stricter than generic component convergence: it must carry the queue strike as a `node_patches` entry keyed to the exact pre-consume queue node, carry raw and transient-marker-normalized baseline hashes, and omit a broad legacy `queue` component replace when the node patch is present. A delayed socket/file applier must reject the patch when the live editor buffer no longer matches either baseline hash; `(HEAD)` annotations, boundary comments, guard markers, and managed pipeline frontmatter are transient and do not by themselves make the generation stale. When no editor listener or live sidecar is active, the CLI falls back to the guarded detached disk write instead of replaying a stale component-wide queue replacement. When a listener or live sidecar is active but cannot apply/prove the node patch, the CLI fails closed and logs the blocked write rather than writing behind the editor.
- The watch daemon keeps a per-file markdown-AST node snapshot for watched session documents and logs `watch_node_events` JSON batches on file changes. Each event carries `{component, node_key, op, item_id, before_index, after_index}` plus optional content and neighbor anchors, so realtime queue/enqueue features can consume structural changes without reparsing text diffs.
- **Consumed queue prompts are echoed into their response block (`#queue-prompt-echo-in-response`).** An auto/synthetic queue head is never typed into `agent:exchange`, so a consumed queue turn would otherwise record only the `### Re:` answer with no trace of what it answered. When a closeout consumes one or more queue prompts, the binary embeds the full consumed prompt text as a labeled blockquote (`> **Queue prompt:**` …) immediately after this cycle's response heading, in **both** the document and the snapshot so the selective-commit boundary stays consistent. The response heading is located by the captured response body's first line, falling back to the last non-code `### Re:` heading in the exchange. Injection is skipped (fail-safe, content unchanged) when the prompt is empty, the exchange/heading cannot be located, the same echo is already embedded, or the prompt's first line already appears as an exchange line (a user typed it in directly, so no echo is needed). The already-present check also recognizes stale recovery echoes that are blockquoted, one-line-labeled, priority-pinned, or prompt-prefixed variants, so repeated queue-consume recovery does not duplicate a stale free-text prompt in `agent:exchange` while the next id-backed queue head remains open. Multi-line prompts and prompts containing fenced code are quoted verbatim. Because the echo lands before the trailing boundary marker, it is committed history and is never re-detected as an unresolved tail prompt.
- A consumed free-text queue head is not proven by a binary consume marker alone. If the head was present at preflight and is no longer queued, `session-check` requires committed exchange proof (usually the queue-prompt echo or response prose that plausibly answers the head) and fails closed with `#lr-queue-patchback-miss` when that history is missing.
- **Opt-in echo summarization for long prompts (`#queue-prompt-echo-summary`).** By default the consumed-prompt echo copies the prompt verbatim (the deferred-by-the-user behavior: "for now, let's try copying the entire prompt"). Setting `agent_doc_queue_prompt_echo_max_chars: <n>` in frontmatter opts into a bounded summary: when a consumed prompt exceeds `n` characters, the echo records the prompt's first line truncated to `n` characters (on a char boundary) plus the count of elided characters and a pointer noting the full prompt is retained in `agent:queue`, instead of the full verbatim text. The threshold is read from the document's own frontmatter; `None` (absent) keeps the verbatim copy. The dedup/idempotency check uses the same rendered echo so a summarized echo is still recognized as already-embedded on re-runs.
- Direct `agent-doc run` / bare-path invocations with `queue_active: true`, explicit `go` mode (`queue: go` or marker-side `go`), and no document diff synthesize the active queue head as the prompt diff. `auto` and `start` are legacy/manual *start* triggers only; continuation is driven by `queue_active: true` plus explicit `go`, so a plain persisted-active `agent:queue` without `go` stays inert (`#active-queue-persisted-no-continue`). Binary-owned route writes must not add `auto`, and should strip it from any touched queue tag. Successful closeout consumes one prompt, commits that item, then continues from a fresh run cycle while the next live queue head is still eligible. Continuation stops without consuming the next prompt when the queue drains, a stop fence or time gate reaches the next live head, the next head changed since the snapshot, `go` mode is absent, or a closeout/session-check/verification step fails; diagnostics name the completed item count, next prompt when known, and stop reason. An inactive plain queue (no explicit start/go trigger) never self-starts. **Unwrapped fenced task recovery (`#queue-unwrapped-fenced-task`):** when an otherwise queue-native-syntax-free body contains operator prose before a balanced Markdown code fence and an explicit request after the final fence, the parser treats that whole visible block as one multiline prompt. A fenced console/log dump without a trailing request remains inert `Freeform`, and any ordinary list item, preset, dispatch, control fence, or slash command disables the recovery so mixed queues cannot be swallowed.
- Codex harness-native `agent-doc <FILE>` turns rely on the installed Stop hook for the same queue liveness. After a clean closeout, if the document still has `queue_active: true`, explicit `go` mode, and a ready next head prompt, `codex-stop` blocks the final answer and instructs the agent to continue in the same pane by answering the next prompt and persisting with `agent-doc finalize <FILE>` or `agent-doc write --commit <FILE>`. The hook must not instruct the owner pane to invoke `agent-doc <FILE>` recursively. The hook records the requested head prompt and fails closed if a repeated Stop hook arrives without the head advancing, so continuation cannot spin forever on an unchanged queue item.
- **Binary-owned continuation final gate (`#codex-auto-queue-stalled-final-gate`).** The shared `queue_continuation::detect` is the single source of truth (`queue_active: true` + explicit `go` mode + active `resolve_activation` + a ready head that is not a stop fence / future time gate / snapshot-modified head). An accepted controller `admin queue pause` (`#qpausego`) does NOT short-circuit this detector: the pause suppresses only the *unattended* supervisor idle-queue watch auto-injection (the flood it fixes) and is surfaced as `preflight.queue_paused`, `preflight.queue_pause_reason`, and pause-aware `preflight.queue_continuation_guidance`, but the attended in-session `/loop` — and `session-check` / the codex-stop gate that consult `detect` — keep draining real go-mode queue work (stalling them on a pause strands genuine backlog). `queue: stop` frontmatter / `--- stop` fences are the in-session stop control; `admin queue resume` clears the pause flag/reason. `auto` and `start` are start triggers only and are not enough for continuation: a persisted-active queue (`queue_active: true`) without `queue: go` or marker-side `go` is not eligible (`#active-queue-persisted-no-continue`). Every successful binary closeout (`finalize`, strict `write --commit`, `repair`, the already-committed no-op path) reconciles a durable marker under the project state ledger: written when a continuation is owed, cleared when the queue drains / `go` is removed / `queue_active` is false / the head advances. The marker is document-scoped liveness state, not Codex-thread ownership proof. Ambient `codex-stop` handling requires tracked state for its exact current `session_id`; if that binding is absent, it is a no-op and must not attach a pure Codex thread to another thread's document. Tracked repeated stops still fail closed on a non-advancing head. `agent-doc session-check` surfaces this as the typed detail `queue_continuation_required=true` / `next_queue_prompt=…` (informational, exit 0 by default); the strict `agent-doc session-check <FILE> --codex-final-gate` exits nonzero when continuation is required, for Codex direct-exec closeout paths that must not emit a final answer past a stalled queue. Live verification remains gated on `#codex-auto-queue-live-verify`.
- If `session-check` interrupts a Codex Stop hook because the prior cycle is already `committed` but the document has fresh unresolved exchange prompt changes with no new cycle (including active-session post-commit prompt drift), the hook treats that as new in-pane work. Even when `stop_hook_active` is already true, it blocks with an instruction to answer the prompt and persist through `finalize` / `write --commit`; it must not emit the generic "already continued once" stop for a still-open cycle.
- A Codex prompt binding that observed a terminal predecessor is settled when a later response cycle starts after that binding and commits a captured response. The Stop hook must retire and park that exact-turn prompt debt even though `last_prompt_cycle.cycle_id` names the predecessor rather than the later committed cycle; otherwise every final answer repeats the stale "has not crossed the binary-owned write boundary" block after `session-check` already proves committed (`#codex-stop-later-cycle-settles-debt`).
- **Queue-head consumption requires an explicit completion signal for `do [#id]` heads (`#queue-strike-on-halt`), with an exact-id exception for synthetic/preset heads (`#queue-head-consume-on-topic-id-regression`).** A `### Re:` heading that merely names a `do [#id]` queue head is *not* a completion signal — a halt/refusal response names the head precisely to explain why it is *not* being done. The CLI `finalize` / `write --commit` path consumes a `do [#id]` head only when the cycle (a) carries a closeout flag — `--done <id>`, `--backlog-gate <id>`, or `--backlog-edit "<id>=…"` — that names the head id, or (b) the head is a genuine fresh operator prompt-target / `do queue` trigger in the cycle diff. For a **synthetic/preset head** — a natural-language prompt carrying a trailing `#preset` id rather than a bare `do [#id]` directive — it additionally consumes (c) when the response heading topic resolves to **exactly** the head id, for example `### Re: #spec-test-build-install-commit-push` completing a synthetic head whose preset id is `#spec-test-build-install-commit-push`. A heading topic that merely contains the id with trailing modifiers (`### Re: #id halt` / `### Re: #id deferred`) never counts, for either head shape. The Codex Stop-hook auto-close path (which has no closeout CLI flags) consumes from a heading on an **exact** topic match (`### Re: do [#id]`) or a topic that resolves to **exactly** the head id (`### Re: #id` for head `do #id`), never on a heading with trailing modifiers.
- **Explicit multi-head drain (`agent-doc queue consume <FILE> [--count N]`, `#multi-head-consume-one-per-finalize`).** The free-text strike heuristic consumes only ONE head per finalize — the head current at that cycle's preflight. When a single turn answers several free-text heads (for example the operator adds new queue items mid-turn and the response addresses all of them), the trailing answered heads stay queued and re-serve on the next auto-loop, producing duplicate-response churn. `queue consume` drains those answered stragglers deterministically: it strikes the leading `N` (default 1) free-text head(s) — the agent asserting they are already answered, the same explicit contract `--done <id>` gives an id-backed head. It is scoped to free-text heads: a leading id-backed (`do [#id]`) head bails with guidance to reap completed/gated work through `--done`/`--backlog-gate`, to explicitly acknowledge correction heads with `--ack-id`, or to leave live work queued. It writes document + snapshot like `queue sync`; the caller closes out through the normal commit boundary. There is deliberately no fuzzy head↔response auto-matcher — guessing which heads a response answered risks deleting a genuinely unanswered prompt, a worse failure than the churn. **Orphaned id-backed head escape hatch (`agent-doc queue consume <FILE> --id <id>`, `#orphanqhead`).** When a backlog item is reaped via `--done` but the matching id-backed queue head strike is lost to IPC drift, the head becomes an undrainable phantom: `--done <id>` is a no-op ("already resolved") and the normal `queue consume` rejects id-backed heads ("reap via --done"), so it sits forever and re-fires the auto-loop. `--id <id>` strikes that specific id-backed head (`do [#id]` / `[#id]` / `#id`) in document + snapshot. It refuses (non-zero, with guidance toward `--done`/`--backlog-gate`) when the id still names an OPEN backlog item — that is live work with a real drain path, never an orphan — so the escape hatch can never silently desync live work. **Open-backlog correction acknowledgement (`agent-doc queue consume <FILE> --ack-id <id>`, `#freshqueueauth`).** `--ack-id` is the explicit inverse proof: the id still names open backlog work, but the exact id-backed queue head is only a correction/acknowledgement to clear. It strikes the queue head in document + snapshot while preserving the open backlog item, and refuses prose heads that merely mention the id (answer those as free text). `--id` and `--ack-id` are mutually exclusive with `--count`.
- **Paused direct-steering acknowledgement (`agent-doc queue consume <FILE> --ack-text <TEXT>`, `#paused-free-text-ack`).** A response completed through direct harness-console steering may have no selected queue-head id because a stopped queue was never dispatched. The ordinary inactive-queue consume therefore remains fail-closed. `--ack-text` supplies an exact operator proof for only the leading free-text row: priority markers are ignored, all remaining text must match exactly, the row must be uniquely AST-addressable, and id-backed heads are refused. The write converges through editor authority, adds the normal queue-prompt provenance echo to the retained response, re-baselines the recovery snapshot, and preserves the paused frontmatter state; it never resumes or drains subsequent work.
- **Noise-head prune (`agent-doc queue prune-noise <FILE>`, `#goqstall2`).** Strikes every active **predicate-proven non-drainable noise** head (pasted console-output, status/log lines, or agent-response fragments — the same lines `session-check` surfaces as `queue_stale_noise_lines=<M>`) at **any** position in the queue, and also strikes orphan id-backed heads whose ids no longer name open backlog work. Unlike `queue consume`, which strikes only a contiguous LEADING free-text run and stops at the first id-backed head, prune clears artifact noise interleaved **behind** id-backed `do [#id]` heads — the shape `queue consume` and the answered-free-text strike could never reach. Id-backed directive heads naming open backlog work and genuinely drainable free-text/prose heads are preserved, so the prune never desyncs tracked or runnable work and never treats fresh operator prose prompts as stale/noise. Classification matches `queue_stale_noise_lines` exactly, including the `preset="..."` short-circuit; ordinary prose reports are drainable queue work even without an imperative verb, while fenced/log-only, bold-report, or agent-comment artifacts remain noise. It strikes the document and snapshot in sync through the editor-IPC-converged write path (`#fcc0`) — supervisor-safe, no hand-editing of a live queue — and is a no-op when the queue is inactive or nothing is predicate-proven. The caller closes out through the normal commit boundary.
- **Free-text queue heads complete on being answered (`#free-text-queue-head-consume` / `#free-text-queue-owner-consume`).** A head with no id directive — a plain question or instruction typed into `agent:queue` — has no `#id` to `--done`, so a non-empty captured response body for that cycle IS its completion signal and the closeout consumes it. The captured response owns that strike only while its exact closeout cycle is open; after `committed` or `abandoned`, the retained audit payload cannot strike an identical recurring command typed for a later turn. Same-cycle recurring imperatives still strike normally. A head is treated as **id-backed** (and therefore still needs an explicit completion signal, not just an answer) only when the *entire* head resolves to a single id directive — `#id`, `[#id]`, or `do [#id]` — or to a `#preset` / queue-trigger. A free-text head that merely *mentions* a `#id` in prose (for example `Approve [#shoptiers]. What are #next-steps?`) is **not** id-backed: it has no single id to complete, so it stays free text and would otherwise hang the auto-queue forever. Classification keys off whether the head resolves to an exact id directive, not whether it contains a `#id` substring. **Registered prompt-preset token heads are free text, not id-backed (`#qpresetstrike`).** A bare queue head that resolves to exactly a `#name` registered in frontmatter `prompt_presets` (and that does *not* also name a tracked `agent:backlog`/`agent:review`/`agent:pending` item) has no backlog row, so `--done <name>` fails ("id not found in backlog/icebox") and the leading `#` previously routed `queue consume` to `--done` and wedged the head as unstrikeable — keeping `queue_active=true` and re-serving every invocation. Such a preset-token head is a synthetic prompt completed by being answered, so it strikes like a free-text head via `queue consume` and the free-text finalize heuristic. A preset token that *also* names a tracked backlog/review id stays id-backed (the tracked-item reap path wins). A controller `queue_paused` state whose reason matches `"<preset> preset head is spent"` is revalidated on every dispatch: if the preset head is absent from the live queue, dispatch clears the stale pause and proceeds; if the live head is that registered preset token, dispatch first consumes it through the canonical queue consumer, clears the pause, and proceeds. The repair logs `spent_preset_pause_repaired ... action=resume_absent_head|consume_head`, so JB Run Agent Doc never repeats the old manual "clear '- #name'" instruction for a stale spent-preset receipt.
- **Free-text execution context is inline (`#ftimmediate`).** A free-text head may carry the same whitespace-delimited `[clean-session]`, `[focused-cycle]`, or `[operator-verify]` token as tracked work. The in-session loop parses that token directly from the queue text: `[focused-cycle]` yields to the supervisor, which force-clears and dispatches the same head in a fresh context; `[operator-verify]` remains deferred from both agent drains; `[clean-session]` stays loop-drainable while retaining its context-reset reason. This reuses the existing execution-context vocabulary without synthesizing backlog ids or durable side records.
- **Pre-materialized undrainable / noise heads self-defer, they do not churn (`#goqstall2`).** `#goqueuestall` defers `[operator-verify]` items at the *backlog→queue sync* boundary; `queue_continuation::detect` applies the same defer to heads *already materialized* in the `## Queue` block. **As of `#qcontdrain`, `[clean-session]` is no longer deferred in either path** — the in-session `/loop` drains clean-session heads in place (a possibly-stalled supervisor previously left them stranded), so only `[operator-verify]` is undrainable. The continuation head walk skips, in addition to a stop fence / future time gate / snapshot-modified head: (a) a `do [#id]` head whose backlog id is deferred (`deferred_backlog_ids`: `[operator-verify]` only; `[clean-session]` drains in-loop now), and (b) a **predicate-proven non-drainable noise** head — structural/log artifacts such as console status lines, fenced log-only blocks, bold response-fragment summaries, or agent comments. Plain operator prose is drainable queue work by default, even when it is phrased as a declarative bug report rather than an imperative. Noise heads are **never auto-deleted** (the live IPC supervisor races on direct queue edits); they are excluded from the continuation head set and counted as `queue_stale_noise_lines` for compatibility. They are cleared on demand through the binary-mediated `agent-doc queue prune-noise <FILE>` command (see below), which strikes only predicate-proven noise/orphan heads through the same editor-IPC-converged write path the closeout strikes use, so the operator never has to hand-edit a live queue. When only deferred/noise heads remain, `session-check` prints `queue_continuation_required=false queue_deferred_heads=<N> queue_stale_noise_lines=<M>` with guidance to drain deferred heads from a clean session and clear predicate-proven noise via `queue prune-noise` — the same self-defer end-state `#goqueuestall` produces for the sync path, so a queue pre-populated with undrainable `do [#id]` heads + pasted evidence stops churning no-op closeouts. **The supervisor idle-queue watch dispatch agrees with this decision (`#qchurn`).** The idle-watch derives its dispatch head from `queue_continuation::live_drainable_continuation_head`, which applies the same drainability + deferred filtering as `detect` (skipping artifact noise and `[operator-verify]` heads; `[clean-session]` is drainable per `#qcontdrain`), instead of the unfiltered `live_continuation_head`. Otherwise the watch would re-inject a no-op `/agent-doc` drain trigger every idle boundary for a queue that `session-check` already reports as needing no continuation — repeated zero-diff cycles with `user_intent_prompt_changes` empty.
- **Legacy stale-supervisor queue pauses recover once (`#jbrestale`).** New controllers tag stale-supervisor `queue_paused` churn bails with `supervisor_restart_redirect`, but route must also recognize the legacy markerless shape from an old route-owned supervisor: `failed_stage=queue_paused reason=#qchurn ... stale host supervisor pid<N> ...`. The classifier also treats `stale route-owned supervisor (pid N) ...` as stale-supervisor churn. If a newer same-document actor transition, a different live supervisor PID, or a named stale PID that is no longer alive and whose pause row predates the current system boot proves the pause predates the current owner, dispatch clears the stale pause before queue blocking. Otherwise both marker shapes use the same one-shot policy: restart the stale supervisor in the non-destructive supervisor path, lift the stale pause, and retry dispatch exactly once. Stale-binary freshness guidance must use idle recycle or normal restart and must not point operators at force/discard recovery unless the owner is genuinely wedged. Deliberate operator pauses and spent-preset pauses still fail closed.
 - If the queue drains, the queue body is cleared, `auto` is removed, and `queue_active` is cleared. When the queue is inactive and all entries are drained residue (completed, preset, or dispatch with no live prompts), the body is cleared regardless of whether the snapshot proves the queue was previously active — completed items are never re-executable, so retaining them is always noise.
- **Already-resolved heads drain, they do not halt (`#drained-done-queue-clear`).** The preflight strike pass converts every leading queue head whose `#id` is already in `agent:done` (or review-gated) into a `Completed` entry. A newly admitted prompt or any currently-open backlog item overrides historical `agent:done` membership (`#qrepeatid`): repeating the same prompt and stable id is executable new work, not completed residue, and the live backlog checkbox preserves that incarnation across maintenance retries until closeout reaps it. When no such reactivation exists and the strike empties the entire live-prompt set — a fully-resolved active queue batch, optionally fronted by a `dispatch #preset` directive — maintenance must re-resolve activation to inactive so the drain-cleanup path clears `queue_active`, strips any legacy `auto`, and empties the body in the same binary-owned pass. It must not classify the now-headless queue as an `item_modified` halt. The cross-cycle head-modified comparison applies the same done/gated strike to the snapshot entries before comparing, so a head resolved this cycle via `--done` does not read as an operator edit while a genuine still-live head edit still halts. A standalone no-diff preflight that drains in this way self-heals its snapshot/HEAD drift through the route queue commit-boundary recovery (which also accepts a drained snapshot when HEAD proves the prior active queue and only queue state differs), instead of stranding the maintenance mutation for a manual `agent-doc commit`.
- The `inactive_queue_residue` warning is a per-*edit* signal, not a per-preflight nag. Preflight emits it only when the inactive queue body actually changed since the snapshot this cycle (the operator added/changed content in an inactive queue, so a `do [#id]` they expect to run silently will not). A stable inactive queue that is unchanged from the committed snapshot — for example the retained live tail an `item_modified` halt leaves behind — is not re-flagged on subsequent edit-free preflights. This stops the recurring residue warning that drove the `#adoc-queue-ipc-drift` loop (root cause #1) while preserving the warning on genuine inactive-queue edits.
- **Live editor-buffer convergence (`#adoc-queue-ipc-buffer-divergence`, root cause #2).** When queue maintenance halts or drains an auto-queue, it writes the corrected queue body, opening-tag `auto` attribute, and `queue:` frontmatter to disk and snapshot. A live route-owned IPC listener keeps its own working buffer; a content-only IPC patch can replace a component body but cannot change the `<!-- agent:queue auto -->` opening-tag attribute or the frontmatter, while a tag/frontmatter-only convergence leaves stale queue body lines in the live buffer. Without converging all three, the live buffer can re-assert stale queue lines, `auto`, or `queue: start` on its next flush and regenerate snapshot/HEAD drift every preflight. After each halt/drain disk write, when a listener is active, preflight pushes a dedicated queue-convergence IPC patch (`ipc_socket::send_queue_convergence`) carrying the corrected `queue` component body, desired `queue_auto` opening-tag state, and canonical `queue:` frontmatter. The plugin applies the queue body as a component replacement, applies the frontmatter merge, and converges the queue opening tag through the `agent_doc_converge_queue_auto` FFI seam (a C int for a stable JNA ABI; an absent `queue` component or already-matching tag is a no-op). The push is best-effort — a missing listener or send error is logged, never fatal — because the disk/snapshot write remains the source of truth. A follow-up maintenance pass on the converged document mutates nothing and sends no further convergence (idempotent).
- **Route-owned activation retains ownership through editor delivery.** If queue activation, route session metadata, or route document preparation is accepted into the lazy CRDT delivery projection but is not yet editor-visible, route waits for the exact target projection before allowing the caller to checkpoint or dispatch. A newer converged editor value returns to the existing CRDT re-merge loop. A bounded non-convergence tells the operator to rerun `agent-doc route <FILE>`; it must not prescribe `agent-doc commit`, because no response cycle or owner-pane capture exists yet. When a tracked `/clear` causes a fresh harness restart and that newer actor generation has already entered `busy` through the supervisor's `auto_trigger_inject`, route treats the active turn as accepted owned dispatch and coalesces it rather than reporting that the fresh pane never became ready. Likewise, capability-proof reuse is an idle-pane admission gate: if the authoritative same-document actor is already `busy` in the registered owner pane, or that same pane has harness-specific live active-turn proof while its persisted actor projection still says `ready`, a missing proof coalesces the active dispatch rather than attempting a restart that its open cycle must reject; an explicit failed proof still fails closed.
