# Planning / Dispatch

Use this runbook immediately after `agent-doc preflight <FILE>`.

## Goal

Turn the diff/preflight signals into an explicit execution contract before repo work starts.

## Command

Run:

```bash
agent-doc plan <FILE>
```

The command emits a structured planning record as JSON.

## Planning Record

- `prompt_targets` — ordered prompts that must be answered this cycle
- `execution_scope` — `normal | plan_backlog_only`; `plan_backlog_only` means the active prompt is a report/planning contract such as `#agent-doc-bug`, so the cycle must create plan/backlog artifacts and must not start repo implementation yet
- `repo_actions` — concrete repo work to complete before response persistence
- `required_commands` — binary/harness commands that must run this cycle
- `pending_mutations` — legacy-named planning field for backlog/review tracked-work mutations that must be resolved before persistence
- `operation_tree` — binary-owned `selected_path_only` tree for the blocker gate and ordered executable phases; follow this path instead of proposing speculative alternatives
- `handoff` — `none | orchestrate | compact | claim | other`
- `blockers` — concrete blockers that mean the cycle should fail closed

## Dispatch Rules

1. Run the planning phase after preflight and before repo work.
2. Execute `required_commands` and respect `handoff` before free-form response generation.
   Use `operation_tree` as the normative ordering: it contains only the selected
   branch, so do not spend context enumerating or exploring paths absent from it.
3. If `handoff=orchestrate`, use the emitted `agent-doc orchestrate ...` command instead of manually simulating the batch.
4. If `handoff=compact`, follow the emitted compact/restart instruction and stop before normal repo work or response finalization. `handoff=compact` requires an explicit prompt or document compaction request; advisory session-accretion signals such as repeated no-op closeout churn must not force this handoff.
5. If `execution_scope=plan_backlog_only`, do not start repo implementation, tests, builds, installs, commits, or pushes from this cycle even if the raw prompt contained imperative wording. Capture the plan and backlog items first, then wait for a later `do #id ...` turn to authorize implementation.
6. Otherwise, execute `repo_actions` before `finalize` / `write --commit`.
7. Resolve `pending_mutations` in the same cycle so backlog/review tracked-work state does not drift.
8. Under `<!-- agent:run subagents -->` (alias `fan-out`, `=N` cap), dispatch the cycle contract's `queue_subagent_dispatch` items instead of draining them. The list covers every `subagents` queue whose `after=` predecessors are empty, including a later queue whose sibling is still claimed, not only the claim-blind selected queue. An earlier inline queue that still has owner-available work keeps later queues off the list. A paused earlier queue does not: `pause` (a legacy marker `stop` included) still leaves the other runs in that turn. Hook-driven entrypoints receive the list from preflight; connected entrypoints receive the equivalent list from `agent_doc_plan`, including when the document diff is empty. Claim, spawn, and start every independent entry in that list in the same dispatch pass before waiting or integrating; sharing a repository or write scope affects serial integration, never worker start order. Subagent heads always start in parallel (`#subagentsalwaysparallel`): `parallelizable` is true exactly when subagent dispatch is enabled, regardless of head count, shared repo, or write scope; only `after=` (per-item ordering: the dependent is held out of the list) serializes them. For each item, finish its `claim_command` successfully and confirm the durable Claimed/🟡 projection before invoking the native spawn tool; if claim fails, do not spawn. Then give it one background subagent in its own worktree outside the IDE-watched project, with the entry's `authorization.subagent_prompt_preamble` pasted verbatim at the top of its prompt (`#waypostauthorization`; `agent-doc queue brief <FILE> --item <...>` re-prints it). Run `start_command` with the returned worker identity as the immediate next action, before other coordinator work, so the marker advances to Running/🚧. Keep the main agent responsive while workers run: it owns operator conversation, steering delivery, durable claim/document reconciliation, serial integration, verification, authorized coordinator-owned effects, and closeout; a worker never self-claims or self-closes the session queue. That preamble also carries the `[#subagent-operator-request-<unique-id>]` blocking-question/permission handback protocol. A request packet is not completion: preserve its exact request, context, and choices in `agent:exchange`, ask from the main session, record the concrete operator action with `--todo-add`, and return the answer to the same worker without treating chat text as a permission grant; check that TODO with `--todo-done` when the answer and worker result complete it. Integrate their work serially; that includes heads with the same repo `write_scope`. Before closeout, compare each handback with its item and resolved preset actions: a prepared artifact is not completion while an authorized post/publish/push/message/API effect remains, and queue auto-clear never preempts that effect (`#effectbeforecloseout`). `parallelizable: false` only means the work has no subagent intent and drains inline one item at a time. Grok Build uses its native `spawn_subagent` tool with `background=true`, once per dispatch item, and passes each returned `subagent_id` to that item's `start_command`; it does not wait for another operator prompt. If `queue_dispatch_acknowledgement_required` is non-empty, a prior claim never reached the worker-bound state: run the entry's exact `start_command` with the returned worker id when spawn succeeded, or its `release_command` to requeue when it did not. Harnesses that genuinely lack an in-session subagent tool fall back to `agent-doc orchestrate <FILE> --mode parallel` over those items under the same claims. Before any successful terminal boundary, reconcile the full document-wide queue: eligible pending, claimed-but-unstarted, lost, and reported rows require coordinator action; only running, explicitly operator-gated, or terminal rows permit a yield, and running/blocked rows are not document completion. Queues without the `subagents` attribute stay coordinator-inline: the main agent executes those rows itself and owns their lifecycle reconciliation; it must not native-spawn them. The claim, `--refresh` heartbeat, request-handback harness matrix, integration, single `make install`, and release-then-close order is in `respond.md` (`#queuesubagents`).
9. If `blockers` is non-empty, surface the blocker and stop instead of freelancing around it.
   A turn the binary's preflight already sealed never reports the `No changes` blocker: plan reads the open cycle, recovers the free-text queue head preflight selected, and otherwise returns `task_class: preflight_sealed` with no blocker — execute the preflight contract (`#plansealedturn`, GH #68 §1).

## Notes

- The current implementation is intentionally deterministic: it reuses the same binary-owned diff classifiers that preflight already depends on.
- The point of the phase is the contract boundary, not hidden model improvisation.
- If the schema needs richer semantics later, keep the same record shape so the skill contract stays stable.
