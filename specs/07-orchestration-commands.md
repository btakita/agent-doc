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
  - `operation_tree`, a compact binary-owned tree whose
    `policy=selected_path_only` records the blocker gate and the ordered phases
    on the chosen path without serializing speculative alternatives
  - `graph_evidence` when a materialized `.tsift/graph.db` exists and the prompt targets include queued `do #id` / `do [#id]` work
  - `handoff`
  - `blockers`
- The implementation reuses the same prompt/diff classifiers that power `preflight`.
- `pending_mutations.resolve_existing` signals that a matching open backlog/icebox item will need `--done <id>` if completed this cycle.
- For explicit `do #id` / `do [#id]` directives that resolve an existing tracked-work item, the planned finalize command includes the matching `--done <id>` argument so the harness closeout path does not rely on the agent remembering that flag manually.
- `pending_mutations.expect_add` signals that the response likely needs new backlog capture. It uses the same backlog-request signals as the preflight capture gate, matched as whole prose words so a `tasks/...` path, a `todo_list.rs` file name, a `#todo-id` or a code span never sets it (SPEC.md).
- `execution_scope=plan_backlog_only` suppresses repo implementation work for report/planning contracts such as `#agent-doc-bug`.
- Lower-agent routing fields are deterministic structural hints, not authority
  to skip parent review. `dispatch_candidate=true` means the prompt is shaped
  well enough for job packets; `agent_doc_dispatch: off` disables that
  candidate flag, while `agent_doc_dispatch: auto` records an opt-in for future
  automatic dispatch. `task_class`, `risk`, model tier, token budgets,
  `write_scope`, and `required_proof` come from prompt targets, backlog text,
  requested presets, and visible repo-work directives.
- `operation_tree` is the normative operation order for harness clients. Its
  blocker gate selects exactly one child (`path.execute` or `path.stop`). An
  executable path selects prompt scope before repo work, pending mutations,
  and required commands; a non-`none` handoff prunes the manual repo-action
  branch because the emitted handoff command owns that work.
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
- `--from-queue` extracts all active `agent:run` prompt entries and queue-level `preset` / `dispatch` directives, rather than only the single queue head used by normal `agent-doc run` resumability.
- **`#presetsalias`: frontmatter `presets:` is an alias for `prompt_presets:`.** Both spellings parse to the same typed map, so every consumer (preflight resolution and `prompt_preset_expansions`, preset-shape warnings, the identity registry, queue preset expansion) honors either. Binary frontmatter writes keep the operator's spelling: `write_preserving` leaves an unchanged `presets:` block byte-for-byte and re-renders a changed one in place under `presets`, never renaming it to `prompt_presets:` or appending a second key. When a document spells BOTH keys, it is not a parse error: the maps merge, `prompt_presets` entries come first and win a name collision, and `presets`-only names are appended; a `presets:` value that is not a map adds nothing. An unrelated write keeps both blocks verbatim; a write that changes the presets collapses them into the single canonical `prompt_presets` key.
- **Preset-associated runbooks are lazy, validated procedure context.** A structured preset may carry `prompt: <string>` plus `runbook: <project-relative-path>`. Scalar presets remain fully compatible. Runbooks must be Markdown below project `runbooks/` or `.agent-doc/runbooks/`; absolute paths, parent traversal, missing targets, paths outside the catalog roots, and symlink escapes fail closed. Preflight does not inject runbook content on every turn: only an invoked preset gets `runbook` and an imperative `runbook_load_instruction` in its `prompt_preset_expansions` entry. `agent-doc runbook list <FILE> [--json]` exposes the deterministic catalog and association/expansion commands; `show <FILE> <#preset|catalog-name|path> [--json]` loads one validated entry; `create <FILE> <kebab-name> [--preset <#id>] [--description <text>]` creates without overwrite and promotes an existing scalar preset through `write_preserving` without changing its body or unrelated frontmatter.
- Batch-level `preset` / `presets` directives request frontmatter `prompt_presets` (or its `presets` alias) and are validated before execution. Exact preset keys win; a bare directive such as `preset review` also resolves to frontmatter key `#review` when only the hashtag form is defined.

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
- Each fan-out pane runs its `claude -p` task as the pane's own process (never typed into the pane's fresh shell) and ends with it; the frontmatter `env` prefix is expanded by the pane's `/bin/sh`, so `$(passage ...)` values never reach the tmux argv, the process list, or logs. See [Session & tmux commands](07-session-tmux-commands.md) (start pane launch).
- If a materialized tsift graph database exists and resolved tasks include `do #id` / `do [#id]` items, orchestration attempts to collect the same graph evidence, conflict matrix, and dispatch trace as `plan`. Sequential and DAG child prompts receive a bounded `<tsift_graph_evidence>` JSON block outside the document mutation only when graph evidence is available. Parallel worktree job packets receive the same block in the task prompt when available, including a normalized `lower_agent_job_packet` derived from the worker prompt packet's owned files, read-only context, forbidden files, expected tests, expansion commands, token budget, fail-closed prompt text, dispatch-trace graph links, worker feedback, projection hashes, and replay/repair commands. If graph collection fails, orchestration warns and continues without the graph block; if collection succeeds, parallel mode still fails closed when the conflict matrix reports `can_parallel=false` or `fail_closed=true`.

## `--mode dag`

- Supports `id=` and `after=` / `deps=` metadata prefixes on task lines.
- Fails fast on unknown deps, duplicate ids, and dependency cycles.
- Executes in deterministic topological source order.
- Every ready node still runs through the normal single-document lifecycle: inject prompt -> `preflight` -> fresh agent request -> strict `finalize`.
- DAG mode is dependency-aware but not concurrent against one session document. Real concurrency belongs to `--mode parallel`.

### Auto-DAG from queue

`agent-doc orchestrate <FILE> --mode dag --from-queue`

- Builds a binary-owned `agent-doc-auto-dag-schedule-v1` schedule from the active `agent:run` body.
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

## `agent:run`

The `agent:run` component batches prompts inside the document.

Repeating batches use the canonical `agent:loop` component. It has the same
body and queue attributes and implies an unbounded loop. `loop=N` keeps the
finite-pass form. Legacy `agent:queue loop` and `agent:run loop` markers remain
accepted and are not rewritten solely to migrate their spelling.

Cross-document route dispatch appends transient prompts only to a non-repeating
`agent:run` (or legacy `agent:queue` without `loop`). It never appends them to
`agent:loop` or another queue carrying `loop`; if no immediate run exists, the
router creates `agent:run` and leaves every repeating queue byte-identical.

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
`auto` attribute on the `agent:run` marker:

| `queue:` value | meaning | subsumes |
|----------------|---------|----------|
| `start` (alias `go`) | activate — drive the queue | `queue_active: true` + marker `auto` |
| `pause` | the one hold — inactive until the operator deletes it or writes `go` (marker spelling: `<!-- agent:run pause -->`) | — |
| (absent) | default `go` — a queue with no control on the marker or in `queue:` runs (`#queuegodefault`) | absence of both, `queue_active: false` |

**There is no `stop` (`#queuestopremove`).** A queue is either running (no
control, `go`, or `start`) or held (`pause`). A drain or halt clears the control
(`frontmatter::merge_queue_state(.., false)` == `clear_queue_control`), so a
head added later simply runs; the binary writes `pause` only for a hold it must
keep across cycles — an operator stop fence it consumed, a wedged owner pane
(`#recguard-wedge`), an operator command preempting an active drain — through
`frontmatter::merge_queue_hold`. Legacy spellings are migrated, never honored as
a hold: a frontmatter `queue: stop` (and a lone `queue_active: false`) reads as
no control and is dropped on the next write, while a legacy `stop` MARKER token
is the operator's hold gesture and reads as `pause`. `merge_queue_control`
refuses `stop`. Before this, the binary wrote `queue: stop` on every halt, and a
head left behind (or mirrored in later) sat inert under a control the operator
never wrote.

**`go` is the default (`#queuegodefault`).** With no marker token and no
`queue:` field, the queue resolves to `go` (`control_binding::resolved_queue_binding`;
route's inactive-head probe shares the rule through `queue_control_defaults_to_go`,
and an activation of a control-less queue persists `queue: go`, never `start`,
via `merge_queue_state`). Free-text admission keeps its unmanaged-queue scope for
a control-less queue, so a free-text head already present is still admitted as
tracked work before the drain takes it. To hold a queue the operator writes `pause`
on either surface; a marker `pause` converges into `queue:` like the other
marker controls. A present but unrecognized `queue:` value (a
half-typed edit) is not "absent" and stays inert, and a legacy
`queue_active` flag still speaks for itself.

**Queue edits imply `go` (`#queueeditgo`).** Editing the queue is consent to
run it. When the pre-turn baseline and the current document differ by at least
one live queue prompt or preset the baseline did not carry (added or reworded;
removals, strikes, and progress/pin re-marks alone do not count), and the
operator did not also change the queue control in that window, preflight arms
`go` on both the marker and `queue:` (`control_binding::infer_queue_go_from_prompt_edit`,
ops-log `queue_edit_go_inferred`). Before this, adding an item after a drain
and invoking Run Agent Doc left the item inert, or armed only `start`, which
never continues past the first head. `queue: pause` is the standing hold.
It blocks the inference and free-text admission's queue start, and a
drain/halt (`merge_queue_state(false)`) or a consumed marker token never
lifts it; only the operator's own edit to `queue:` does. A control gesture made
beside the prompt edit (for example setting `pause` while adding an item) wins
over the inference.

- The value is parsed leniently (case- and whitespace-insensitive). An
  unrecognized value (typo) is ignored and never flips activation.
- `frontmatter::normalize_queue_control` folds a recognized `queue:` value onto
  `queue_active` at parse time, so every existing reader (preflight activation,
  Claude `/loop` continuation, Codex Stop-hook, OpenCode auto-loop) honors it
  without a separate read path. The canonical key wins over a stale
  `queue_active:` line in the same frontmatter.
- `queue_active:` and the `auto` marker attribute remain accepted as deprecated
  input for backward compatibility; new documents should use `queue: go` /
  `queue: pause`, or no control at all.

#### Marker-side control (`start` / `go` / `pause` on `agent:run`)

`start`/`go`/`pause` are also accepted as **marker** control tokens on the
`<!-- agent:run ... -->` opening tag — the ephemeral gesture spelling of the
frontmatter control:

- `<!-- agent:run go -->` / `<!-- agent:run start -->` fresh-activates the
  queue, identical to the legacy `auto` attribute (routed through the Auto
  trigger). `go` is an alias for `start`.
- `<!-- agent:run pause -->` (legacy spelling `stop`) holds the queue and
  converges into `queue: pause`.
- The control token is stripped from the opening tag once the queue drains or a
  hold is persisted, so it never re-triggers on the next cycle. Settled drain
  maintenance treats marker `go`/`start` as the same immediate-activation state
  as legacy `auto`: it removes the marker token and clears the frontmatter
  control in one persisted write. A second maintenance pass over that drained
  queue is byte-identical, leaving no control-binding normalization pending.

These marker tokens are recognized queue-only attributes (no
`misplaced_component_attr` typo warning).
They are canonicalized as bare tokens, never `key=true`: preflight repairs
boolean-serialized forms such as `priority=true` / `go=true` and the malformed
`preset="#name"=true` suffix back to `priority` / `go` and `preset="#name"` in
both the visible document and snapshot before continuing queue maintenance.
Queue-tag rewriters never rewrite an explicit empty preset: `preset=""` means
"no preset" (GH #227), is preserved byte for byte in the document and snapshot,
and resolves to no preset everywhere a queue preset is read.

Queue maintenance binds the marker gesture and `queue:` frontmatter
bidirectionally (`#qactsync`). It compares the current editor-authoritative
document with the stable pre-turn baseline: when exactly one representation
changed, that operator edit wins and is projected to the other representation.
An explicit marker token also wins when no baseline is available, so a fresh
`go`/`start` gesture can activate a document whose durable frontmatter still
reads `queue: pause`. If both representations changed incompatibly in one
window, canonical frontmatter wins deterministically and the conflict is
logged. A second convergence pass over the synchronized document is a no-op.

The route/dispatch path honors marker-side control identically to preflight: an
inactive document (no `queue: start` / `queue_active: true`) whose `agent:run`
opening tag carries `go`/`start` is recognized as an activatable head, so
`agent-doc route` (and the JetBrains `Run Agent Doc` action it backs) starts the
queue even when the frontmatter still reads `queue: pause`. A marker-side `pause`
keeps the queue inert on that path and wins over `auto`/`go`/`start`.

Activation readers that run without a preflight convergence pass first share
preflight's control predicate (`#qbindingone`, GH #79):
`control_binding::queue_control_activation`. These are the supervisor/loop
drainability count (`drainable_head_count`) and the continuation detector
(`required_continuation`). Before this, they re-derived activation from raw
frontmatter. A marker `go` beside a stale frontmatter hold was then active for
preflight but reported `drainable_head_count: 0`, so the idle watch never
dispatched and the queue never drained. Closeout-time readers (consumption,
noise pruning) see frontmatter that preflight's `#qactsync` convergence has
already synchronized. A lone legacy `queue_active:` flag with no marker control
stays inert, matching preflight.

#### Writer emits canonical `queue:` (phase 4)

Queue-maintenance write paths persist the canonical control directly:
`frontmatter::merge_queue_state` writes `queue: go` (or `start`) on activation
and clears the control on a drain or halt (`#queuestopretire`,
`#queuestopremove`), removing any deprecated `queue_active:` line in the same
write: the queue falls back to its default `go` (`#queuegodefault`), is idle
while empty, and a head added later runs. A hold the binary must keep is the
explicit `queue: pause` from `frontmatter::merge_queue_hold`. That default `go` drains unattended
too (`#queuegodefaultdrain`): the supervisor idle watch's drainability count and
the continuation detector the Codex Stop hook reads treat a control-less queue
exactly like an explicit `go`. An operator `pause` survives a
drain. A drain that held fresh backlog ids out of the queue also clears the
control: the backlog opted into mirroring (`queue=append|sync`), so the next
cycle mirrors and runs them. Both fields are normalized away together by the replay-hash /
boundary-compare paths (`strip_queue_active_frontmatter`,
`strip_route_queue_state_for_boundary_compare`), so a legacy `queue_active:` and
a migrated `queue: start` compare equal and do not regenerate the
snapshot/HEAD drift loop. Reads continue to resolve through
`normalize_queue_control` (`queue:` → internal `queue_active`), so the deprecated
field remains accepted as input for backward compatibility.

- **Remaining (`#queue-state-unify` follow-ups):** harness-parity doc rewrites
  (Claude `/loop`, Codex Stop-hook, OpenCode auto-loop instruction surfaces) are
  tracked separately.

### Backlog→queue sync (`queue` attribute, `#backlog-queue-sync-attr`, `#queue-enqueue-action`)

An `agent:backlog` component may carry a `queue` attribute so the binary keeps
`agent:run` populated from the active backlog instead of requiring a manual
`--backlog-reorder` + hand-regenerated queue each cycle. `agent:icebox` is a
parking lot, not an automatic scheduling source: a drained queue and drained
backlog remain terminal until the operator moves work to backlog, adds a manual
queue item, or marks a specific parked item with a per-item enqueue token.

- `<!-- agent:backlog queue -->` (bare token) — **append** (the default).
- `<!-- agent:backlog queue=append -->` — add `do [#id]` for active backlog ids
  not already referenced in `agent:run`; existing entries and order are
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
  `**enqueue**` is appended to `agent:run` as `do [#id]` even when the component
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
- **Operator-delete tombstones read row ids through the projection
  (`#claimrowtombstone`).** The mirror's operator-delete tombstone ledger
  records an id that was an active head in the committed snapshot (or the last
  observed frontier) and is now absent from the live queue, and lifecycle
  convergence gives a tombstoned `do [#id]` a lawful multiplicity of zero
  (`#qdelauth`). Both sides must read a row's id the same way: lifecycle
  markers and the binary-owned `[#owner#generation]` attempt annotation are
  projection metadata, never the row's id
  (`queue_response::queue_prompt_done_id` strips them before reading the first
  `#`). Observed 2026-10-26 on `tasks/agent-doc/agent-doc-bugs.md`: the snapshot
  held `📥 do [#claimrowmigrate]`, the claim projected
  `☑️ [#subagent:claimrowmigrate#1] do [#claimrowmigrate]`, reconciliation read
  that row's id as `subagent`, tombstoned `claimrowmigrate` as deleted, and the
  13:09:26Z queue-maintenance write removed the reported row with no closeout
  (the claim record stayed in the ledger, unprojected). Regressions:
  `queue_response::tests::queue_prompt_done_id_ignores_lifecycle_and_owner_projection`
  and `tests::run_queue_maintenance_keeps_a_claimed_mirror_row_whose_snapshot_was_bare`
  (`agent-doc-preflight-io`).
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
  `agent:run priority`, so append-built queues are reordered immediately even
  when the queue marker itself has no `priority` token. Automatically promoted
  queue prompts are annotated with `:round_pushpin:`; manual priority dispatches
  use `:pushpin:`. A bare `priority` attribute on the `agent:run` marker still
  stable-sorts `do [#id]` prompts by their source item's priority after the sync
  (`queue::sort_prompts_by_priority`), covering manually edited queues.
  `priority` is recognized on backlog/icebox/review/queue by both the lint and
  preflight (`agent:review priority` sorts gated review items in place; review
  items never feed queue ranks). See `specs/pending-system.md` for the per-item token grammar.
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
- **Free-text materialization is an exactly-once projection by source provenance (`#queue-materialization-provenance`).** When queue maintenance replaces an id-less source row with `do [#id]` plus a backlog row while the editor concurrently revises that source, the CAS repair derives identity from the complete three-plane transition: baseline source slot → projected id/backlog receipt → observed editor cut. Removing the exact projected ids from the observed queue must restore baseline cardinality and order; every non-source slot must remain byte-identical, and each proven source slot must contain one actionable revision. Only then may the revision update the original backlog id and its id-less replay row disappear. Prefix/suffix similarity, adjacency by itself, and global normalized-text matching are not identity evidence. Ambiguous equal-text sources, extra authored rows, missing projected ids, or unrelated edits fail closed. The join is idempotent: after the first repair the durable id remains and the source replay row is absent, so retries/CAS re-observation cannot materialize a second backlog id or queue head.
- snapshot a newly activated queue body as the closeout baseline instead of treating the changed head as an edit to in-flight work
- **Retained queue authority fails closed.** `EditorAttachedMissingReplica`,
  `EditorSyncPending`, and exhausted current-authority reads are admission errors,
  never an empty/default queue state. When a live registered authority still equals
  the merge baseline but disk differs, disk is evidence of a newer native editor
  save—not substitute authority. Preflight requests typed editor-replica
  re-registration and re-observes the editor-owned cut for the same bounded retry
  budget used by other transient authority states. It proceeds when that cut
  converges, preserves disk byte-for-byte throughout, and refuses to start a
  response if the retry budget expires.

#### Duplicate queue-effect identity (`#queueeffectidentity`)

Invariant: every lawful repeated live free-text occurrence is a separately
addressable effect. Two rows may have the same operator-authored work text, but
they MUST NOT inherit one shared claim, lifecycle marker, report receipt, or
closeout merely because that text normalizes equally. Conversely, an annotation
MUST NOT increase the snapshot-authored multiplicity and thereby turn a replay
artifact into new work.

The pure policy owner is
`document_queue::annotate_duplicate_queue_effects`, invoked by the existing
`converge_queue_via_lifecycle` preflight projection. Its transition table is:

| Observed live single-line free-text family | Successor |
| --- | --- |
| One bare row | Unchanged |
| Two or more lawful bare rows | Append `⟦effect:1⟧`, `⟦effect:2⟧`, ... in queue order |
| Existing unique positive effect ids plus bare rows | Preserve existing ids; assign each bare row the lowest unused positive id in queue order |
| A repeated or zero effect id | Preserve the first valid occurrence; deterministically reassign later/missing occurrences |
| One annotated survivor | Preserve its id; never compact or renumber |
| Id-backed or multiline row | Unchanged |

Lifecycle convergence strips only the effect suffix when computing the
free-text multiplicity family, then annotates the lawful survivors. Claim and
receipt identity retain the suffix. Therefore convergence remains replay-safe,
while `release + publish ⟦effect:1⟧` and `release + publish ⟦effect:2⟧` are
exactly different targets. A bare target against several annotated descendants
is an ambiguity error listing the exact rows; claim resolution never chooses
the first annotated effect.

**A claim is bound to the row it was issued for, never to a text-identical
sibling (`#claimrowmigrate`).** Annotation is an edit of the claimed row, so
the claim follows that row's annotated successor, exactly like any other
in-place edit (`#claimfollowedit`). A claim is *located* once its anchor pairs
with at least one live line, or once exactly one candidate row (an anchored
descendant, a same-identity line, or an effect-annotated copy of the same
authored text) carries this exact attempt's visible `[#owner#generation]`
projection. A located claim holds only those lines: a text-identical row added,
restored, or pasted later never joins its anchor slots, and when the pair is
annotated `⟦effect:1⟧`/`⟦effect:2⟧` the claim moves to its own row's id
regardless of which copy comes first. The visible owner projection is the
tie-break that text alignment cannot provide: when a restore and the
annotation land in one queue cut directly beside the claimed row, both
annotated rows contain the claimed text, and only the binary-written
`[#owner#generation]` on the original row identifies it. That is why
`QueueClaimLedger::follow_edits` receives queue rows with their lifecycle
projection retained (`queue_continuation::live_queue_head_rows`); it strips the
projection for work text and identity and reads the owner annotation solely as
row evidence. An annotation naming a different owner or generation is not
evidence, and a row another live claim holds is never taken. Only an
*unlocated* legacy claim (no anchor yet, or no anchored line survives) still
anchors every byte-identical copy; when those copies are annotated it migrates
onto the copy carrying its projection, else to the first descendant in queue
order. Observed 2026-10-26 on `tasks/agent-doc/agent-doc-bugs.md`: a 52h-old
reported `subagent:release_publish_patch#3` handback moved onto a freshly
restored identical `release + publish` row (`☑️ … ⟦effect:1⟧`) and its own row
became `📥 … ⟦effect:2⟧`. Regressions:
`queue_claim::tests::a_located_claim_never_widens_onto_a_restored_identical_sibling`,
`an_effect_annotation_follows_the_row_carrying_the_claims_own_projection`,
`a_legacy_multi_slot_claim_migrates_onto_its_projected_row_only`, and
`queue_cmd::tests::reported_claim_keeps_its_row_through_a_duplicate_effect_annotation`.

**Every queue that displays a claimed identity shows its attempt
(`#queuemirrormarkers`).** A claim, report receipt, and completion cell are
keyed by work identity, never by queue id, and a claimed identity is already
held out of every queue's drain. Its visible lifecycle marker and
`[#owner#generation]` annotation are a projection of that same state, so the
projection covers every queue component, not only the scheduler-selected one:
the claim command path (`queue_cmd::derive_claim_runtime_projection`, run after
every claim, start, report, and release, and by completion feedback) and the
relay's durable-attempt overlay project all queues; preflight maintenance
projects its selected queue with pass-local evidence and then repaints the
sibling queues' live rows from the same ledgers
(`ClaimProjectionScope::SiblingsOf`, carrying the pass's selected heads so a
sibling copy of a selected head shows the same `🚧`). The traversal is
`queue_mirror_projection::project_runtime_projections_across_queues`. A
sibling row with no claim, receipt, or completion cell for its identity
projects `📥` (`#queueworkevidence`), so a sibling copy can never keep an
expired generation, a released report annotation, or a `🚧` left behind by an
earlier pass that selected its queue. Struck rows are reopened on durable live
evidence only in the scheduler-selected queue: a sibling's struck row is that
queue's own terminal or `agent:loop` pass-owned evidence, so another queue's
claim never resurrects it, and no strike is propagated into a sibling (loop
rows close only through their own pass, GH #361/#364). Claim edit-following
(`follow_edits`) stays anchored to the selected queue. Observed 2026-10-26 on
`tasks/agent-doc/agent-doc-bugs.md`: preflight selects the first queue that is
not entirely claimed, so passes that selected `issues` painted
`🚧 [#subagent:layout_liveverify#1]`, `☑️ [#subagent:release_publish_patch#3]`,
and a selection `🚧` on its rows; once `main` was selected again those rows
froze while `main` moved to generation 2 and closed claim #3. Regressions:
`queue_mirror_projection::tests::every_queue_displaying_an_identity_receives_its_authoritative_attempt`,
`queue_cmd::tests::claim_projection_reaches_every_queue_displaying_the_identity`,
`queue_cmd::tests::claim_release_clears_every_queue_and_never_reopens_a_sibling_struck_row`,
`queue_cmd::tests::sibling_scope_leaves_the_projected_queue_untouched`, and
`tests::run_queue_maintenance_projects_claim_state_onto_sibling_queue_rows`
(`agent-doc-preflight-io`).

Evidence inputs are one immutable current queue cut plus the committed snapshot
used by lifecycle convergence. No timestamp, claim-ledger ordering, release
version guess, or later response text may mint the id. The projection is a
one-shot pure transform with no long-lived Source/Computed/Effect lifetime; the
existing preflight document effect persists its successor. Audit evidence is the
visible suffix together with the existing queue claim/operation logs. Allowed
mutation surfaces are only live queue-row text and the claim ledger's normal
line-follow migration; operator prose, row order, non-prompt entries, tracked ids,
and struck history are out of scope.

Verification MUST cover deterministic assignment, idempotence, stable survivors,
repeated-id repair, replay multiplicity, ambiguous bare-family lookup, distinct
claim identities, legacy claim migration, and marker projection on only the exact
claimed occurrence.

### Resolved directives are task authorization (`#directiveauthorization`)

An imperative operator edit such as `go ...`, `push ...`, `commit + push`, or a
resolved prompt preset whose effective body authorizes the named action (for
example `#auth`) is current operator authorization for that task. Claude Code,
Codex, and dispatched workers MUST execute the authorized action without asking
the operator to say `go` / `push`, repeat the directive in chat, reconfirm, or
invoke the preset again. For a dispatched worker, the verbatim queue item,
tracked-item text, and every resolved effective preset body in
`subagent_prompt_preamble` jointly define that scoped authorization.

Task authorization and harness-native tool permission are distinct. A resolved
directive does not forge or bypass a native permission grant; if the harness
still protects a particular tool action, the exact tool permission MUST be
granted through the harness permission system. Conversely, the existence of a
possible native prompt MUST NOT make the agent reject or re-confirm task
authorization before it attempts the authorized action.

### Subagent operator-request handback (`[#subagent-operator-request-<unique-id>]`)

Every `agent-doc queue brief` preamble MUST tell a dispatched worker how to
surface a genuinely missing scope/choice or a harness-native permission request
through its coordinator. A worker MUST NOT guess or silently deny the request,
but MUST first honor any task authorization already resolved above. Before
stopping, it sends or returns this packet without paraphrase:

```text
[#subagent-operator-request-<unique-id>] subagent operator request
- *queue_item:* <origin-queue-item>
- *kind:* question | permission
- *blocking:* true
- *request:* <exact question or exact tool/action requiring permission>
- *context:* <why it is needed, current state, target/scope, and risk or effect>
- *choices:*
  - **<label>**: <effect>
```

The worker MUST replace `<unique-id>` with a short request-specific lowercase
suffix containing letters, digits, and hyphens, and MUST use a different marker
id for every request packet in the session document. The fixed legacy
`[#subagent-operator-request]` marker remains readable for retained and
historical packets, but workers MUST NOT emit or reuse it. A closeout that
presents two blocking packets with the same marker id fails before response
capture and names the duplicate id.

The packet MUST render its fields as list items and emphasize each key with its
colon (`- *key:* value`) so the ask is easy to scan while retaining one stable
key/value field per line. Choices are nested below `- *choices:*`. Historical
plain `key: value` fields and the common colon-outside-emphasis or bold-key
spellings remain read-compatible, but workers emit the canonical list form.
The packet MUST include every offered choice with its effect, and MUST render
each choice title in Markdown bold while leaving the effect as ordinary prose.
`- *choices:* []` is valid only when the harness supplied no choices. The
coordinator MUST copy the
complete packet verbatim into `agent:exchange`, present the same request from
the main harness session, and send the operator's answer verbatim to the same
worker when the harness supports continuation. The claim remains live while
the request is pending; a packet is not worker completion. The closeout that
presents a blocking packet MUST add one brief, concrete operator action to
`agent:todo` with `--todo-add`. When the operator's answer and the continued
worker result complete that action, the completion closeout MUST check it off
with `--todo-done`; the handback path must neither omit the TODO nor leave a
completed request open (`#wayposttodo`). The binary enforces the first half:
a closeout whose response presents a blocking packet without its own
`--todo-add` gets a derived, id-layered `agent:todo` item from the packet's
`request:` and choice labels in the same transaction (`#packettodo`,
[§2](02-document-format.md)).

For current packets, `queue_item` is copied unchanged from the dispatch brief.
The derived todo owns the packet id. While it remains open, the same live queue
row carries `🙋` beside its lifecycle state (`🙋🚧` while running) and appends
`[#request-id]` beside the secondary request rather than before the primary
work. These projections are stripped from queue identity and make the row
non-drainable. `--todo-done <request-id>` clears `🙋`; equivalently, an operator
may append a local answer as `[#request-id]: <answer>`, which the next closeout
folds into that same todo-done transition while retaining the local evidence.

Permission safety is stricter than question routing: a conversational answer is
context, not a tool permission grant. The worker may retry a protected action
only after the harness permission system records a grant for that exact action.
The coordinator MUST NOT broaden the requested target, duration, command
prefix, filesystem scope, or external side effect.

The transport contract distinguishes documented native behavior from the
portable agent-doc fallback:

| Harness | Native behavior | agent-doc fallback |
| --- | --- | --- |
| Claude Code | Foreground and background subagent tool-permission prompts surface in the main session. `AskUserQuestion` is removed from subagents; background workers can retain `SendMessage` and `SubagentHandback`. | Send the packet through `SendMessage`/`SubagentHandback` when available, otherwise return it as the worker result. The native permission UI remains authoritative. |
| Codex | Multi-agent runs provide inter-agent `send_message`; the parent can resume a worker with `followup_task`. | Send the packet to the parent, or return it as the final handback when messaging is unavailable. A protected tool call still requires Codex's approval path. |
| Grok Build | Documented subagents are child sessions that return a summary to the parent; no portable mid-run question channel is part of the documented contract. | Stop and return the packet in the summary. The coordinator asks and redispatches or resumes only when the active Grok version exposes a supported continuation. |
| OpenCode | Background children notify the parent with their result; the built-in `general` subagent denies questions by default, and each child uses its own configured permissions. | Return the packet as the child result. The coordinator asks, then continues the same child session ID when available; parent permission does not replace child permission. |

### Post-commit queue consumption

- **Claimed-row removal is authoritative cancellation
  (`#queue-removal-cancels`):** when an operator explicitly removes a claimed
  `agent:run` row, mid-turn steering emits `dispatch=cancel_claimed` with the
  exact claim owner, bound harness worker, and generation. The binary first
  tombstones that exact identity+generation, acknowledges/drops any pending
  completion delivery, and publishes a retained retirement successor so outbox
  replay cannot resurrect/reinsert/close the row or apply the result. A later
  same-text row is a fresh generation and never consumes stale completion. The
  harness interrupts the old attempt only if it remains alive, at the nearest
  safe consistency boundary, and preserves
  recoverable branch/worktree/commit artifacts, stop further implementation,
  tests, integration, publication, release, and external issue closure, and
  generation-fenced release the claim. Best judgment applies only to shutdown
  mechanics, never to whether sunk-cost work continues. Already-crossed
  irreversible effects are reported rather than undone. The binary expresses
  this binary-to-harness contract; it does not claim to kill a harness worker
  directly. With live editor authority, the destructive transition requires the
  editor's captured operator operations, replayed from the cycle baseline, to
  remove that same claim identity. A whole-buffer projection that omits claimed
  rows while the captured operation lineage edits only another row is not
  cancellation evidence: the claim and any reported completion receipt remain
  intact. Missing, unreadable, mismatched, unrelated, and duplicate lineage all
  fail closed without dispatching worker cancellation; detached disk edits retain
  direct deletion semantics. A durably terminal struck-row deletion is
  presentation-only cleanup. If Markdown strikethrough conflicts with a live
  claim, reported receipt, or completion outbox, steering emits
  `degraded_claim_mismatch`, preserves/fences that work, and performs no
  interrupt, release, integration, apply, publication, or terminal inference.

**Claim closure is part of the strike (`#claimstrike`, GH #303).** Free-text answer strikes and id-backed done/gate/edit consumes release the live claim at the same successful document-mutation boundary. In particular, an id-backed `Reported` attempt must not wait for later disk-based closeout pruning: after its anchored line is struck, a later operator re-add of the same `#id` is new, unclaimed, dispatchable intent and never inherits the earlier attempt's `☑️` marker.

Queue subagent authorization accepts multiple whitespace-separated references in a quoted queue `preset=` value, for example `preset="#push #auth"`. The compatibility alias `presets=` resolves through the same path (`#presetattralias`); when both spellings carry nonempty values, canonical `preset=` wins. `queue brief` must therefore carry a resolved queue-level `#auth` declared with either spelling into `authorization.presets`, rather than degrade the dispatch to `item_text_only` and ask the operator to repeat permission. References resolve independently in source order, duplicate canonical names collapse at their first occurrence, and each structured preset keeps its runbook. Mixed resolved and unresolved references produce `status: unresolved_preset`; resolved details remain visible, but authorization still fails closed.

- **Completed multiline row residue drains with its row (`#queuemultilineclear`).** An indented Markdown continuation at or beyond a completed inline row's content column belongs to that row and drains with it. Unindented, shallow, or otherwise unowned freeform operator text remains a lossless-cleanup blocker.

- After a successful response closeout, required closeouts mark the first prompt complete in the queue in the same locked read/parse/write cycle. If a manual closeout resolves multiple contiguous queued `do #id` prompts with repeated `--done <id>` flags, it may consume that contiguous done-backed head batch in one closeout; it must stop before the first unresolved queued prompt.
- Explicit terminal tracked-work IDs (`--done`, `--backlog-gate`, and `--review-resolve`) also mark matching queued prompts completed in place when they are not part of the contiguous active-head consumption range. These resolutions participate in the same atomic tracked-work target as their backlog/review transitions, including editor-retained closeouts where later response-stage queue cleanup is deferred. The target fails closed if any resolved ID still has a live matching queue head. This preserves an unrelated current head while striking opportunistically resolved queued work, and the document/snapshot must be updated together before commit.
- Queue-head identity checks strip leading priority markers (`:pushpin:`, `:round_pushpin:`, markdown pin/prioritized aliases, and pin emoji) before comparing or classifying the head. A pinned `do [#id]` remains an id-backed directive that requires explicit completion proof; it is not a free-text head completed by any response body.
- Completed prompts remain visible as `- ~prompt text~` while later prompts remain queued. When an answered free-text strike resolves the final live prompt and the remaining body is losslessly discardable residue, that same projection clears the queue body, removes marker-side `start`/`go`/`auto`, and persists the stopped frontmatter state; it must not require a later preflight maintenance pass.
- The consumed prompt range must be completable or drainable from both the live document and the snapshot in a provably identical way; otherwise strict closeouts fail before commit. Non-draining consumption strikes queue items by markdown-AST node key so intentional duplicate prompt text is not conflated.
- IPC payloads keep legacy component `patches` but annotate each patch with an explicit `op`; append-mode boundary patches also carry `node_id`. When before/after document states are available, the payload includes `node_patches` entries shaped as `{component, node_key, op}` plus operation-specific anchors/content for item-level insert, strike, unstrike, replace, remove, and move.
- Queue-consume editor IPC is stricter than generic component convergence: it must carry the queue strike as a `node_patches` entry keyed to the exact pre-consume queue node, carry raw and transient-marker-normalized baseline hashes, and omit a broad legacy `queue` component replace when the node patch is present. A delayed socket/file applier must reject the patch when the live editor buffer no longer matches either baseline hash; `(HEAD)` annotations, boundary comments, guard markers, and managed pipeline frontmatter are transient and do not by themselves make the generation stale. When no editor listener or live sidecar is active, the CLI falls back to the guarded detached disk write instead of replaying a stale component-wide queue replacement. When a listener or live sidecar is active but cannot apply/prove the node patch, the CLI fails closed and logs the blocked write rather than writing behind the editor.
- The watch daemon keeps a per-file markdown-AST node snapshot for watched session documents and logs `watch_node_events` JSON batches on file changes. Each event carries `{component, node_key, op, item_id, before_index, after_index}` plus optional content and neighbor anchors, so realtime queue/enqueue features can consume structural changes without reparsing text diffs.
- **Consumed queue prompts are echoed into their response block (`#queue-prompt-echo-in-response`).** An auto/synthetic queue head is never typed into `agent:exchange`, so a consumed queue turn would otherwise record only the `### Re:` answer with no trace of what it answered. When a closeout consumes one or more queue prompts, the binary embeds the full consumed prompt text as a labeled blockquote (`> **Queue prompt:**` …) immediately after this cycle's response heading, in **both** the document and the snapshot so the selective-commit boundary stays consistent. The response heading is located by the captured response body's first line, falling back to the last non-code `### Re:` heading in the exchange. Only the cycle's own prompt consumption echoes. A terminal tracked-work transition (`--done`, `--pending-gate`, `--review-resolve`) strikes its `agent:run` heads through `plan_queue_completion_strike_projection` (`#donequeuestrike`) and never writes into the response cell: it is a backlog transition, not the consumption of this cycle's prompt, and it runs with no snapshot, so an echo it embedded reached the document only whenever its closing baseline checkpoint did not run. `#closeoutduplicatedcommit` (agent-doc-bugs.md, 2026-10-15 19:51): the tracked-work transaction deferred behind the retained delivery projection after echoing nine `do [#id]` heads, the captured-finalize resume re-baselined without the echo, the live copy of the response differed from the committed one, and commit-time recovery re-appended the whole response. Injection is skipped (fail-safe, content unchanged) when the prompt is empty, the exchange/heading cannot be located, the same echo is already embedded, or the prompt's first line already appears as an exchange line (a user typed it in directly, so no echo is needed). The already-present check also recognizes stale recovery echoes that are blockquoted, one-line-labeled, priority-pinned, or prompt-prefixed variants, so repeated queue-consume recovery does not duplicate a stale free-text prompt in `agent:exchange` while the next id-backed queue head remains open. Multi-line prompts and prompts containing fenced code are quoted verbatim. Because the echo lands before the trailing boundary marker, it is committed history and is never re-detected as an unresolved tail prompt.
- A consumed free-text queue head is not proven by a binary consume marker alone. If the head was present at preflight and is no longer queued, `session-check` requires committed exchange proof (usually the queue-prompt echo or response prose that plausibly answers the head) and fails closed with `#lr-queue-patchback-miss` when that history is missing.
- **Opt-in echo summarization for long prompts (`#queue-prompt-echo-summary`).** By default the consumed-prompt echo copies the prompt verbatim (the deferred-by-the-user behavior: "for now, let's try copying the entire prompt"). Setting `agent_doc_queue_prompt_echo_max_chars: <n>` in frontmatter opts into a bounded summary: when a consumed prompt exceeds `n` characters, the echo records the prompt's first line truncated to `n` characters (on a char boundary) plus the count of elided characters and a pointer noting the full prompt is retained in `agent:run`, instead of the full verbatim text. The threshold is read from the document's own frontmatter; `None` (absent) keeps the verbatim copy. The dedup/idempotency check uses the same rendered echo so a summarized echo is still recognized as already-embedded on re-runs.
- Direct `agent-doc run` / bare-path invocations with `queue_active: true`, explicit `go` mode (`queue: go` or marker-side `go`), and no document diff synthesize the active queue head as the prompt diff. `auto` and `start` are legacy/manual *start* triggers only; continuation is driven by `queue_active: true` plus explicit `go`, so a plain persisted-active `agent:run` without `go` stays inert (`#active-queue-persisted-no-continue`). Binary-owned route writes must not add `auto`, and should strip it from any touched queue tag. Successful closeout consumes one prompt, commits that item, then continues from a fresh run cycle while the next live queue head is still eligible. Continuation stops without consuming the next prompt when the queue drains, a stop fence or time gate reaches the next live head, the next head changed since the snapshot, `go` mode is absent, or a closeout/session-check/verification step fails; diagnostics name the completed item count, next prompt when known, and stop reason. An inactive plain queue (no explicit start/go trigger) never self-starts. **Unwrapped fenced task recovery (`#queue-unwrapped-fenced-task`):** when an otherwise queue-native-syntax-free body contains operator prose before a balanced Markdown code fence and an explicit request after the final fence, the parser treats that whole visible block as one multiline prompt. A fenced console/log dump without a trailing request remains inert `Freeform`, and any ordinary list item, preset, dispatch, control fence, or slash command disables the recovery so mixed queues cannot be swallowed.
- Codex harness-native `agent-doc <FILE>` turns rely on the installed Stop hook for the same queue liveness. After a clean closeout, if the document still has `queue_active: true`, explicit `go` mode, and a ready next head prompt, `codex-stop` blocks the final answer and instructs the agent to continue in the same pane by answering the next prompt and persisting with `agent-doc finalize <FILE>` or `agent-doc write --commit <FILE>`. The hook must not instruct the owner pane to invoke `agent-doc <FILE>` recursively. Its final continuation selection uses the current editor/CRDT-authoritative document content, not a fresh disk read that can lag a converging native save, and the reported prompt strips the transient `🚧` projection. The hook records that normalized requested head prompt and fails closed if a repeated Stop hook arrives without the head advancing, so continuation cannot spin forever on an unchanged queue item (`#stopcurrentqueuehead`).
- **Binary-owned continuation final gate (`#codex-auto-queue-stalled-final-gate`).** The shared `queue_continuation::detect` is the single source of truth (`queue_active: true` + explicit `go` mode + active `resolve_activation` + a ready head that is not a stop fence / future time gate / snapshot-modified head). An accepted controller `admin queue pause` (`#qpausego`) does NOT short-circuit this detector: the pause suppresses only the *unattended* supervisor idle-queue watch auto-injection (the flood it fixes) and is surfaced as `preflight.queue_paused`, `preflight.queue_pause_reason`, and pause-aware `preflight.queue_continuation_guidance`, but the attended in-session `/loop` — and `session-check` / the codex-stop gate that consult `detect` — keep draining real go-mode queue work (stalling them on a pause strands genuine backlog). `queue: stop` frontmatter / `--- stop` fences are the in-session stop control; `admin queue resume` clears the pause flag/reason. `auto` and `start` are start triggers only and are not enough for continuation: a persisted-active queue (`queue_active: true`) without `queue: go` or marker-side `go` is not eligible (`#active-queue-persisted-no-continue`). Every successful binary closeout (`finalize`, strict `write --commit`, `repair`, the already-committed no-op path) reconciles a durable marker under the project state ledger: written when a continuation is owed, cleared when the queue drains / `go` is removed / `queue_active` is false / the head advances. The marker is document-scoped liveness state, not Codex-thread ownership proof. Ambient `codex-stop` handling requires tracked state for its exact current `session_id`; if that binding is absent, it is a no-op and must not attach a pure Codex thread to another thread's document. Tracked repeated stops still fail closed on a non-advancing head. `agent-doc session-check` surfaces this as the typed detail `queue_continuation_required=true` / `next_queue_prompt=…` (informational, exit 0 by default); the strict `agent-doc session-check <FILE> --codex-final-gate` exits nonzero when continuation is required, for Codex direct-exec closeout paths that must not emit a final answer past a stalled queue. Live verification remains gated on `#codex-auto-queue-live-verify`.
- If `session-check` interrupts a Codex Stop hook because the prior cycle is already `committed` but the document has fresh unresolved exchange prompt changes with no new cycle (including active-session post-commit prompt drift), the hook applies the same receiver-first rule as the clean-session path: on the first Stop, a drain-ready supervisor owns the reactive idle steering wake, so the hook allows exit without opening a successor or emitting Codex's `Blocked by hook` continuation (`#postcommitpromptownership`, `#steeringwake`). Without a proven receiver, it treats the change as new in-pane work. Even when `stop_hook_active` is already true, it blocks with an instruction to answer the prompt and persist through `finalize` / `write --commit`; it must not emit the generic "already continued once" stop for a still-open cycle. Before blocking, Stop recomputes the same typed steering items and live queue-claim routing used by the mid-turn hook (`#claimedstopforward`). An item classified `forward_to_owner` remains the claimed worker's work: a diff containing only those items allows the coordinator turn to stop, while a mixed diff blocks for and names the first item that is not externally owned. When that item is a queue line, the block names how the persisted answer closes it (the verbatim `> **Queue prompt:** <line>` echo for free text, `--done <id>` for an id head), because a heading-only answer leaves the line queued to be served again. An item classified `persist_reported` (`#reportedrelease`) is not fresh work: the block says its worker already reported and its claim was released, names the same exact closeout, forbids a new subagent or re-claim, and logs `codex_stop_committed_prompt_diff_reported_worker_unpersisted`. The hook does not capture `last_assistant_message` for it (`#fpestopreplay`): a closing chat summary may span several items and carries no queue echo, so persisting it would neither prove which line it answers nor strike one.
- `#stopsteeringafterabandon`: if the first Codex Stop attempt fails before capture and owner release records an empty `cancel_preflight_cycle_abandoned` predecessor while prompt-bearing steering remains pending, recursive Stop opens one fresh out-of-band closeout cycle from committed `HEAD` plus the current document authority. That successor records `codex_stop_steering_continuation_started`; the opening Stop blocks for the answer, and only a later recursive Stop observing that marker may run the normal strict auto-close. This handoff needs no manual `agent-doc <FILE>` trigger and does not relax recursion for any other abandoned or open cycle.
- If `session-check` reports a retained binary-owned response delivery or an exact `write_applied` captured cycle awaiting its terminal commit, the Codex Stop hook keeps that state on the keyed binary/supervisor recovery path even when `stop_hook_active` is already true. It must not fall through to the generic "already continued once" branch or request another agent-authored finalize/write; the editor buffer remains authoritative and the same capture owns convergence and commit.
- **Auto-queue retained-holder gate names its holder (`#sdkretained`).** Before selecting the next `agent:run auto` head, the Codex Stop hook reads `retained_session_closeout_holder` — the same single read behind `retained_write_blocks_session_closeout` — which returns the concrete durable holder: a write intent whose settlement verdict is `Unsettled`/`Unobserved` (named by its `intent_id`), or the controller's retained compact projection continuation (named by its `continuation_id`; terminal edge `compact_projection_completed`). The pause message cites that key; it never re-reads `pending_document_write` to name it, which cannot see a compact projection and produced the placeholder `intent_id=unobserved-retained-intent` on haiven-dev `tasks/sdk.md` (cycle-1791805221920, 2026-10-12). The wait is derived from the shared ownership predicate (`agent_doc_turn::write_ownership::retained_queue_gate` over `agent_doc_capture_io::retained_write_ownership`): no observed holder → no pause; compact projection → await; write intent → the predicate is asked with `retained_projection` set, so `Deferred` awaits while `DeliveryRejected`, `ReplicaUnserved`, or an unregistered refusing endpoint (`Stranded`) returns that verdict's `retained_write_remedy` (`codex_stop_auto_queue_continuation_recovery ... action=named_recovery`) instead of an unbounded wait. Deferrals log `codex_stop_auto_queue_continuation_deferred intent_id=<key> holder=write_intent|compact_projection`.
- A Codex prompt binding that observed a terminal predecessor is settled when a later response cycle starts after that binding and commits a captured response. The Stop hook must retire and park that exact-turn prompt debt even though `last_prompt_cycle.cycle_id` names the predecessor rather than the later committed cycle; otherwise every final answer repeats the stale "has not crossed the binary-owned write boundary" block after `session-check` already proves committed (`#codex-stop-later-cycle-settles-debt`).
- **Queue-head consumption requires an explicit completion signal for `do [#id]` heads (`#queue-strike-on-halt`), with an exact-id exception for synthetic/preset heads (`#queue-head-consume-on-topic-id-regression`).** A `### Re:` heading that merely names a `do [#id]` queue head is *not* a completion signal — a halt/refusal response names the head precisely to explain why it is *not* being done. The CLI `finalize` / `write --commit` path consumes a `do [#id]` head only when the cycle (a) carries a closeout flag — `--done <id>`, `--backlog-gate <id>`, or `--backlog-edit "<id>=…"` — that names the head id, or (b) the head is a genuine fresh operator prompt-target / `do queue` trigger in the cycle diff. For a **synthetic/preset head** — a natural-language prompt carrying a trailing `#preset` id rather than a bare `do [#id]` directive — it additionally consumes (c) when the response heading topic resolves to **exactly** the head id, for example `### Re: #spec-test-build-install-commit-push` completing a synthetic head whose preset id is `#spec-test-build-install-commit-push`. A heading topic that merely contains the id with trailing modifiers (`### Re: #id halt` / `### Re: #id deferred`) never counts, for either head shape. The Codex Stop-hook auto-close path (which has no closeout CLI flags) consumes from a heading on an **exact** topic match (`### Re: do [#id]`) or a topic that resolves to **exactly** the head id (`### Re: #id` for head `do #id`), never on a heading with trailing modifiers.
- **Explicit multi-head drain (`agent-doc queue consume <FILE> [--count N]`, `#multi-head-consume-one-per-finalize`).** The free-text strike heuristic consumes only ONE head per finalize — the head current at that cycle's preflight. When a single turn answers several free-text heads (for example the operator adds new queue items mid-turn and the response addresses all of them), the trailing answered heads stay queued and re-serve on the next auto-loop, producing duplicate-response churn. `queue consume` drains those answered stragglers deterministically: it strikes the leading `N` (default 1) free-text head(s) — the agent asserting they are already answered, the same explicit contract `--done <id>` gives an id-backed head. It is scoped to free-text heads: a leading id-backed (`do [#id]`) head bails with guidance to reap completed/gated work through `--done`/`--backlog-gate`, to explicitly acknowledge correction heads with `--ack-id`, or to leave live work queued. It writes document + snapshot like `queue sync`; the caller closes out through the normal commit boundary. There is deliberately no fuzzy head↔response auto-matcher — guessing which heads a response answered risks deleting a genuinely unanswered prompt, a worse failure than the churn. **Orphaned id-backed head escape hatch (`agent-doc queue consume <FILE> --id <id>`, `#orphanqhead`).** When a backlog item is reaped via `--done` but the matching id-backed queue head strike is lost to IPC drift, the head becomes an undrainable phantom: `--done <id>` is a no-op ("already resolved") and the normal `queue consume` rejects id-backed heads ("reap via --done"), so it sits forever and re-fires the auto-loop. `--id <id>` strikes that specific id-backed head (`do [#id]` / `[#id]` / `#id`) in document + snapshot. It refuses (non-zero, with guidance toward `--done`/`--backlog-gate`) when the id still names an OPEN backlog item — that is live work with a real drain path, never an orphan — so the escape hatch can never silently desync live work. **Open-backlog correction acknowledgement (`agent-doc queue consume <FILE> --ack-id <id>`, `#freshqueueauth`).** `--ack-id` is the explicit inverse proof: the id still names open backlog work, but the exact id-backed queue head is only a correction/acknowledgement to clear. It strikes the queue head in document + snapshot while preserving the open backlog item, and refuses prose heads that merely mention the id (answer those as free text). `--id` and `--ack-id` are mutually exclusive with `--count`.
- **Paused direct-steering acknowledgement (`agent-doc queue consume <FILE> --ack-text <TEXT>`, `#paused-free-text-ack`).** A response completed through direct harness-console steering may have no selected queue-head id because a stopped queue was never dispatched. The ordinary inactive-queue consume therefore remains fail-closed. `--ack-text` supplies an exact operator proof for only the leading free-text row: priority markers are ignored for a canonical queue item and all remaining text must match exactly; when projection damage removed that row's Markdown list marker, the byte-exact bare row may be struck through its unique parser span only if no canonical prompt exists. Id-backed heads are refused. The write converges through editor authority, adds the normal queue-prompt provenance echo to the retained response, re-baselines the recovery snapshot, and preserves the paused frontmatter state; it never resumes or drains subsequent work.
- **Long-running closeout preserves exact completion authority (`#longrunstrike`).** Claim TTL expiry ends dispatch ownership and permits recovery or reassignment; it does not erase the durable attempt's identity while that same queue row remains open. A later closeout may therefore strike a post-baseline row after the claim has expired, but only when the response is materialized, contains the exact `> **Queue prompt:**` echo for that row, and does not defer it. This lets coordinator runs longer than the default two-hour claim TTL retire completed subagent work without extending stale ownership or weakening the post-baseline mutation fence.
- **Queue-item claims (`agent-doc queue claim|release|claims <FILE>`, `#queueclaim`).** A queue head handed to a worker outside the in-session loop (a background subagent dispatched under a `#subagents` preset) is *claimed*: `agent-doc queue claim <FILE> --item <ID_OR_TEXT> --owner subagent:<label> [--ttl-secs N]`. The claim keys on the queue head's **work identity** (`agent_doc_element_queue::queue_head_identity`, `#claimdispatchidentity`), the one key that claims, `queue_subagent_dispatch`, mid-turn steering, in-session drainability and preflight selection all share. It strips lifecycle/priority markers (`📥`, `🟡`, `🚧`, `☑️`, legacy `📫` / `📤`, `⚠️`, `🔄`, `❔`, `⏭️`, `⏳`, `📌`, `**pin**`) and leading intent/preset tags (any leading `#tag` or `#tag:` token that starts with a letter and is followed by more text: `#subagents`, `#subagent:`, `#gh-fix`, `#bug:`), then keys the remainder as `#id` when it is id-backed (`do [#a]`, `[#a]`, `do #a`, a bare `#a`) and as whitespace-collapsed lowercased text otherwise (the URL or prose). So an operator edit that only adds, removes or swaps tags or markers keeps the claim: a claimed `#subagents: <url>` edited into `#subagents: #gh-fix <url>` stays claimed, is not pruned as closed, and is not offered again in `queue_subagent_dispatch`. **A claim is bound to its line, not to the line's text (`#claimfollowedit`).** Each claim stores an *anchor*: the queue's live heads (normalized, markers stripped) when it was made or last followed, and the indexes of the lines it holds (`QueueClaim.anchor`). **A queue-row mutation is steering, never implicit unregistration (`#queue-mutation-steering`).** On every ledger read the anchor is aligned with the live heads by the same rule mid-turn steering uses to report `previous` -> `verbatim` edits (`agent_doc_queue::queue_head_alignment::pair_queue_heads`, shared code): Myers over work identities pairs unchanged and moved lines, then inside each replace hunk similar free-text rows pair first and, when the hunk keeps its row count, still-unpaired rows at the same structural offset pair too. A live claim whose identity is no longer a live head follows the line its anchored line became, so any structurally aligned replacement of the claimed row — a corrected URL, reworded prose, appended note, complete rewrite, or changed tracked id — keeps the claim and its exact owner/generation, is not pruned as closed, and is not offered again in `queue_subagent_dispatch`. Nothing is hidden by this: the same edit is surfaced to the coordinator as `dispatch=forward_to_owner` with `previous` and `verbatim` (`#claimedsteerwake`, `07-closeout-commands.md`), and the owner decides how the mutation changes its work. Before this, the claim keyed on the line's text: steering forwarded the edit to the owner while the ledger dropped the claim (`queue_claim_prune pruned=1`), so the next preflight offered the edited line to a second worker (agent-doc-bugs.md, 2026-10-15). What does not follow: a claimed line deleted while a similar line is added in another hunk, a line another live claim already holds, and a claim explicitly closed with its queue head. Only an old claimed row with no same-slot successor is removal/cancellation. An expired active attempt becomes a retained, nonblocking `Lost` record so its lineage remains visible and recovery-eligible. Two identical lines are one work identity, so a claim made on text that matches several live heads binds all of them (`queue_claim ... occurrences=N`): the claim holds while any of those lines, or any line edited from one of them, is live, and when an edit leaves several such lines with different texts it follows the first in queue order instead of being dropped. That multi-copy binding is for an *unlocated* claim only: once a claim's anchor locates its row, or exactly one candidate row carries its exact `[#owner#generation]` projection, a text-identical sibling added later never joins it and an `⟦effect:N⟧` annotation moves it to its own row's successor (`#claimrowmigrate`, § Duplicate queue-effect identity). A claim stored before anchors existed is anchored on the first read that sees its line; until then it follows only an *edit continuation* (`queue_claim::is_edit_continuation`, `#claimfollowsedit`: one normalized free-text remainder is a prefix or suffix of the other at a word boundary, the shorter side carrying at least eight alphanumerics). The follow is applied in memory on every ledger read and persisted on the next mutation and at closeout pruning (`queue_claim_followed_edit`); the claim's `item_text` becomes the new line and its earlier texts are kept in `followed_from` (last eight), so `claim`, `--refresh` and `release` by the text the worker was handed resolve to the line it became (`queue_claim_resolved_followed_edit`). Every closed claim and active-to-lost expiration is logged with its owner, generation, and cause before the aggregate maintenance result, so a lifecycle transition is never silent. CRDT convergence and dedup keep the tag-sensitive `QueueItemIdentity::from_prompt`, because they decide whether two lines of text are the same line, and collapsing `#gh-fix <url>` with `#review <url>` there would delete operator text. A ledger stored under the older tag-sensitive key is re-keyed from each claim's `item_text` on load (claims that now share one identity merge; the later expiry wins). `queue claim`, `--refresh`, `start`, `report`, `release`, and `abandon` resolve `--item` only against the live editor authority's current executable heads when an authority cut exists; disk is authoritative only when no live cut is available (GH #296). Thus an unsaved editor-added head remains claimable, while a disk head deleted in the editor immediately yields the same `QueueClaimMiss` as a typo and creates no ledger row. An authority-only resolution logs `queue_claim_resolved source=editor_authority reason=head_not_yet_saved_to_disk`; a detached miss lists the disk heads, and a live-authority miss lists only the authority heads. The claim is stored per document in `state.db` (`queue_document_state`, `state_kind = "queue_claims"`), read-modify-written inside `BEGIN IMMEDIATE`. Claiming an item that is not a live queue head fails. Every claimed queue head visibly carries its typed lifecycle marker in both the queue and its id-backed tracked-work row (`#claimmarker`): `🟡` claimed/dispatching, `🚧` positively refreshed/running, `☑️` worker-complete handoff/awaiting integration, or `⚠️` lost/expired. Every marker is excluded from claim identity; release or queue removal clears it, while expiry retains `⚠️` until recovery or reclaim. A claimed head is excluded from the in-session drainability filter: `queue_continuation::detect` / `detect_for_content` (and therefore the continuation marker, `session-check`, and the Claude/Codex Stop hooks), preflight's `queue_drainable_head_count` / `queue_continuation_required`, preflight's `selected_queue_prompts`, and preflight's supervisor-scope drainability (`queue_supervisor_drainable`) plus its synthetic queue-head diff (`#explicitrunqueue`). A claimed head is never the cycle's synthesized `+do [#id]` continuation diff, so an explicit Run Agent Doc (or any preflight) on a queue whose only live heads are claimed, with no exchange prompt and no other document edit, reports `no_changes: true`, opens no response cycle, records no `expect_done_or_gate_ids`, leaves the claim untouched, and names each claim in `no_changes_explanation.claimed_queue_items` (`claimed by <owner>, expires <t>`). Before this, the synthetic diff fell back to the first live head regardless of claims, so every press re-synthesized the same `+do [#id]` (the baseline never advanced past it), opened a cycle with `queue_drainable_head_count: 0`, and forced a no-op response that tripped `expect_done_or_gate_guard`. A real operator edit beside a claimed head still admits a cycle as before. When every remaining head is claimed or deferred, no continuation is required: the Claude Stop hook allows the final answer and logs `claude_stop_queue_continuation_skipped reason=heads_claimed_in_flight ... state=waiting_on_claims`. A claim does not exempt a head from being struck as answered (`#claimstrike`): the claim names the head's owner, while the response's `> **Queue prompt:**` echo states that the work is done, so a claimed free-text head that the cycle's response echoes with no deferral (`**Deferred:**` lead or deferral phrasing) is struck by finalize's `#ftstrike` (and by the controller's answered-head projection, the commit-time late-strike recovery, and preflight's `#qheadresidue` catch-up), and the strike releases its claim in the same closeout (`queue_claim::release_closed_head`, `queue_claim_release ... reason=closed_by_strike`). These strike sets therefore equal the session-check completed-residue set, which never consulted claims; exempting claimed heads (the `#deferstrike` rule of 0.35.453) let a closeout commit with the echoed head unstruck and session-check then INTERRUPT on `#qheadresidue` until the claim was released by hand. A claimed head the response echoes with a deferral stays queued and claimed. The finalize pre-write evidence gate still owes no answer for a claimed head this cycle selected. A claim ends when the item leaves the queue (pruned at closeout by `reconcile_marker`) or on generation-matched `agent-doc queue release <FILE> --item <ID_OR_TEXT> --generation N`; TTL expiry (default 7200s) transitions the attempt to retained nonblocking `Lost` instead of deleting its lineage. The TTL exists because a background subagent exposes no pid or heartbeat agent-doc can observe, so a crashed worker could otherwise strand the head forever. The coordinator keeps a long-running worker's claim alive with `agent-doc queue claim <FILE> --item <ID_OR_TEXT> --owner <OWNER> --refresh --generation G [--ttl-secs N]`, which moves the claim's expiry to now + N seconds (default 7200) and keeps `claimed_at_secs`. A refresh never creates or takes over a claim: it is refused, with a usage error and no ledger change, when the item has no claim, when the claim has already expired, or when another owner holds it. Each refresh logs `queue_claim_refresh outcome=refreshed owner=... expires_at=...` or `queue_claim_refresh outcome=refused reason=unclaimed|expired|other_owner|stale_attempt owner=...`. A plain same-owner re-claim refreshes the same attempt; a different owner cannot steal a live attempt. Reclaim after `Lost` creates a new monotonic generation. An unreadable claim ledger is reported on stderr and treated as no claims (the pre-claim behaviour). The supervisor idle-queue watch does not consult claims yet. **Owner-session liveness (`#claimownerliveness`).** A claim also records the coordinator session that made it (`owner_session`: `harness`, `session_id`, `pane`, `pid`), read from the claiming process's environment (`CLAUDE_CODE_SESSION_ID` or `CODEX_THREAD_ID`, `TMUX_PANE`, `CLAUDE_PID`). A subagent inherits its coordinator's environment, so a claim it makes binds to the coordinator session too. The turn-status hooks also record the session id each pane's main harness session last reported (`UserPromptSubmit`, `Stop`, `SessionStart`; never `SubagentStop`), as the `pane_harness_session` coordination row. A claim's owner session has **ended** when the recorded harness pid is gone (`owner_process_exited`), or when its pane runs a different session of the same harness (`session_replaced`): the caller itself sits in that pane under another session id, or, for Claude only, the pane's hook reported another session id at or after `claimed_at_secs`. A `/clear` is a new session in the same process, so it reads as `session_replaced`. A claim with no recorded session (made outside a harness, or stored before this field existed) is `unrecorded` and judged by expiry and closure only. Policy: an ended owner session **does not release the claim**. A background subagent spawned before `/clear` can legitimately outlive it and report back, so the claim keeps holding the head out of inline drainage and dispatch until its TTL, exactly as before. What changes is what agent-doc asserts. The `no_changes_explanation` names the head as a **stale claim**: its `claimed_queue_items[]` entry carries `owner_session_ended {reason, detail, adopt_command, release_command}`, and the guidance says nothing proves the work is in flight and that the agent must not report it as running. It offers the two exits instead of "do NOT release": **adopt** with `agent-doc queue claim <FILE> --item <ID> --owner <OWNER> --refresh --generation <N>` when one of the current coordinator's live subagents is doing the work (a refresh re-binds the claim's `owner_session` to the caller, so it reads as live again), or **release** with `agent-doc queue release <FILE> --item <ID> --generation <N>` so the head is dispatched again. A live claim beside it keeps the in-flight wording. `queue claims` reports each claim's `owner_session_state` (`live`, `unrecorded`, `ended`) and `owner_session_end_reason`. `queue_claim` / `queue_claim_refresh` ops-log lines carry `owner_session=<harness>@<pane>:<pid>` (never the session id). Observed 2026-10-11 on haiven-dev `tasks/sdk.md`: `#cligenport` stayed claimed by a pre-`/clear` subagent that had finished a different item and never started this one, and preflight's "do NOT release" guidance made the post-`/clear` coordinator, which had no live subagents, report it to the operator as running.
- **Fenced claim attempts and typed runtime projection (`#queueclaimstate`, `#claimmarker`, `#queueclaimworker`).** Each queue-item claim attempt is one `a = (work identity, owner, generation)` record. A new identity starts at generation 1; reclaim after a lost/expired attempt increments the generation; a live claimed/running/reported attempt cannot be stolen. Dispatch is a three-boundary protocol: `queue claim` creates `Claimed(a)`/`🟡`; the harness spawns a worker and returns its stable identity `w`; then `queue start <FILE> --item <item> --owner <owner> --generation N --worker-id <w>` durably binds that exact attempt and creates `Running(a)`/`🚧`. The binary never infers spawn success from an owner label, intended tool call, process poll, or TTL heartbeat. The safety invariant is `Running(a) => exists exactly one w: worker(a) = w`, and the live binding is injective within the document: `worker(a) = worker(b) => a = b`; `queue start` is the only command transition that creates `Running`, and loading downgrades any unbound legacy `Running` row to `Claimed`. Replaying the exact `(a,w)` start is idempotent; a different worker for `a`, the same worker for another live attempt, a missing/stale generation, a foreign owner, or a terminal attempt is refused without mutation. Thus a late acknowledgement can never advance a reclaimed generation. `queue claim ... --refresh --generation N` only extends an already-bound running attempt; it cannot promote `🟡`. `queue report-complete ... --generation N --response <TEXT>` requires acknowledged running state plus a non-empty complete worker response and records both; placeholder-only reports are refused; `queue release ... --generation N` ends that exact attempt. The completion key includes work identity, owner, generation, claim creation time, and the immutable bound worker id. Therefore an unstarted, stale-generation, or foreign-worker cell cannot report the current attempt. Exact durable completion evidence may recover `Reported` only for its matching current attempt; it never creates `Running`. Generation-zero rows loaded from pre-generation state remain compatible with claim identity but use the explicit start boundary before new heartbeat/report transitions.

  The same typed target drives every id-backed tracked-work projection: `Claimed`/`🟡`, `Running`/`🚧`, `Reported`/`☑️`, `Lost`/`⚠️`, `Reconciling`/`🔄`, and `Unknown`/`❔`. A completion transition replaces a stale tracked-work `🚧` with `☑️`; release or queue closure clears the prefix. Pending, skipped, and awaiting-operator queue presentation does not decorate tracked work.

  Durable state projects through the current editor/CRDT authority as one binary-owned prefix: `📥` pending/unclaimed, `🟡` claimed/dispatching (no positive worker evidence), `🚧` worker-bound running (only after `queue start`), `☑️` worker-complete handoff/outbox or receipt awaiting coordinator integration, `⚠️` lost/expired/recovery-eligible, `🔄` exact work evidence awaiting reconciliation, or `❔` degraded/unknown because durable state cannot safely be interpreted. A live attempt or receipt also carries the normal-bracket projection `[#<owner>#<N>]` (for example, `[#subagent:sdk#2]`); it is metadata, not work identity. The parser continues to accept former reported `📫` and `📤` markers and normalizes them to `☑️` on the next projection write. It also accepts legacy `⟦<owner>#<N>⟧` and `⟦owner=<owner>;generation=<N>⟧` annotations and normalizes them to the normal-bracket spelling. The blocking `🙋` marker composes directly with lifecycle (`🙋🚧`) and `[#request-id]` is appended as secondary metadata, while `⏳` retains its existing awaiting-operator meaning. Every projection prefix, request suffix, local request answer, and owner annotation is stripped before queue identity, matching, editing, striking, and reclaim, so state changes never retarget work. Reported attempts do not expire while awaiting integration; expired claimed/running attempts become retained nonblocking lost records rather than disappearing. Redispatch replaces only lost/explicitly abandoned attempts. Claim creation, start acknowledgement, heartbeat, report-complete, expiry maintenance, release, closeout, preflight, and controller hydration each derive the desired projection from durable ledgers and converge that authoritative cut; unrelated in-session markers, operator-action markers, whitespace, and operator edits are preserved. After every ledger transition, projection first absorbs the latest operator/editor text, reapplies all ledger-derived state by exact work identity and current generation, rereads the live authority, and succeeds only when that cut is settled; a merge that chooses the editor cell is retried from the new cut and bounded exhaustion is reported as an error rather than false success (`#queueclaimprojectionrace`). `queue claims` reads this same live authority, so unsaved normalization and marker-only edits cannot hide a durable claim. Durable binary completion feedback contains an attempt-scoped key and reconciles only a matching current/legacy-generation-zero claim. Evidence whose attempt record is gone, retired, cancelled, or whose generation differs is quarantined: it cannot manufacture ownership or mutate a reclaimed item. The projected head set is persisted in cycle state so the command's own marker write is not reclassified as operator retargeting on the next preflight.
- **A reported attempt is coordinator writeback debt (`#reportedwritebackdebt`).** `Claimed` and `Running` heads remain excluded from the coordinator's in-session queue prompt because their workers still own execution. `Reported` is different: it remains fenced from redispatch, but the worker has completed and the coordinator owns integration, verification, and closeout. Therefore every continuation surface — preflight selection/counts, session-check, supervisor fallback, and Codex Stop — must treat an active `Reported` head as unresolved coordinator work, even when the preceding response cycle is already committed and even when the claim's recorded coordinator session has ended or been replaced. The shared out-of-loop projection contains only `Claimed`/`Running` attempts plus dispatchable or capacity-held subagent heads; it must never hide an active `Reported` attempt merely because its durable claim remains attached. An already-committed status message cannot settle that later report. Stop must keep the turn alive (or yield to a proven ready supervisor) until the result crosses the normal binary-owned write boundary and the queue row closes. The durable claim continues to block redispatch throughout; making the head visible to writeback never makes it fresh worker work.
- **Reported projection requires a visible receipt (`#reportedprojectionreceipt`).** A durable `Reported` attempt projects as `☑️`; retaining a CRDT successor is acceptance, not proof that the editor received it. `queue report-complete` may return promptly with the claim and completion cell durably retained, but when no live editor replica can acknowledge that successor it must print the retained intent and the concrete next action: open the document in the active editor/project and run `agent-doc queue repair-projection <FILE>`. A later completion report rebases the retained successor over the fresh editor-authoritative cut before adding its own cell and marker, so concurrent operator queue edits are neither attributed to the report's scope nor dropped. When a live claimed row is edited, every lifecycle mutation (`start`, refresh, report, release, or abandon) persists the same anchor-derived `followed_from` rekey that read-side `queue claims` resolved before addressing the attempt; the original dispatched text and the current exact text therefore reach one generation-fenced claim. Claim/runtime projection likewise patches only the marker/attempt annotation inside each exact parsed source span; it must not canonicalize a recovered multiline wrapper, insert a fence, or rewrite a concurrently added neighboring row. That repair settles the retained write through editor authority before rederiving markers; it never releases the claim, consumes the row, or writes disk behind an editor-owned buffer. `queue claims` reports `visible_projection_state`, expected/observed markers, and an executable `next_action_argv` for every mismatch. Thus the state-machine liveness invariant is explicit: an active claim whose durable marker differs from every visible row must have either an in-flight retained delivery or a surfaced repair action; retained acceptance alone is not terminal success.
- **A missed dispatch acknowledgement is a coordinator obligation (`#queueclaimworker`, GH #301).** Let `age(a) = now - claimed_at(a)` and `g = 10s`. For every live attempt, `Claimed(a) && worker(a) = none && age(a) >= g` is `DispatchAcknowledgementMissing(a)`, not in-flight worker evidence. The predicate is owned by `agent_doc_queue::queue_claim::dispatch_evidence_missing`; visible-marker drift is independent and cannot hide it. Every preflight re-evaluates the predicate and emits an operator-visible `queue_dispatch_acknowledgement_missing` warning. Strict closeout (`respond` / `write --commit`) fails before response capture, and the Claude Stop hook blocks the final answer instead of logging `heads_claimed_in_flight`. Every boundary names both generation-fenced exits with the claim's actual item, owner, and generation: acknowledge a successful spawn with the exact `queue start ... --worker-id <worker-id>` template, or abandon the unstarted attempt with `queue release ... --generation N --requeue`. `queue claim` prints the same next-step pair immediately. `Running`, `Reported`, `Lost`, an unexpired `Claimed` attempt inside the grace, and a claim whose row is no longer live do not produce this obligation. These are actorless one-shot command boundaries over the durable claim ledger: they share the typed predicate and perform no polling or retry loop; `queue start`/`release` remain the only state-changing effects. `now` and `claimed_at` are read from one clock, `agent_doc_queue_io::queue_claim::claim_clock_now_secs` (`#finalgrace`): the system clock in production. A debug build (never an optimized one) lets a test pin it, either for its own thread (`pin_claim_clock_for_current_thread`) or for a spawned CLI process (`AGENT_DOC_TEST_QUEUE_CLAIM_NOW_SECS=<epoch secs>`), so a test that leaves a claim unacknowledged on purpose gets the same outcome however long it runs. A test never relies on finishing inside `g`.
- **Every queue row has an authoritative visible state (`#queueworkevidence`).** For a queue row `q`, define `A(q)` as its current-generation claim attempt, `R(q)` as its exact-attempt reported-release receipt, `F(q)` as a structurally binary-attested completion cell for that generation, and `T(q)` as durable integrated/retired terminal state. An ordinary live row with none of `A`, `R`, `F`, or `T` projects `📥` pending; it is never bare merely because no worker owns it. `A` projects its typed state and owner/generation; `R` projects `☑️`; `F` without a governing exact attempt projects `🔄`; unreadable, contradictory, or manually drifted state projects `❔` and blocks dispatch instead of asserting false ownership. A canonical local answer row shaped `[#subagent-operator-request-<id>]: <answer>` is keyed by that reserved request id instead of an empty free-text identity, so pending and worker-bound lifecycle state stays visible while the answer routes. The same bytes inside a balanced inline-code span are evidence only: annotation discovery ignores them, queue identity keeps the surrounding prompt intact, and `queue brief` carries the full prompt verbatim. Proven integration closes/strikes the row and retires the exact completion key. A strike is terminal against finished-work evidence (`#strikeisterminal`): `R`, `F`, and a `Reported` or `Lost` attempt all say the worker is done, which is exactly the state integration strikes, so none of them reopens a struck row, whichever path struck it (closeout echo, preflight residue strike, `queue consume`, the orphan-id strike, the controller's completion projection, or the operator). Only a current `Claimed`/`Running` attempt contradicts a strike; reconciliation then reopens the row as `❔` while preserving and fencing that attempt. Preflight lifecycle markers, the claim-command projection, and the relay attempt projection all decide this through one predicate, `queue_claim::struck_row_reopened_by_claim`. A strike also ends that evidence (`#strikeretires`). `R` and `F` are keyed by work identity, and `F` names its row only by text, so evidence that outlives its struck row would govern the next row with the same text: live 2026-10-26, a 52-hour-old `release_publish_patch` cell painted `🔄 release + publish` onto a new release request. Every binary strike therefore retires, for each row it strikes, the row's structurally attested completion cells in the same successor (document and snapshot, through `queue_consume::retire_struck_rows_completion_feedback` / `retire_consumed_completion_feedback`) and, once that successor is accepted, the row's reported-release receipt (`claim_release_receipt::retire_struck`). This holds on every strike path, not only closeout `queue consume` (whose receipts end at the same closeout's claim/receipt prune): the closeout echo / `#ftstrike`, the preflight `#qdonestrike`, `#qheadresidue`, and `#qftbklgstrike` strikes (retired before a drain clear can remove the struck rows that identify them), the orphan `queue consume --id` strike (a full-text successor, since the cell lies outside the queue node ops, fenced against a retained target that still carries the cell), the `#goqstall2` noise/orphan-head prune, and the controller's answered-head projection. The rows a successor strikes are `queue_consume::heads_struck_by_successor(before, after)`: live in `before`, struck in `after`, with no live row of the same identity left in any queue. A strike that leaves a live row of that identity (a mirror copy or a duplicate) keeps the evidence for that row, and a deleted row goes through cancellation (`#queue-removal-cancels`) rather than this rule. Retirement removes only binary-attested cells; operator exchange text and heading-shaped prose are never touched. A receipt is never retired before the strike successor is accepted, so a failed write cannot drop work evidence from a still-live row; a receipt left by a failed retirement is pruned by the next closeout. The design retires the transient cell rather than fencing a retained cell by generation: the cell is a temporary "awaiting integration" handback that `#subagentcompletionfeedback` already retires with the queue strike, and a generation fence would have to reach every completion-evidence reader, several of which see only document text. Operator strikes and evidence left by strikes before this rule are outside it. Before, a receipt or completion cell also reopened a struck row, and because not every strike path retired them then, the preflight residue strike was undone in its own pass (live 2026-10-26: 24 struck rows came back as `❔`, then `☑️` from a receipt or `🔄` from a completion cell, and steering offered them for fresh dispatch). Deleting a row backed by genuine durable terminal state is presentation-only cleanup; deleting live work is authoritative cancellation and moves its exact attempt to the cancellation ledger. Cancellation and retirement records never project artifacts onto a later same-text row, and generation allocation fences stale completion/outbox replay. Cosmetic prefixes alone are never authority. External state such as a GitHub issue being closed is never polled by marker projection and is not, by itself, authority to edit the document.
- **Crash-safe reported handoff (`#queueworkevidence`).** Reported release is ordered `A(Running) -> A(Reported) -> A(Reported) + R(a,w,k) -> R(a,w,k)`; only then may closeout produce `Integrated` and remove the live row. `R(a,w,k)` retains the exact generation `a`, worker identity `w`, and binary completion key `k` after the live claim is removed. Thus every durable crash cut before closure satisfies `WorkEvidence(q) = A(q) || R(q) || F(q)`, and `WorkEvidence(q)` is monotonic for the lifetime of `q`.
- **Reported-queue status is visible without inventing work (`#queuehandoffstatus`).** When one or more live queue heads are `Reported` (`☑️`), preflight projects one aggregate `📤 Queue handoff: N reported item(s) awaiting coordinator integration, verification, and closeout.` line into `agent:status`, updates the count idempotently, migrates the former `📥 Queue handoff:` spelling, and removes the line when no reported heads remain. Existing status prose is preserved. The line describes the coordinator obligation; current execution remains the turn-status/pane-title `⟳` signal. Agent-doc MUST NOT add a synthetic queue row for coordinator activity because queue rows are actionable FIFO work and such a row would be eligible for dispatch/consumption. If editor convergence lands this exact projection after Git has already committed the same cycle's response snapshot, the terminal retry treats the byte-exact ledger-derived status delta as owned closeout work and commits it; unrelated status drift remains blocked.
- **Controller-handoff claim projection (`#controllerhandoff`, `#claimmarker`).** Let `D(a)` be the durable state of attempt `a`, `M(D(a))` its unique lifecycle marker (`Claimed -> 🟡`, `Running -> 🚧`, `Reported -> ☑️`, expired/lost -> ⚠️`), and `H(c)` any canonical document cut reconstructed during controller handoff from disk, a recovery projection, or a retained editor replica. Before `H(c)` becomes readable authority, hydration computes `H'(c) = overlay(H(c), {q(a) -> (M(D(a)), owner(a), generation(a))})` and projects unrelated unclaimed live rows as `📥`. For every live row owned by `a`, `marker(H'(c), q(a)) = M(D(a))`; every unrelated row and non-queue byte is unchanged. `overlay(overlay(c, D), D) = overlay(c, D)`, so restart/reconnect replay is idempotent. A stale disk, snapshot, or editor cut may contribute operator text but cannot regress durable attempt state, attach stale-generation artifacts, or hide an unknown state; ledger-read failure surfaces as a diagnostic or `❔`, never a falsely certain marker. The overlay occurs both when constructing a cold relay and on the single retained-replica reseed transition before that temporary relay becomes readable; the resulting CRDT delta is delivered to the editor rather than written behind it on disk.
- **Reported release (`agent-doc queue release <FILE> --item <ID_OR_TEXT> --generation N [--requeue]`, `#reportedrelease`).** A generation-matched plain release is the coordinator's statement that the claim's worker reported and its result is not persisted yet: it ends the claim, records an exact-attempt release receipt (`state_kind = "queue_claim_release_receipts"`, separate from the claim ledger; logs `queue_claim_release_receipt_recorded`), and prints the persist-now closeout for the line (`[queue] next: ...`: the verbatim `> **Queue prompt:** <line>` echo for free text, `--done <id>` for an id head). A receipt can be created only from `Reported(a)` with a bound worker and nonempty completion key. Let `E(q)` mean an exact current-response echo, `D(q)` a deferral, `B(q)` presence in the turn baseline, and `W(q)` exact live-claim or exact-attempt-receipt authority. A free-text closeout may strike only when `E(q) && !D(q) && (B(q) || W(q))`; a lifecycle emoji alone never satisfies `W(q)`. This makes a released `☑️` row deterministic when the baseline still predates it, while unstarted, stale, foreign, legacy owner-only, or hand-marked rows fail closed. While any receipt governs (the line remains live and no newer current-generation claim or terminal record supersedes it), `queue_subagent_dispatch` never offers the line to a new worker, preflight projects `☑️` plus its owner/generation, and continuation paths tell the coordinator to persist the result. Time alone never erases this work evidence. `--generation N --requeue` forgets any report receipt but retains the attempt as nonblocking `⚠️`, so a new generation may reclaim it without the row appearing unstarted. The finalize that strikes the line ends both its claim (`#claimstrike`) and, at closeout reconciliation, its receipt (`queue_claim_release_receipt_prune`). Before this, release discarded the only fact that the line was answered: on haiven-dev `tasks/docs.md` (2026-10-15) the coordinator released `subagent:page-status-enum`, stopped with the answer only in chat, was bounced to "answer" the line, persisted a heading-only answer that left it queued, and dispatched a second worker for the same question.
- **Subagent authorization (`agent-doc queue brief <FILE> --item <ID_OR_TEXT> [--json]`, `#waypostauthorization`).** A dispatched queue subagent sees only the prompt its coordinator writes, so the binary resolves the operator's authorization for each dispatch item from the document and carries it in the item's `queue_subagent_dispatch` entry as `authorization`, next to `claim_command` and `brief_command` (the `queue brief` command for that item). `authorization` holds: `status`; `item` (the queue line verbatim); `tracked_items` (`{id, component, text}` for every id the head names that a `backlog`/`review`/`pending` item carries, text verbatim with continuation lines); `unresolved_ids` (named ids no tracked item carries, reported rather than guessed); `presets` (`{name, source, resolved, body?, runbook?}`, where `source` is `queue_attr` for the selected queue's `preset=`, `queue_line` for a body-level `preset <name>` line, or `item` for a registered `#preset` token in the line; names resolve through frontmatter `prompt_presets`/`presets` with or without the leading `#`); and `subagent_prompt_preamble`, the ready-to-paste text. The preamble quotes the item, the tracked-item text and each resolved preset body verbatim (with a runbook-load line for a preset that declares one), scopes the authorization to that one item, and restates the coordinator rules: the subagent works only in its own worktree outside the IDE-watched project, writes command output, logs and scratch files inside that worktree rather than to a fixed name in a shared scratchpad or temp directory (sibling subagents share the coordinator's scratchpad, and concurrent runs truncating one `check.log` interleave, so one subagent read another worktree's `make check` output as its own; `#checktmpdirleak`), never runs `make install`, a release build, a version bump, a tag or a release (the coordinator runs the single install), never edits, writes or commits the session document or runs agent-doc write/respond/queue commands against it, commits and pushes only its own branch fast-forward and only when the authorization includes commit/push, and reports back branch, SHA, files and test results. `status` is `resolved` when every applicable preset resolved, `item_text_only` when no preset applies (the preamble then says the item text grants no outward action it does not itself name), and `unresolved_preset` when the queue declares a preset frontmatter does not define. That case fails closed: the preset's `resolved` is `false`, it has no `body` (none is ever invented), and the preamble carries the `AUTHORIZATION NOT RESOLVED` marker telling the subagent that nothing beyond the verbatim item text was authorized and not to commit, push, install, release or publish. `agent-doc queue brief` resolves `--item` the same way `queue claim` does (on disk, then in the editor authority's text while a save is deferred), refuses an item that is not a live head, and prints the same preamble (`--json`: the whole object). The mid-turn hook and closeout steering name the `queue brief` command in their `subagent` dispatch directive. The coordinator pastes the preamble verbatim at the top of the subagent prompt and never paraphrases or extends it.
- **Quoted item presets are inert (GH #331).** Item-level preset discovery masks balanced inline-code spans before tokenization, so evidence such as ``do not run `#upgrade` here`` or `` `- ~~#upgrade~~ — auto-struck` `` cannot grant that preset's operator authorization. The same shared resolver feeds `queue brief`, response-evidence closeout, the pre-write gate, mid-turn steering, and skip diagnostics. Presets written outside inline code still resolve; multiple item presets are emitted in first textual occurrence order and repeated references are deduplicated.
- **Subagent launch policy (GH #328).** Each `queue_subagent_dispatch` entry optionally carries `model` and `effort`; absent fields preserve harness inheritance. Resolution is per field: selected queue `model=` / `effort=`, then applicable structured prompt-preset metadata (`model:` / `effort:`, with an item preset preferred over queue presets), then document frontmatter `subagent_model` / `subagent_effort`. `queue brief` includes the resolved values and explicitly tells the coordinator to pass them to Claude Code Agent `model` / `effort` or Codex `spawn_agent` `model` / `reasoning_effort`. `queue claim` records the resolved pair in the attempt's `dispatch_policy`, and `queue start` preserves it with the concrete worker binding, so later cost analysis can attribute the worker to the dispatch policy that launched it.
- **Queue schedule status (`agent-doc queue schedule <FILE> [--json]`, `#queueschedule`).** Read-only report of every scheduled queue (`interval=`, `time=`, `loop` on a queue marker, see `02-document-format.md` §2.5 Queue schedule). Each entry is identified by its canonical `queue_id` and reports cadence, whether a pass is running and which, passes completed against the `loop` limit, next run and last tick, how many heads its closed window holds, backoff, and any pending `loop` restore. A schedule the project controller has not admitted yet reports `pending_admission: true` ("waiting for the project controller"). An invalid declaration has no runnable status, reports its parse errors, and holds only that queue fail-closed. `--json` prints `{ "scheduled": "true", "queues": [...] }` (or `{ "scheduled": "false" }`); array order is document queue order and every entry includes `queue_id`, so multiple schedules are never conflated. For one valid schedule, the original flat status and `held_heads` fields are also emitted at the root for single-queue client compatibility. The command never ticks, opens a window, or writes; the controller's single scheduler loop owns every transition. Scheduler ops-log lines identify `queue_id`: `queue_schedule_admitted`, `queue_schedule_invalid`, `queue_schedule_tick decision=opened|recovered|empty_poll|skipped_paused|skipped_busy|exhausted ...`, `queue_schedule_pass_completed`, `queue_schedule_loop_restored`, `queue_schedule_loop_restore_failed`, `queue_schedule_tick_failed`, `queue_schedule_removed`.
- **Loop restore completion requires settled, visible delivery (`#looprestoreproof`, GH #330, GH #348).** A roster item merely appearing live in one current-editor observation does not complete a pending restore: retained CRDT delivery can expose that text before a concurrent closeout projection settles. The scheduler plans the restore from the exact authoritative queue cut that proved the pass complete, never from a later pre-strike projection, and passes that same cut as the write compare-and-swap base. Even when every roster item was already live in that cut, the restore path runs the write-convergence guard. After the transport accepts the write it re-reads current document authority and proves every roster identity live; `already_current`, CRDT semantic-no-op, and CRDT exact-no-op receipts are not success while the row remains struck or absent. A retained, unsettled, or post-write-missing result keeps durable `restore_pending` and its backoff retry; only the post-write visible proof may clear it or log `queue_schedule_loop_restored`.
- **Idle loop ticks recover lost restores (GH #335, GH #348).** If a due tick sees a closed, non-held loop with no live head and no pending restore but finds struck rows in that queue, it persists an open next-pass window and `restore_pending` from the struck roster, then runs the normal restore path. While that restore remains pending, realtime observation cannot treat those same old struck rows as evidence that the recovered pass ran or emit `queue_schedule_pass_completed`. The recovered tick is logged from the post-restore state, so success reports the next cadence and failure reports its real backoff deadline; an unset restore backoff must never render Unix epoch as `next_run`. Paused/stopped queues, busy windows, and exhausted `loop=N` schedules retain their existing decisions.
- **Exact queue marker mutation (`agent-doc queue attr <FILE> --id <QUEUE_ID> [--set KEY[=VALUE]]... [--remove KEY]...`, `#queue-id-attr-edit`).** The required `--id` resolves exactly one queue through the canonical multi-queue graph (`default` names a legacy single unkeyed queue). Missing, malformed, or duplicate ids refuse before any write. Attribute keys come from the queue component vocabulary; unknown keys and invalid bare/value shapes refuse instead of landing ignored text. The binary derives the successor from current editor authority while holding the queue-edit lease, validates the complete successor graph and the selected queue's schedule, and submits the exact source/target pair through the existing editor/CRDT compare-and-swap convergence path. A changed authority generation therefore refuses or rebases only at that shared write-policy boundary; the command never overwrites a newer cut. The mutation replaces only the selected opening marker and preserves sibling markers, queue bodies, and all other bytes. Removing an absent attribute and setting an already-equal value are successful no-ops.
- **Inbound queue append (`agent-doc queue add <FILE> --source <NAME> [--key <KEY>] [--attach <PATH>]... [--json] [TEXT]...`, `#queueinbound`, GH #237).** The supported way for an external tool (an editor plugin, a game-engine play-mode bridge, a CI watcher) to hand work to a session. It appends one attributed free-text head to the end of the document's selected `agent:run` component (the block preflight reads queue work from, `queue_graph::selected_component`), immediately before the closing marker: `- [from:<source>] <text>`, or `- [from:<source> key=<key>] <text>` with a key, followed by ` (attachment: <path>)` per attachment. Text comes from the trailing arguments (joined with spaces) or, when none are given, from stdin; every whitespace run, including newlines, collapses to one space so the head is always a single queue line. `--source` and `--key` accept only ASCII letters, digits, and `- _ . : / @`, so the tag carries no `#`, bracket, or whitespace: it never reads as a tracked-work id or preset reference, and the head classifies as an ordinary drainable free-text head (a `#preset` the tool writes in the text still expands as usual). **Write path.** The head is planned against the realtime current document (`try_resolve_current_document_content`) and written through `QueueCommandEffects::converge_document_or_disk`, the binary-owned path `queue sync` mirrors use: while an editor holds the document the change is applied to the live CRDT replica and delivered to the editor (`queue_add_writeback transport=crdt_relay`), never written to disk behind the buffer; operator text typed but not yet saved survives. The insertion is byte-preserving: no existing queue line is re-rendered (`#qauthorder`). The queue-mutation lease (`QueueEditGuard`, `#sqedit-race`) is held across the read, plan, and write. The snapshot is deliberately not checkpointed: an inbound head is new queue input exactly like an operator-typed line, so the next preflight lists it in `queue_prompts`. **Dedupe.** With `--key`, a live (unstruck) queue head whose tag carries the same source and key makes the add a no-op (`outcome: duplicate`, exit 0, document untouched). Without a key, a live head with identical rendered text is the duplicate. A struck (consumed) head never blocks a later add, so a recurring failure re-enters the queue once its earlier head was handled. Dedupe state is the document itself; there is no side ledger. **Merge identity (`#fpeseverityparse`).** Every keyed queue merge (deferred-write composition, retained-target rebase, editor-cut rebase; `agent_doc_merge::crdt::list_item_key`) keys an inbound head by its attribution tag, `from:<source>:key=<key>` (or by its normalized text when unkeyed), never by a `[#id]` its free text names. The head and an operator-typed line for the same tracked-work id (`[from:actionable-review key=stggrafanaowner] do [#stggrafanaowner]` beside `do [#stggrafanaowner]`) are therefore two distinct queue cells: a concurrent operator edit to either can neither drop the inbound head nor splice its bytes into the operator's line. Strike and progress markers do not change the identity, so consuming the head still reconciles as an edit of the same cell. **Redaction.** `agent_doc_secret_redact::redact` runs on the text before it is normalized and written. Each `--attach <PATH>` must be a UTF-8 text file of at most 1 MiB; its redacted copy is written to `<project>/.agent-doc/inbound/<short-content-hash>-<sanitized-name>` (content-addressed, so a retried add reuses the same file and renders the same head) and the head references that project-relative path, so a multi-line log never enters the queue line. **Steering.** A running session sees the head through mid-turn steering (`steering --follow`, the PostToolUse hook, closeout steering) as a queue addition with `dispatch=drain_after_current`: passive queue work that runs in queue order after the current item closes. Subagent intent (`#subagents`, or a queue/preset that requests subagents) upgrades it to `subagent` exactly as for an operator-typed line. Queue activation is not changed; an inactive queue holds the head until it is activated. Ops log: `queue_add outcome=added|duplicate source=<s> key=<k|->`. `--json` prints `{"outcome":"added","head":...,"attachments":[...]}` or `{"outcome":"duplicate","existing":...}`. Resolving the target document from a path (`--for-path <dir>`) is deferred: agent-doc has no project-local path→document mapping yet, so tools pass the document path explicitly.
- **Subagent scratch isolation (`#waypostauthorization`).** Concurrent subagents dispatched by one coordinator share its scratchpad, and a fixed message-file path there let one subagent's commit carry a sibling's message. The `queue brief` preamble therefore restates one more coordinator rule, `SUBAGENT_SCRATCH_ISOLATION_RULE`: a subagent passes its commit message through `git commit -F -` (a stdin heredoc) or a file inside its own worktree, never a file in a shared scratchpad or `/tmp`, and puts any other temp file in its own worktree or a unique `mktemp` path. Exchange subagents get no preamble, so the exchange-subagent dispatch texts (preflight `exchange_subagent_dispatch_guidance`, the mid-turn hook and closeout steering directive) append the same text verbatim from the `subagent_scratch_isolation_rule!` string-literal macro, the single source of truth.
- **Noise-head prune (`agent-doc queue prune-noise <FILE>`, `#goqstall2`).** Strikes every active **predicate-proven non-drainable noise** head (pasted console-output, status/log lines, or agent-response fragments — the same lines `session-check` surfaces as `queue_stale_noise_lines=<M>`) at **any** position in the queue, and also strikes orphan id-backed heads whose ids no longer name open backlog work. Unlike `queue consume`, which strikes only a contiguous LEADING free-text run and stops at the first id-backed head, prune clears artifact noise interleaved **behind** id-backed `do [#id]` heads — the shape `queue consume` and the answered-free-text strike could never reach. Id-backed directive heads naming open backlog work and genuinely drainable free-text/prose heads are preserved, so the prune never desyncs tracked or runnable work and never treats fresh operator prose prompts as stale/noise. Classification matches `queue_stale_noise_lines` exactly, including the `preset="..."` short-circuit; ordinary prose reports are drainable queue work even without an imperative verb, while fenced/log-only, bold-report, or agent-comment artifacts remain noise. It strikes the document and snapshot in sync through the editor-IPC-converged write path (`#fcc0`) — supervisor-safe, no hand-editing of a live queue — and is a no-op when the queue is inactive or nothing is predicate-proven. The caller closes out through the normal commit boundary.
- **Free-text queue heads complete on being answered (`#free-text-queue-head-consume` / `#free-text-queue-owner-consume`).** A head with no id directive — a plain question or instruction typed into `agent:run` — has no `#id` to `--done`, so a non-empty captured response body for that cycle IS its completion signal and the closeout consumes it. The documented deferral shape is the exception (`#deferstrike-closeout`): when the response quotes that head and the quote's scope opens with `**Deferred:**` (or another recognized deferral marker), deferral vetoes every closeout completion signal, including an explicit `> **Queue prompt:**` echo, a matching completion id, or a synthetic response topic; the head remains live and `queue_active` stays set. A committed deferral of an active `go` head is nevertheless a new continuation edge (`#deferredliveness`): the supervisor schedules one clear-or-fresh-restart and redispatch for that cycle, while non-automatic contexts must surface the blocker plus the explicit `agent-doc <FILE>` rerun action. The captured response owns a strike only while its exact closeout cycle is open; after `committed` or `abandoned`, the retained audit payload cannot strike an identical recurring command typed for a later turn. Same-cycle recurring imperatives still strike normally. A head is treated as **id-backed** (and therefore still needs an explicit completion signal, not just an answer) only when the *entire* head resolves to a single id directive — `#id`, `[#id]`, or `do [#id]` — or to a `#preset` / queue-trigger. A free-text head that merely *mentions* a `#id` in prose (for example `Approve [#shoptiers]. What are #next-steps?`) is **not** id-backed: it has no single id to complete, so it stays free text and would otherwise hang the auto-queue forever. Classification keys off whether the head resolves to an exact id directive, not whether it contains a `#id` substring. In particular, the optional-`do` grammar reserves bare `[#id]: note` as inert prose: the immediate colon keeps its durable identity, claims, dispatch, node selection, and answer closeout on the free-text path even when `#id` appears in `agent:todo`. An exact `> **Queue prompt:** [#id]: note` echo completes it, while `queue consume --id` and `--ack-id` refusals must name bare `agent-doc queue consume <FILE>` as the valid path. The explicit `--done <id>` todo-own-id lifecycle remains the documented exception that may atomically check the todo and strike its matching annotated queue row. Explicit `do [#id]: note` remains an annotated id-backed directive. **Registered prompt-preset token heads are free text, not id-backed (`#qpresetstrike`).** A bare queue head that resolves to exactly a `#name` registered in frontmatter `prompt_presets` (and that does *not* also name a tracked `agent:backlog`/`agent:review`/`agent:pending` item) has no backlog row, so `--done <name>` fails ("id not found in backlog/icebox") and the leading `#` previously routed `queue consume` to `--done` and wedged the head as unstrikeable — keeping `queue_active=true` and re-serving every invocation. Such a preset-token head is a synthetic prompt completed by being answered, so it strikes like a free-text head via `queue consume` and the free-text finalize heuristic unless the response defers it. A preset token that *also* names a tracked backlog/review id stays id-backed (the tracked-item reap path wins). A controller `queue_paused` state whose reason matches `"<preset> preset head is spent"` is revalidated on every dispatch: if the preset head is absent from the live queue, dispatch clears the stale pause and proceeds; if the live head is that registered preset token, dispatch first consumes it through the canonical queue consumer, clears the pause, and proceeds. The repair logs `spent_preset_pause_repaired ... action=resume_absent_head|consume_head`, so JB Run Agent Doc never repeats the old manual "clear '- #name'" instruction for a stale spent-preset receipt.
- **A free-text closeout consumes only the head its decision approved (`#qconsumeapprovedhead`, GH #240).** The response closeout decides whether to consume against the document it read before the write (`queue_consumption_allowed_for_response` over the pre-write head), but the consume itself re-reads the live document. Another closeout driver (for example a captured-finalize resume's repair pass, whose `#repairftstrike` strikes answered free-text heads) can strike the answered head between those two reads, so "the leading live head" at consume time is the next, unanswered operator prompt. Consuming it deleted operator work, drained the queue, and cleared `queue: go`. A no-done-id response closeout therefore captures the approved head from the decision document (`approved_queue_head_for_consume`: text with lifecycle/priority markers removed, plus the strike-stable node identity `{item id}:{occurrence}` without the positional component index) and passes it to the consume. The consume takes the leading head only when it is still that exact head; otherwise it is a monotonic no-op logged as `queue_consume_unapproved_head_noop ... #qconsumeapprovedhead`, the other head stays live, and `queue: go` is unchanged. A decision that read no live head (queue inactive or empty) approves nothing, so a head that appeared since is never consumed by that closeout. Node identity separates two heads with identical text (`- go` / `- go`), which a text compare cannot. An id-naming closeout keeps its id match (`#qftnodoneeat`), which already binds the consume to a proven head. This fence sits beside `#qselectedheadonly` (preflight selection) and the `expected_head` stale-head guard; it does not depend on which driver struck first.
- **Every captured-finalize closeout path mutates only under the closeout lease (`#closeoutleaseresume`, GH #240 follow-up).** A durable capture does not own the cycle's closeout lease: a live `respond`/`write` closeout of the same cycle may hold it. The supervisor's resume start gate (`captured_finalize_resume_should_start`) only admits an attempt; the resume worker acquires the lease through the controller actor's closeout-owner CAS (role `captured_finalize_resume`, pinned to the expected cycle) before any document, queue, snapshot, or commit mutation. This covers the legacy repair fallback for a capture without a typed mutation plan, whose already-applied branch strikes answered free-text heads (`#repairftstrike`), advances the snapshot, and commits; that path used to run with no lease, so it overlapped a live closeout and struck the head that closeout had approved. Write-runtime calls nested inside the held lease on the same thread are covered by it and do not claim again. A lease held by another live closeout refuses the attempt before any effect, as `RetryAt` at that lease's deadline; it is never a poll loop. The foreground owner's release publishes a `closeout_owner_released` captured-finalize wake while the cycle is still open, and that wake preempts the deadline. A resume's own release is not a wake for itself. A superseded cycle makes the resume a no-op (`captured_finalize_resume_legacy_repair_skipped ... reason=cycle_superseded`). Without a project actor (an ignored or standalone document), the existing actorless compatibility path applies.
- **Queue auto-clear between items (`#bgclearpolicy`).** An active queue drains each item in a fresh agent context by default. Bare `clear` is the positive declaration and is present in the default template; omission remains enabled for compatibility, while legacy `disable-auto-clear` opts out (preflight reports the effective `queue_auto_clear`). The rule is every completed direct-queue turn; `subagents`/`fan-out` queues defer to the next safe idle boundary after claims settle. It is not an accretion threshold: the next item is dispatched to a freshly-cleared agent, and accretion measures the exchange size, which a clear does not change (only compaction reduces it). The supervisor owns the boundary: preflight (`queue_auto_clear_yield: true`, `queue_continuation_required: false`), session-check (`queue_continuation_required=false queue_auto_clear_yield=true`), the MCP `agent_doc_finalize` / `agent_doc_session_check` results, and the Codex Stop hook all end the in-session drain at a proven closeout when the document's supervisor is drain-ready and owes no earlier hand-off; the supervisor then clears once (Claude Code `/clear` / OpenCode `/new` typed into the live process, Codex fresh restart) and dispatches the next head. With auto-clear off or no ready supervisor the in-session drain continues in the same context. Gates, delivery, and the 2026-06-30 history: [Supervisor](supervisor.md).
- **Free-text execution context is inline (`#ftimmediate`).** A free-text head may carry the same whitespace-delimited `[clean-session]`, `[focused-cycle]`, or `[operator-verify]` token as tracked work. The in-session loop parses that token directly from the queue text: `[focused-cycle]` yields to the supervisor, which dispatches the same head (in a fresh context when queue auto-clear is on); `[operator-verify]` remains deferred from both agent drains; `[clean-session]` stays loop-drainable while retaining its context-reset reason. This reuses the existing execution-context vocabulary without synthesizing backlog ids or durable side records.
- **Pre-materialized undrainable / noise heads self-defer, they do not churn (`#goqstall2`).** `#goqueuestall` defers `[operator-verify]` items at the *backlog→queue sync* boundary; `queue_continuation::detect` applies the same defer to heads *already materialized* in the `## Queue` block. **As of `#qcontdrain`, `[clean-session]` is no longer deferred in either path** — the in-session `/loop` drains clean-session heads in place (a possibly-stalled supervisor previously left them stranded), so only `[operator-verify]` is undrainable. The continuation head walk skips, in addition to a stop fence / future time gate / snapshot-modified head: (a) a `do [#id]` head whose backlog id is deferred (`deferred_backlog_ids`: `[operator-verify]` only; `[clean-session]` drains in-loop now), and (b) a **predicate-proven non-drainable noise** head — structural/log artifacts such as console status lines, fenced log-only blocks, bold response-fragment summaries, or agent comments. Plain operator prose is drainable queue work by default, even when it is phrased as a declarative bug report rather than an imperative. Noise heads are **never auto-deleted** (the live IPC supervisor races on direct queue edits); they are excluded from the continuation head set and counted as `queue_stale_noise_lines` for compatibility. They are cleared on demand through the binary-mediated `agent-doc queue prune-noise <FILE>` command (see below), which strikes only predicate-proven noise/orphan heads through the same editor-IPC-converged write path the closeout strikes use, so the operator never has to hand-edit a live queue. When only deferred/noise heads remain, `session-check` prints `queue_continuation_required=false queue_deferred_heads=<N> queue_stale_noise_lines=<M>` with guidance to drain deferred heads from a clean session and clear predicate-proven noise via `queue prune-noise` — the same self-defer end-state `#goqueuestall` produces for the sync path, so a queue pre-populated with undrainable `do [#id]` heads + pasted evidence stops churning no-op closeouts. **The supervisor idle-queue watch dispatch agrees with this decision (`#qchurn`).** The idle-watch derives its dispatch head from `queue_continuation::live_drainable_continuation_head`, which applies the same drainability + deferred filtering as `detect` (skipping artifact noise and `[operator-verify]` heads; `[clean-session]` is drainable per `#qcontdrain`), instead of the unfiltered `live_continuation_head`. Otherwise the watch would re-inject a no-op `/agent-doc` drain trigger every idle boundary for a queue that `session-check` already reports as needing no continuation — repeated zero-diff cycles with `user_intent_prompt_changes` empty.
- **Legacy stale-supervisor queue pauses recover once (`#jbrestale`).** New controllers tag stale-supervisor `queue_paused` churn bails with `supervisor_restart_redirect`, but route must also recognize the legacy markerless shape from an old route-owned supervisor: `failed_stage=queue_paused reason=#qchurn ... stale host supervisor pid<N> ...`. The classifier also treats `stale route-owned supervisor (pid N) ...` as stale-supervisor churn. If a newer same-document actor transition, a different live supervisor PID, or a named stale PID that is no longer alive and whose pause row predates the current system boot proves the pause predates the current owner, dispatch clears the stale pause before queue blocking. Otherwise both marker shapes use the same one-shot policy: restart the stale supervisor in the non-destructive supervisor path, lift the stale pause, and retry dispatch exactly once. Stale-binary freshness guidance must use idle recycle or normal restart and must not point operators at force/discard recovery unless the owner is genuinely wedged. Deliberate operator pauses and spent-preset pauses still fail closed.
 - If the queue drains, the queue body is cleared, `auto` is removed, and `queue_active` is cleared. When the queue is inactive and all entries are drained residue (completed, preset, or dispatch with no live prompts), the body is cleared regardless of whether the snapshot proves the queue was previously active — completed items are never re-executable, so retaining them is always noise.
- **Already-resolved heads drain, they do not halt (`#drained-done-queue-clear`).** The preflight strike pass converts every leading queue head whose `#id` is already in `agent:done` (or review-gated) into a `Completed` entry. A newly admitted prompt or any currently-open backlog item overrides historical `agent:done` membership (`#qrepeatid`): repeating the same prompt and stable id is executable new work, not completed residue, and the live backlog checkbox preserves that incarnation across maintenance retries until closeout reaps it. When no such reactivation exists and the strike empties the entire live-prompt set — a fully-resolved active queue batch, optionally fronted by a `dispatch #preset` directive — maintenance must re-resolve activation to inactive so the drain-cleanup path clears `queue_active`, strips any legacy `auto`, and empties the body in the same binary-owned pass. It must not classify the now-headless queue as an `item_modified` halt. The cross-cycle head-modified comparison applies the same done/gated strike to the snapshot entries before comparing, so a head resolved this cycle via `--done` does not read as an operator edit while a genuine still-live head edit still halts. A standalone no-diff preflight that drains in this way self-heals its snapshot/HEAD drift through the route queue commit-boundary recovery (which also accepts a drained snapshot when HEAD proves the prior active queue and only queue state differs), instead of stranding the maintenance mutation for a manual `agent-doc commit`.
- The `inactive_queue_residue` warning is a per-*edit* signal, not a per-preflight nag. Preflight emits it only when the inactive queue body actually changed since the snapshot this cycle (the operator added/changed content in an inactive queue, so a `do [#id]` they expect to run silently will not). A stable inactive queue that is unchanged from the committed snapshot — for example the retained live tail an `item_modified` halt leaves behind — is not re-flagged on subsequent edit-free preflights. This stops the recurring residue warning that drove the `#adoc-queue-ipc-drift` loop (root cause #1) while preserving the warning on genuine inactive-queue edits.
- **Live editor-buffer convergence (`#adoc-queue-ipc-buffer-divergence`, root cause #2).** When queue maintenance halts or drains an auto-queue, it writes the corrected queue body, opening-tag `auto` attribute, and `queue:` frontmatter to disk and snapshot. A live route-owned IPC listener keeps its own working buffer; a content-only IPC patch can replace a component body but cannot change the `<!-- agent:run auto -->` opening-tag attribute or the frontmatter, while a tag/frontmatter-only convergence leaves stale queue body lines in the live buffer. Without converging all three, the live buffer can re-assert stale queue lines, `auto`, or `queue: start` on its next flush and regenerate snapshot/HEAD drift every preflight. After each halt/drain disk write, when a listener is active, preflight pushes a dedicated queue-convergence IPC patch (`ipc_socket::send_queue_convergence`) carrying the corrected `queue` component body, desired `queue_auto` opening-tag state, and canonical `queue:` frontmatter. The plugin applies the queue body as a component replacement, applies the frontmatter merge, and converges the queue opening tag through the `agent_doc_converge_queue_auto` FFI seam (a C int for a stable JNA ABI; an absent `queue` component or already-matching tag is a no-op). The push is best-effort — a missing listener or send error is logged, never fatal — because the disk/snapshot write remains the source of truth. A follow-up maintenance pass on the converged document mutates nothing and sends no further convergence (idempotent).
- **Route-owned activation retains ownership through editor delivery.** If queue activation, route session metadata, or route document preparation is accepted into the lazy CRDT delivery projection but is not yet editor-visible, route waits for the exact target projection before allowing the caller to checkpoint or dispatch. A newer converged editor value returns to the existing CRDT re-merge loop. A bounded non-convergence tells the operator to rerun `agent-doc route <FILE>`; it must not prescribe `agent-doc commit`, because no response cycle or owner-pane capture exists yet. When a tracked `/clear` causes a fresh harness restart and that newer actor generation has already entered `busy` through the supervisor's `auto_trigger_inject`, route treats the active turn as accepted owned dispatch and coalesces it rather than reporting that the fresh pane never became ready. Likewise, capability-proof reuse is an idle-pane admission gate: if the authoritative same-document actor is already `busy` in the registered owner pane, or that same pane has harness-specific live active-turn proof while its persisted actor projection still says `ready`, a missing proof coalesces the active dispatch rather than attempting a restart that its open cycle must reject; an explicit failed proof still fails closed.
- **Claim text excludes lifecycle projection (GH #304).** `queue claim` resolves both disk and live editor-authority heads through projection-free text. Binary-owned lifecycle prefixes (`📥`, `🟡`, `🚧`, `☑️`, legacy `📫` / `📤`, `⚠️`, `🔄`, `❔`, `⏭️`, `⏳`), the operator-choice marker (`🙋`), request-id suffixes and local answers (`[#request-id]: <answer>`), and owner/generation annotations never enter `QueueClaim.item_text`, claim anchors, CLI output, or free-text start/release handles; operator-authored priority pins remain intact.

- **Torn duplicate owner projection repairs from positive evidence (`#queueownerrepair`).** If editor convergence leaves a valid exact-attempt owner annotation followed by the same owner/generation with its closing `]` torn away immediately before a lifecycle marker (for example `[#owner#g] [#owner#g📥 prompt`), queue normalization removes that duplicate fragment before identity extraction and matching, including the projection-safety guard's before/after identity comparison, then reprojects the canonical row from durable state. Repair requires either the preceding valid annotation or the exact durable target owner/generation; an unproved lookalike remains operator text.

- **Lost-claim diagnostics are actionable (`#queueclaim`).** `agent-doc queue claims <FILE>` lists every durable attempt whose queue row is still live, including retained `Lost` attempts. A lost row carries `lost_recovery.reason = claim_ttl_expired`, tells the coordinator to recover and integrate a late completed worker result when one exists, and prints the exact reclaim command for creating the next generation otherwise. A visible `⚠️` row must never require guessing why ownership ended or what action is next.

- **Claim freshness diagnostics distinguish durable dispatch from progress (`#claimfreshnessdiagnostics`).** `no_changes_explanation.claimed_queue_items` reports the attempt generation, exact durable state (`claimed`, `running`, `reported`, or `lost`), worker binding/start time when present, owner-session state, and expiry. Its guidance treats `claimed` as a dispatch reservation, `running` as positive acknowledgement that a concrete worker was bound, and `reported` as coordinator integration debt. `queue claims` additionally reports a stable `assessment`, an operator-facing `assessment_detail`, `claim_age_secs`, `ttl_remaining_secs`, and `worker_started_age_secs` when a worker is bound. These fields describe only evidence agent-doc owns. They MUST NOT turn an old `claimed_at_secs`, a `running` marker, or a live coordinator session into a worker heartbeat or proof of current progress in an external job: agent-doc cannot observe those facts. A coordinator waiting on CI, publication, or another external system therefore keeps the claim while naming that wait; age alone does not make the claim stale. Wherever older prose calls a non-ended claim "in flight", that phrase means fenced from duplicate dispatch, not observably making progress.

## Durable queue worktree lineage

- **The ledger is authoritative (`agent-doc queue lineage|lineage-record`, `#queue-lineage`).** Queue Markdown and live claims are projections, and a worktree may be removed after collection, so neither is the provenance authority. Each document has a separate `queue_lineage_v1` row in `state.db`, keyed by exact queue work identity and monotonic claim generation. `queue claim` creates the attempt receipt and consults its generation floor; `queue start` attaches the concrete worker. Thus a struck or operator-removed row re-added with identical text cannot reuse an old generation, and a stale completion/source receipt cannot attach to the replacement. Identity aliases preserve an attempt across a claim-followed line edit without merging generations.
- **Receipts are explicit and monotonic.** The append-only lifecycle is `created`, `attached`, `dirty`, `committed`, `pushed`, `integrated`, `released`, `cancelled`, `retained`, and `removed`. Coordinators append evidence with `agent-doc queue lineage-record <FILE> --item <ID_OR_TEXT> --generation N --event <EVENT> ...`; `integrated` always requires an explicit `--source-sha`, `--integration-sha`, and `--method cherry-pick|rebase|merge|fast-forward|other`. An ancestry or patch-equivalence check validates that explicit edge when possible but never invents it. Commit objects, base ancestry, pushed refs, cherry-pick/rebase patch equivalence, fast-forward/merge ancestry, and tagged/published release tags are verified before their receipts are accepted. Built-in queue claim/start automatically records the exact attempt and worker. A standalone `parallel`/orchestrate worktree or job result has no queue identity/generation in its `WorktreeInfo`, so its coordinator must append the attached/result receipts explicitly; branch names or task text must never be used to guess that join.
- **History survives presentation cleanup without making the routine workflow noisy.** Claim/start/report/integration/release transitions record lineage without emitting extra lineage chatter or writing a Markdown projection. `agent-doc queue lineage <FILE> [--item ...] [--generation N] [--verbose|--json]` retains and reports historical closed attempts, then rechecks live Git facts. Default text is exactly one compact row per attempt; `--verbose` is the explicit human-readable evidence/diagnostic view, and `--json` is the machine-readable view. A deleted worktree or unavailable ref is a degraded diagnostic; branch/ref/tag or ancestry drift is a mismatch. The compact row reports the diagnostic count and points to `--verbose`; neither diagnosis invalidates nor deletes the receipt. Cancellation terminalizes only the exact generation; only retained/removed cleanup receipts may follow. Removing an already struck queue row is presentation-only and never deletes lineage. Markdown may project a compact `[lineage:gN src:<sha> int:<sha> release:<tag>]` hint only on explicit request, but that hint is never authoritative and must not be used to infer integration.
