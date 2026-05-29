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
  - `model_tier`
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
  `write_scope`, and `required_proof` come from prompt targets, pending text,
  requested presets, and visible repo-work directives.
- `tsift_context` records the stable JSON handoff commands used by job packets:
  `tsift status --json`, `tsift context-pack <FILE> --json --budget normal`,
  `tsift source-read <handle>`, `tsift diff-digest --cached`, and
  `tsift test-digest --input`. Missing, stale, or failing context is advisory at
  the agent-doc turn boundary: continue without tsift graph/context evidence,
  but keep the diagnostic visible to workers and parent review.
- Copied `prompt_presets` frontmatter defines reusable prompts but does not invoke them; plan/preflight must ignore preset definition lines when expanding preset references from added diff text.
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

### Command classification

- Queue/orchestration items starting with `/` are commands; everything else is a prompt.
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
5. Persist the final response through `agent-doc finalize <FILE> --baseline-file ...`.
6. Run `agent-doc session-check <FILE>` and stop on the first failure.

Additional rules:

- Sequential orchestration requires normal git-backed finalize; `--no-git` is invalid.
- CRDT streaming is provisional only until the final `finalize -> session-check` closeout succeeds.
- While a CRDT streaming step is still generating, partial output is checkpointed through the shared partial-response capture ledger so pane death does not discard all in-progress assistant text.
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
- Every ready node still runs through the normal single-document lifecycle: inject prompt -> `preflight` -> fresh agent request -> `finalize` -> `session-check`.
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
  per distinct target in the union of `resolve_existing` pending mutations and
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
- Batch-level preset directives may use `preset <name>` lines in the queue body. The queue parser also accepts `dispatch <name>` as a preserved batch directive so older queued batches can close out without parse failure.
- Multi-line prompts use `~~~prompt ... ~~~` or bare `--- ... ---` fences.
- Control fences:
  - `--- start` / `~~~start`
  - `--- start at <time>` / `--- start <time>`
  - `--- stop` / `~~~stop`

### Data model

- Queue entries are parsed as `Prompt`, `Completed`, `Preset`, `StartFence`, or `StopFence`.
- Activation resolution considers `auto`, inline start fences, exchange-triggered `do queue` / `run queue`, and persisted `queue_active`.

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
- halt when the current head prompt was edited since the snapshot for a queue that was already active
- snapshot a newly activated queue body as the closeout baseline instead of treating the changed head as an edit to in-flight work

### Post-commit queue consumption

- After a successful response closeout, required closeouts mark the first prompt complete in the queue in the same locked read/parse/write cycle. If a manual closeout resolves multiple contiguous queued `do #id` prompts with repeated `--done <id>` flags, it may consume that contiguous done-backed head batch in one closeout; it must stop before the first unresolved queued prompt.
- Completed prompts remain visible as `- ~prompt text~` while later prompts remain queued.
- The consumed prompt range must be completable or drainable from both the live document and the snapshot in a provably identical way; otherwise strict closeouts fail before commit.
- Direct `agent-doc run` / bare-path invocations with `queue_active: true` and no document diff synthesize the active queue head as the prompt diff. For `agent:queue auto`, successful closeout consumes one prompt, commits that item, then continues from a fresh run cycle while the next live queue head is still eligible. Continuation stops without consuming the next prompt when the queue drains, a stop fence or time gate reaches the next live head, the next head changed since the snapshot, or a closeout/session-check/verification step fails; diagnostics name the completed item count, next prompt when known, and stop reason.
- Codex harness-native `agent-doc <FILE>` turns rely on the installed Stop hook for the same auto-queue liveness. After a clean closeout, if the document still has `queue_active: true`, `agent:queue auto`, and a ready next head prompt, `codex-stop` blocks the final answer and instructs the agent to invoke `agent-doc <FILE>` again in the same turn. The hook records the requested head prompt and fails closed if a repeated Stop hook arrives without the head advancing, so auto continuation cannot spin forever on an unchanged queue item.
- **Queue-head consumption requires an explicit completion signal for `do [#id]` heads (`#queue-strike-on-halt`), with an exact-id exception for synthetic/preset heads (`#queue-head-consume-on-topic-id-regression`).** A `### Re:` heading that merely names a `do [#id]` queue head is *not* a completion signal — a halt/refusal response names the head precisely to explain why it is *not* being done. The CLI `finalize` / `write --commit` path consumes a `do [#id]` head only when the cycle (a) carries a closeout flag — `--done <id>`, `--pending-gate <id>`, or `--pending-edit "<id>=…"` — that names the head id, or (b) the head is a genuine fresh operator prompt-target / `do queue` trigger in the cycle diff. For a **synthetic/preset head** — a natural-language prompt carrying a trailing `#preset` id rather than a bare `do [#id]` directive — it additionally consumes (c) when the response heading topic resolves to **exactly** the head id, for example `### Re: #spec-test-build-install-commit-push` completing a synthetic head whose preset id is `#spec-test-build-install-commit-push`. A heading topic that merely contains the id with trailing modifiers (`### Re: #id halt` / `### Re: #id deferred`) never counts, for either head shape. The Codex Stop-hook auto-close path (which has no closeout CLI flags) consumes from a heading on an **exact** topic match (`### Re: do [#id]`) or a topic that resolves to **exactly** the head id (`### Re: #id` for head `do #id`), never on a heading with trailing modifiers.
- If the queue drains, the queue body is cleared, `auto` is removed, and `queue_active` is cleared.
- The `inactive_queue_residue` warning is a per-*edit* signal, not a per-preflight nag. Preflight emits it only when the inactive queue body actually changed since the snapshot this cycle (the operator added/changed content in an inactive queue, so a `do [#id]` they expect to run silently will not). A stable inactive queue that is unchanged from the committed snapshot — for example the retained live tail an `item_modified` halt leaves behind — is not re-flagged on subsequent edit-free preflights. This stops the recurring residue warning that drove the `#adoc-queue-ipc-drift` loop (root cause #1) while preserving the warning on genuine inactive-queue edits.
- **Live editor-buffer convergence (`#adoc-queue-ipc-buffer-divergence`, root cause #2).** When queue maintenance halts or drains an auto-queue, it writes the corrected queue body, opening-tag `auto` attribute, and `queue_active:` frontmatter to disk and snapshot. A live route-owned IPC listener keeps its own working buffer; a content-only IPC patch replaces only a component body and cannot change the `<!-- agent:queue auto -->` opening-tag attribute or the frontmatter, so without convergence the live buffer re-asserts `auto` / `queue_active: true` on its next flush and the snapshot/HEAD drift regenerates on every preflight. After each halt/drain disk write, when a listener is active, preflight pushes a dedicated queue-convergence IPC patch (`ipc_socket::send_queue_convergence`) carrying the desired `queue_auto` opening-tag state plus the `queue_active` frontmatter. The plugin applies the frontmatter merge and converges the queue opening tag through the `agent_doc_converge_queue_auto` FFI seam (a C int for a stable JNA ABI; an absent `queue` component or already-matching tag is a no-op). The push is best-effort — a missing listener or send error is logged, never fatal — because the disk/snapshot write remains the source of truth. A follow-up maintenance pass on the converged document mutates nothing and sends no further convergence (idempotent).
