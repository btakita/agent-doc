---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
pending_done_guard: warn
agent_doc_lint_dialect: off
prompt_presets:
  '#advance-review': Go through the review items. Add actionable backlog + queue items to complete or advance the review items.,
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix.
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#simworld': Strengthen Simworld tests.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: start
---

## Status

<!-- agent:status patch=replace -->

done
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260624-051651.md`*

Compacted content:
❯ - Archived 4 response topic(s): #advr11; #advr12; #advr13; 1 more
❯ - Prior summary/context: ❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac...

### Re: #advr15 — gpt-5

Converted the recycle-boundary `Run Agent Doc` verification into deterministic proof coverage for `#jbdisprecycle`.

The runtime R1/R2/R3 fixes were already shipped (`ad55b993`, `d21079b7`): dispatch gates before pane input while `recycle_inflight` is set, `start_session` retries while the recycle marker is fresh, and `route/dispatch.rs` logs `route_dispatch_submit_recycle_settle` before the post-settle submit retry.

I added the missing deterministic proof layer in SimWorld:
- The modeled post-recycle dispatch now records `route_dispatch_submit_recycle_settle ... action=submit_once_after_settle` before rejoining the normal dispatch path.
- Dispatch proof now records `dispatch_start_proof ... proof=submitted` when a pending dispatch is proved.
- `route_sim_dispatch_defers_during_recycle_then_injects_once_after_settle` now asserts: no injection while recycling, exactly one post-settle `dispatch_inject attempt=1`, exactly one recycle-settle submit marker, exactly one `dispatch_start_proof`, no `attempt=2`, and no stranded `start_session failed` text.

Verification passed:
- `cargo fmt`
- `cargo test recycle -- --test-threads=2`
- `cargo test -p agent-doc --test test_cli flowcore_hot_path_guard_and_proof_tokens_are_budgeted -- --test-threads=2`
- `make check`
- `git -C src/agent-doc diff --check`
- `gh run list --workflow CI --limit 1` (latest CI success: `28074608395`)

Commits: submodule `896563a9 test(sim): prove recycle-boundary dispatch submit`; parent `ec9d1f2d3 chore: advance agent-doc submodule`.

### Re: #advr16 — gpt-5

> **Queue prompt:**
>
> :pushpin: do [#advr16]


Completed #advr16. The focused-cycle supervisor path now has deterministic proof for the full handoff: `session-check` logs `session_check_supervisor_drain_handoff` with `ui_outcome=deferred_for_supervisor_drain` / `next_action=yield_to_supervisor_clear_and_continue`, idle-watch logs a focused-cycle-specific `#qfocsup` context reset instead of the clean-session reason, the following `idle_queue_watch_drain` carries `proof=go_drain_dispatch`, and SimWorld proves the stale-supervisor recycle, clear settle, go-drain dispatch, and fresh-agent response-materialization sequence.

Changed submodule commit: `ad8709344 test(queue): prove focused-cycle supervisor drain`; parent gitlink commit: `920e94087 chore: advance agent-doc submodule`. Verification passed: targeted focused-cycle/session-check/idle-watch tests, `make check`, `git -C src/agent-doc diff --check`, latest GitHub Actions CI run `28074608395` success. I also ran `make install`; `lib-install` marked one controller for recycle at the next idle boundary.

### Re: #advr17 — gpt-5

> **Queue prompt:**
>
> :pushpin: do [#advr17]


Closed #advr17 as already satisfied by the shipped `#fbwire` / `#fullboundary` implementation. The acceptance pieces are present in `85b173fb` and `2367d883`: the supervisor idle-watch runs `convergence_gate_decision` before inter-item dispatch, slow ACK keeps deferring while `inflight > 0` so the next head cannot pile up, wedged no-ACK crosses the bounded timeout into `ForceDiskFallback`, and the fallback emits `convergence_gate_force_disk_fallback severity=error` with a persisted loadable playback artifact.

Fresh verification passed: `cargo test -p agent-doc-orchestration convergence_gate_slow_ack_drain_never_dispatches_while_inflight_positive -- --test-threads=2`, `cargo test -p agent-doc-orchestration convergence_gate_wedged_editor_trips_force_disk_after_timeout -- --test-threads=2`, `cargo test -p agent-doc-orchestration record_convergence_force_disk_fallback_emits_loud_error_and_loadable_playback -- --test-threads=2`, `make tmux-ci`, and the previously-run `make check`. Latest GitHub Actions CI run `28074608395` is successful. No additional code changes were needed for this item.

### Re: #advr18 — gpt-5

> **Queue prompt:**
>
> :pushpin: do [#advr18]


Completed the op-capture verification work for `#qnodemerge4verify` and installed the updated local binary/library.

What changed:
- `agent_doc_record_editor_op` now records byte evidence in `ops.log`: `delete_len`, `insert_bytes`, and `insert_non_ascii`, without logging inserted text.
- `merge_contents_crdt_with_ops` now logs durable merge-consumer proof when captured ops are actually accepted: `editor_ops_for_base accepted=true ... offsets=... delete_bytes=... insert_bytes=... insert_non_ascii=... #qnodemerge4wire`.
- Added `agent-doc verify-op-capture <FILE> [--expect-cafe-demo]`, which fails closed unless the target document has both producer-side `editor_op_recorded` and merge-side `editor_ops_for_base accepted=true` evidence. With `--expect-cafe-demo`, it verifies the canonical `café 日本 😀` byte contract: `offset=6`, `delete_len=6`, and non-ASCII insert evidence.
- Added deterministic regressions for the FFI non-ASCII sidecar path and merge acceptance path, replacing `日本` with `世界` at UTF-8 byte offset `6` and asserting no mojibake.

Verification passed:
- `cargo fmt`
- `cargo test record_editor_op_ffi_preserves_non_ascii_byte_offsets -- --test-threads=2`
- `cargo test -p agent-doc-orchestration merge_with_ops_logs_accepted_non_ascii_byte_offsets -- --test-threads=2`
- `cargo test op_capture_verify::tests -- --test-threads=2`
- existing op-capture wiring tests
- `npm test` in `editors/vscode` (includes existing `café 日本` and emoji UTF-16 to UTF-8 tests)
- `GRADLE_USER_HOME=/tmp/agent-doc-gradle ./gradlew test buildPlugin` in `editors/jetbrains`
- `make check` (clippy, 5068 tests, doctests, medium SimWorld corpus)
- `git diff --check`
- latest GitHub Actions CI run is green: `28074608395`

Commits:
- `57ecf388 qnodemerge4: add op-capture verifier` in `src/agent-doc`
- `99bc090fb chore: advance agent-doc submodule` in the parent repo

For the operator-side IDE replay, the installed checker is now the proof command after the live edit/merge: `agent-doc verify-op-capture <FILE> --expect-cafe-demo`.
<!-- agent:boundary:344da539 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" priority go -->
- ~~:pushpin: do [#advr18]~~
- :pushpin: do [#advr19]
- :pushpin: JB `Run Agent Doc` still does not quickly submit the prompt.
<!-- /agent:queue -->
<!--
- 🧪 test edit for [#qnodemerge4wire]
---
Please make backlog replacements for all gated review items. We need to complete the plan, not have any more gates after this round. Let me know when I need to install JB plugin updates. Make sure all JB live gate items have adequate logging to prove/disprove the item. Clear the review items. I will test, files bugs, and you will check the logs.
---
`'/clear' is disabled while a task is in progress
---~~~~
A recent monsterrodholders.md session had much churn after I edited the queue while a turn was running. One churn item included deleting a queue item that I added
---
agent-doc reentrant run on equityfundingsource.md had recovery churn.
```
● The recovery is complete and fully consistent. The binary converged the now-drained queue (empty block, queue_active:
  false), no_changes: true, both responses are committed in the session doc, and both code deliverables are on
  origin/main. The auto-loop terminated naturally.

  Let me record the specific recovery path for this failure mode, since it's a recurring dogfooding pattern
```
---
The last run had a code fence stripped away by agent-doc.
#spec-test-build-install-commit-push
---
What are #next-steps to fix bugs?
-->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#advr19] Advance original #cyh0: identify and instrument remaining File Cache Conflict trigger by capturing exact action, agent-doc command, and ops marker such as `write_authority action=... transport=disk`; distinguish ack-content, VCS-refresh/save, and editor-IPC convergence surfaces.
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
<!-- /agent:review -->

<!-- agent:icebox -->
- [ ] [#qloop1] #agent-doc-feature Define durable queue loop annotation syntax and parser/rendering behavior: mark loop-only queue/backlog entries, keep annotation/state out of prompt completion proof, and expose a stable current-item marker for editor/supervisor surfaces.
- [ ] [#qloop2] #agent-doc-feature Implement idle-loop scheduling for annotated loop items: when normal drainable queue work is empty, dispatch loop items in deterministic sequence, persist the loop cursor/last-run state, and prove regular queue items always preempt loop work.
- [ ] [#qloop3] #agent-doc-feature Add loop poll controls: global default poll period plus per-loop override, next-run time gates, bounded backoff/jitter, and tests proving no tight spin when there are no relevant events.
- [ ] [#qloop4] #agent-doc-feature Add event-aware loop scheduling: record deterministic event counters/timestamps and let loop items schedule earlier/later based on event frequency before any model-assisted scheduling hook.
- [ ] [#qobs1] #agent-doc-feature Add a bounded observer telemetry snapshot surface: supervisor/session actors expose compact per-document state (cycle phase, queue head, actor readiness, IPC inflight, recent typed ops-log markers, and artifact handles) without dumping raw transcripts by default.
- [ ] [#qobs2] #agent-doc-feature Implement deterministic observer rules that scan session-check/preflight/ops-log snapshots for annotated issues and recurring failure patterns, then file deduped session-doc backlog bugs with evidence paths and stable issue fingerprints.
- [ ] [#qobs3] #agent-doc-feature Add a sidecar observer agent-doc loop that can monitor all running project sessions from snapshot artifacts, rate-limit bug filing, avoid pane interference, and route created bug items to the correct session document or a configured observer inbox.
- [ ] [#qobs4] #agent-doc-feature Add model-assisted observer triage as advisory-only: feed compact snapshot/context-pack handles to a subscription or local model, require deterministic evidence before auto-filing bugs, and record model provenance/confidence on suggestions.
- [ ] [#qlm1] #agent-doc-feature Define queue item backend-selection syntax for local-model execution, separate from queued `/model <tier>` switches: support explicit per-item backend/model annotations on normal and loop queue items without changing prompt text or completion proof.
- [ ] [#qlm2] #agent-doc-feature Implement a local-model queue dispatcher backend that can run eligible queue/loop items through a configured local model, capture stdout/structured results, and write responses through the normal agent-doc closeout boundary.
- [ ] [#qlm3] #agent-doc-feature Add local-model dispatch safety gates and telemetry: capability checks, GPU/resource lease integration, fallback policy, provenance logs, and tests proving local-model queue items cannot be silently marked complete without a committed response/proof.
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
