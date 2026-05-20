# Flow Map

This reference records the first FlowCore ownership map. It is an inventory,
not a behavior change plan: command modules still execute the current hot path,
while `src/flow` provides typed state, pure decisions, and mirror-mode
operational events.

## Owning Flows

| Flow | Owns | Current hot-path modules |
|---|---|---|
| `session_cycle` | Direct invocation lifecycle: preflight, plan, execution, patchback, commit, session-check | `preflight.rs`, `plan.rs`, `run.rs`, `write.rs`, `git.rs`, `session_check.rs` |
| `routed_reopen` | Actor binding, readiness barrier, dispatch authorization, submit, dispatch proof | `route.rs`, `start.rs`, `project_controller.rs`, `session_actor.rs`, `supervisor/*` |
| `closeout` | Terminal response durability, pre-write/pre-commit guards, retryable repair boundaries, and commit/session-check guard | `write.rs`, `git.rs`, `repair.rs`, `session_check.rs`, `cycle_state.rs` |
| `document_mutation` | Patchback shape parsing, component mutation, prompt ownership normalization, duplicate repair, and late fallback mutation refusal | `write.rs`, `template.rs`, `merge.rs`, `pending.rs`, `repair.rs` |
| `operator_clear` | Clear Session Context and interrupt-clear guard decisions | `session_cmd.rs`, editor clear actions, `route.rs` readiness helpers |
| `orchestration_batch` | Queue freeze, child dispatch, child patchback normalization, batch stop/resume | `orchestrate.rs`, `queue.rs`, `queue_dispatch.rs`, `plan.rs` |

## Duplicated State Checks

The recurring bug classes map to a single proposed owner:

| Bug class | Current duplication | Flow owner |
|---|---|---|
| Starting/busy actor dispatch into an unready pane | `route.rs` readiness loops, controller actor state, supervisor runtime probes | `routed_reopen` |
| Accepted-only dispatch proof described as submitted/consumed | route log strings, ops summary classification, harness-specific proof checks | `routed_reopen` |
| Late IPC/file fallback after committed cycle | `write.rs` terminal checks, patch sidecars, `cycle_state.rs`, `git.rs` no-op closeout | `closeout` |
| Malformed or plain template patchback escaping `agent:exchange` | `write.rs`, `orchestrate.rs`, `repair.rs`, template parser callers | `document_mutation` |
| Prompt prefix or duplicate prompt repair before snapshot trust | Final template reconciliation, IPC socket/file paths, fallback merge, post-commit repair, session-check | `document_mutation` |
| Pending capture or done guard ambiguity | `write.rs`, `session_check.rs`, `plan.rs`, `pending.rs` | `session_cycle` |
| Clear Session Context racing active panes | editor actions, `session_cmd.rs`, route readiness classification | `operator_clear` |
| Queue item mutation between children | `queue.rs` halted-state detection, `orchestrate.rs` child loop, child finalize | `orchestration_batch` |

## Stringly Typed Proof Fields To Promote

These fields currently cross modules as strings and should converge on FlowCore
enums as extraction proceeds:

| Field | Proposed enum |
|---|---|
| `proof`, `proof_scope` | `DispatchProof` |
| routed reopen prompt-ready / dispatch-start failure reasons | `RoutedReopenGuardReason` |
| `actor_state`, `runtime_state`, `supervisor_health` | routed reopen actor facts |
| `drift_kind`, `basis`, `reason=already_current`, pre-write/pre-commit guard names | `CloseoutGuardReason` and closeout terminal-state reason |
| `markers`, `patches`, `exchange_patches`, `unmatched_len` | `PatchbackShape` and document-mutation parse event |
| `source`, `write_mode`, `patch_id`, `cycle_id` | `DocumentMutationKind` and closeout ids |
| `queue_halted`, `item_modified`, child task labels | orchestration batch outcome |
| protected/busy/idle clear states | operator clear input state |

## Mirror-Mode Event Contract

`ops.log` may now contain `flow_event` records:

```text
flow_event file=<path> flow=<flow> stage=<stage> outcome=<outcome> reason=<token>
```

`agent-doc ops summary` groups these events by flow stage and outcome. It keeps
named buckets for common failures, and falls back to a generic
`flow <flow> <stage> <outcome>` bucket for newly added typed events so a
FlowCore emitter cannot disappear into an undifferentiated tactical-log bucket.
The mirror-mode emitters now cover routed reopen prompt-ready and dispatch-proof
failures, document-mutation patchback parse and visible-write guard outcomes,
strict closeout guard blocks, committed-cycle late-fallback rejection, repair
recovery boundaries, orchestration child patchback normalization, operator clear
guards, and closeout commit completion. Later phases should replace tactical log
parsing with flow events rather than adding more free-form log strings.

## Regression Gate

The hot-path regression gate has two parts:

- Routed-reopen prompt-ready and dispatch-proof failures use
  `RoutedReopenGuardReason`, so `route.rs` cannot pass arbitrary failure reason
  strings into FlowCore events.
- `tests/test_cli.rs::flowcore_hot_path_guard_and_proof_tokens_are_budgeted`
  budgets existing `guard_`, `proof=`, `proof_scope=`, `reason=`,
  `flow_reason=`, and `accepted_only` tokens in route/write/preflight/
  session-check/orchestrate/git/repair files. A budget change is the audit
  point: new tactical guard/proof tokens must first move into the owning FlowCore
  enum/event, or the test expectation must be updated with that audit complete.

## Phase Boundaries

Phase 1 adds the vocabulary, pure decision helpers, mirror-mode events, and
ops-summary grouping. The routed-reopen extraction has started: delivery mode,
dispatch-start proof, degraded-authority refusal, runtime guard, event
construction, and prompt-ready-barrier classifiers now live in
`flow::routed_reopen`, while `route.rs` keeps tmux, supervisor IPC, and
controller I/O. Route, closeout, document mutation, operator clear, and
orchestration extraction should continue by moving one pure decision at a time
behind the new types, then deleting the corresponding tactical branch from the
legacy module after equivalent deterministic coverage exists.
