---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent: claude
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
  '#spec-test-build-install-commit-push': red-green-refactor. update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': red-green-refactor. update spec + tests. commit + push
queue: start
---

## Status

<!-- agent:status patch=replace -->

done
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260718-041115.md`*

Compacted content:
- Archived 1 response topic(s): JetBrains route deferred on retained delivery
- Prior summary/context: prior compacted content: Archived 1 response topic(s): Compact Exchange concurrent queue edits and recovered closeout
❯ I switched from Codex to Claude. Then ran JB `Restart Agent`. Then JB `Run Agent Doc` when Claude was open. Got an error. The prompt was sent but not submitted.
```
Agent Doc did not switch harnesses for tasks/agent-doc/agent-doc-bugs2.md. Current actor is codex; frontmatter now resolves to claude-code. The pane is not at a dispatch-ready boundary. Use Interrupt and restart to force the harness switch.
```

❯ #spec-test-build-install-commit-push

### Re: JB harness switch deferred against a stale actor record — opus-4-8

Reproduced from the log — `Restart Agent` worked, and `Run Agent Doc` then refused based on a record that was never updated. Four distinct defects; the error message you saw was wrong on its own terms.

**Timeline from `.agent-doc/logs/ops.log` (all cycle-1784347905776):**

| Time | Event |
|---|---|
| 04:12:06 | `restart_agent_menu_invoked source=jetbrains` |
| 04:12:07 | `harness_change_detected old=codex new=claude gate=Restart` |
| 04:12:09 | `agent_restart_performed old_harness=codex new_harness=claude action=spawn_fresh_harness` |
| 04:12:14 | `route_authoritative_actor_harness_mismatch_deferred stored_harness=codex expected_harness=claude-code actor_state=busy` |
| 04:12:18 | `controller_lifecycle state=busy caller=dispatch reason=auto_trigger_inject` |

The restart **succeeded** — Claude was spawned fresh at 04:12:09. Five seconds later route still read `stored_harness=codex`.

**1. The harness writeback never reaches the persisted record (root cause).**
`run.rs:562` calls `shared.set_current_harness(&new_harness)` under the `#actor-harness-switch-writeback` tag. That setter only mutates an in-memory `Mutex<String>`; its single consumer is `supervisor_io.rs:35`, feeding `tmux_submit_profile_for_harness` for inject delivery. Nothing propagates it to the `harness_id` column that `authoritative_actor.rs:244` reads. The doc comment on `set_current_harness` claims it exists "so the persisted authoritative actor record stops reading the old harness" — that plumbing was never wired.

**2. The comparison normalizes only one side.**
```rust
let expected_harness = agent_doc_harness::normalize_harness_name(&harness.binary);
if !record.harness.trim().is_empty()
    && record.harness != "default"
    && record.harness != expected_harness
```
`record.harness` is raw. The restart logs `new_harness=claude` while route expects `claude-code`, so fixing #1 alone would still mismatch — this one is latent behind the first.

**3. The advice you got was self-inflicted.**
`forceRequired` fired on `actor_state=busy`, but busy came from `restart_continue_spawn` — the restart the plugin had just performed (generation stayed 1485 throughout). "Use Interrupt and restart to force the harness switch" told you to redo the thing that had already worked.

**4. The stranded prompt.**
The pane reconciled `ready` at 04:12:17 and `auto_trigger_inject` typed the trigger one second later, while Claude Code was still starting. Text landed in the composer, Enter did not take. Same shape as `#jbtsiftnosub2`, but on a restart-fresh spawn instead of a JB-created fresh pane — the existing stranded-draft resubmit path doesn't cover this entry point.

**Coverage gap:** `set_current_harness_updates_state_backbone_harness_identity` asserts only the in-memory getter round-trip, so nothing failed when the persisted-record half was absent.

Filed as `#actorharnessrecordwriteback`, `#actorharnessnormcompare`, `#actorswitchdeferbusyself`, `#restartfreshtriggerstranded`, `#actorharnessswitchcoverage`.

**Not implemented this cycle** — this is diagnosis only. The fix spans `start-runtime-io`, the controller actor-record write path, and `route-io`, and the defer-guard change (#3) needs SimWorld coverage to avoid trading a false defer for a false dispatch. I did not want to land a partial multi-crate change and call it done.

**Recovery now:** `agent-doc session restart-supervisor --force`, or revert `agent:` to `codex` and switch again after the queue drains.

One incidental note: preflight first failed the integrity gate (`unclosed component: &lt;!-- agent:exchange --&gt;`) while the CRDT model was mid-reconcile on your pasted fenced block; it cleared on retry once the stale `preflight_started` cycle aged out. No repair was needed.
JB `Run Agent Doc` on equityfundingsource.md error: `route deferred for /home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md: Lazily current transition remained delivery_pending for 5000ms; retry after it settles`

### Re: agent-doc did not submit the prompt — opus-4-8

> **Queue prompt:**
>
> do [#actorharnessrecordwriteback]
>
> do [#actorharnessnormcompare]
>
> do [#actorswitchdeferbusyself]
>
> do [#restartfreshtriggerstranded]
>
> do [#actorharnessswitchcoverage]


All five queued items are fixed, verified, committed (`e39e98650`), pushed, installed, and the supervisor is marked to recycle at the next idle boundary.

Your report — "agent-doc did not submit the prompt" — was the visible tip of five defects that all fell out of the one codex→claude harness switch at 04:12. Four of them were already implemented in the working tree from the diagnosis cycle; this turn finished the two that were still open and landed the whole set together.

**What was already in the tree, now committed**

- `#actorharnessrecordwriteback` — `set_current_harness` wrote only the in-memory harness identity. Nothing wrote the persisted `ActorRecord` that route reads, and `transition_state_*` carries the stored harness forward on every later lifecycle transition, so the record said `codex` forever. The supervisor now writes through `set_record_harness_direct`, preserving pane/generation/state and rejecting stale owners.
- `#actorharnessnormcompare` — the mismatch guard normalized only `expected_harness`, so `claude` vs `claude-code` tripped it even after the writeback landed. Both sides normalize now.
- `#actorswitchdeferbusyself` — the defer read its own in-flight restart's busy window as an operator-blocked pane and told you to "Interrupt and restart to force the harness switch", i.e. abort the restart that was completing the switch. `actor_busy_is_self_induced_restart` now recognizes that window and says to wait for the boundary. A genuinely operator-blocked pane keeps the forceful hint, and a paused queue / unreachable supervisor still outranks the wait (there the boundary restart really will not fire).

**What this turn implemented**

- `#restartfreshtriggerstranded` — the actual "typed but not submitted". The restart-fresh pane reconciled `ready` at 04:12:17 and the inject fired at 04:12:18, while Claude Code was still starting; the submit key was swallowed and the supervisor reported `Sent` anyway. Route's fresh-start path already resubmits a stranded trigger (`#jbtsiftnosub2`), but a restart-fresh spawn is a different entry point that never reaches it. The supervisor inject now snapshots cycle state before the send, waits for a document-cycle ack, and — only on a *dispatch-ready* pane that still shows the trigger — resends one bare submit key and re-checks. A busy pane is a running turn and is left alone; a failed capture fails closed rather than claiming delivery. The decision is a pure function (`auto_trigger_submit_follow_up`) in `agent-doc-supervisor`, so the resubmit-once-then-fail-closed policy is unit-tested without tmux.
- `#actorharnessswitchcoverage` — the old test asserted an in-memory getter round-trip while its comment claimed IPC `state` "feeds the persisted actor record", which is exactly why the missing half shipped. Beyond the session-actor unit tests, there is now a SimWorld scenario that drives codex→claude restart → persisted record → route dispatch, asserts no `harness_mismatch_deferred` marker, **and** carries a negative control that disables the writeback and reproduces your exact log line `stored_harness=codex expected_harness=claude-code`. Without that control the assertions would be vacuous.

I modeled the persisted record as its own SimWorld field rather than reusing `launch_harness`. The two diverging *is* the bug — collapsing them into one string is what let this class hide.

**Verification** — `make check` green (clippy + full suite + TLC). `cargo test --bin agent-doc`: 676 passed. CI run 29632791694 is queued; recording that and continuing rather than blocking on it.

One thing I hit and did not fold in: `crash_resilience::tests::log_panic_to_file_appends_without_panicking` fails under `cargo test --workspace` but passes in isolation and under `make check` — it appends to a fixed panic-log path that concurrently-running crate test binaries share. That is a real test-hygiene defect, just an unrelated one, so it is at the top of the backlog rather than smuggled into this commit.

Still owed by a human: eyeball an actual codex→claude switch in a live pane and confirm the trigger submits on the first try.
<!-- agent:boundary:41dc4f21 -->
<!-- /agent:exchange -->
## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" go -->
- ~~do [#actorharnessrecordwriteback]~~
- ~~do [#actorharnessnormcompare]~~
- ~~do [#actorswitchdeferbusyself]~~
- ~~do [#restartfreshtriggerstranded]~~
- ~~do [#actorharnessswitchcoverage]~~
- ⏭️ 📌 do [#jblocalhistverify]
- ⏭️ 📌 do [#capturebacklogatomic]
- ⏭️ 📌 do [#opcapturelazily]
- ⏭️ 📌 do [#jbtsiftnosub2verify]
- 🚧 📌 do [#5bzd]
- 📌 do [#sidecarstate]
- 📌 do [#64we]
- 📌 do [#adstatechart2]
- 📌 do [#adstatechart3]
- 📌 do [#adstatechartkt]
- 📌 do [#adstatechartjs]
- 📌 do [#ipcauthority]
- 📌 do [#ipcliveness]
- 📌 do [#ipcverhandshake]
- 📌 do [#ipcpostcommitinv]
- 📌 do [#staleplugindeploy]
- 📌 do [#5qjj]
- ⏭️ 📌 do [#jblocalhistverify]
- ⏭️ 📌 do [#capturebacklogatomic]
- ⏭️ 📌 do [#opcapturelazily]
<!-- /agent:queue -->

## Notes

<!-- agent:notes -->
We are not using an evented FD because this path deliberately has no live bidirectional connection to the plugin. The evented transport is the socket IPC path; file IPC exists specifically for “socket absent/degraded/stale but plugin file
watcher may still apply patches”. We could use notify/inotify/kqueue as an implementation detail for the filesystem wait, but it still needs the same timeout, nack, ack-content, committed-cycle, and post-apply validation fallback. The
right extraction is probably a focused FileIpcAckWaiter/sidecar waiter with today’s poll backend first, then an evented backend as an optimization once it is out of orchestration.
---
```
● Finalize deferred with an IPC edge case: socket reported already_applied (response landed in the live buffer) but
  disk_missing_response_probe failed and it deferred ("document changed after merge; retry after typing stops"). Per
  the closeout rules I must not force-disk here — I retry the editor path. Let me first check the actual on-disk state.

  Ran 1 shell command

● The response was captured (cycle at response_captured) and socket-applied to a live JetBrains buffer
  (already_applied), but not yet flushed to disk — a live agent-doc sync --focus process for this doc is holding the
  editor buffer, so the disk-probe deferred ("retry after typing stops"). Per closeout rules I retry the editor path
  (no force-disk). Let me retry the finalize:

  Ran 1 shell command

● Same deferral, and notice: current_len=3546 stayed constant but current_hash changed between the two attempts
  (02e1c199 → 64233287) — the live buffer is actively oscillating (my boundary insertions + a live sync --focus process
  for this doc). The response is durably captured (response_sha256=d7ddb028). This is the already_applied +
  buffer-differs deadlock. Before choosing recovery, I must know if there's a real JetBrains editor or just an orphaned
  sync process holding a phantom buffer.

  Ran 1 shell command

● No live JetBrains IDE, no active listener (both sockets 0B/stale), and the sync process has exited. The
  already_applied acks came from a now-dead stale endpoint — the "live buffer" is phantom. The response is durably
  captured and disk is still the clean 2344-byte prompt. This is the documented stale-endpoint → force-disk recovery
  (no live editor to preserve)
JB `Compact Exchange` on this document had an error. As I was typing this prompt a JB `File Cache Conflict` occurred. I lost what I was typing:
```
agent-doc command failed (exit 1): Error: project controller command `compact_document` failed: compact: editor delivery ACK recovery for /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md did not settle within 8000ms; the canonical response remains retained in CRDT + Lazily state and the editor reconnect continues asynchronously (no force-disk or operator recovery required)
```.---
 Let me first confirm HEAD hasn't advanced (no concurrent commit), then force-disk
  finalize.
```
---
Please make backlog replacements for all gated review items. We need to complete the plan, not have any more gates after this round. Let me know when I need to install JB plugin updates. Make sure all JB live gate items have adequate logging to prove/disprove the item. Clear the review items. I will test, files bugs, and you will check the logs.
---
`'/clear' is disabled while a task is in progress
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
<!-- /agent:notes -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#crashresiliencelograce] test-hygiene: crash_resilience::tests::log_panic_to_file_appends_without_panicking fails under a parallel workspace run (cargo test --workspace) at src/crash_resilience.rs:173 but passes in isolation and under make check. The panic-log append test shares a fixed path across concurrently-running crate test binaries. Give it a per-test temp path so the suite is order-independent.
- [ ] [#compacttombgc] [recommended] Add a fenced Compact Exchange CRDT epoch: rebootstrap attached replicas from the compacted snapshot, reject or component-rebase older-epoch deltas, expose the all-replica stability frontier, then run TextCrdt tombstone GC.
- [ ] [#compactindcellsverify] [operator-verify] With agent-doc 0.35.3 and JetBrains plugin 0.2.276 active, run Compact Exchange while deleting several queue items and editing a post-boundary prompt; confirm the deletions and prompt survive, compaction commits, and ops.log records compact_rebased_authoritative_siblings without compact_dropped_non_exchange_item.
- [ ] [#5qjj] steeringmirrorverify|[operator-verify] Restart IntelliJ IDEA 2026.1 to activate plugin 0.2.258 (if stale bytecode appears, uninstall, restart, reinstall the signed local zip, and restart); during an active turn, make multiple exchange edits and verify the JetBrains banner and VS Code status item report the aggregate count/full hover, then run agent-doc once after editing and confirm the handoff queues without submitting a partial edit.
- [ ] [#zqv8] [operator-verify] #jbrelaylive Restart IntelliJ IDEA 2026.1 to activate the installed JetBrains plugin 0.2.247, rerun Run Agent Doc on tasks/professional/equityfundingsource.md while the dim Summarize recent commits suggestion is visible, and confirm dispatch succeeds without a protected-input refusal or a long UI freeze.
- [ ] [#crde] JetBrains Run Agent Doc route/dispatch failed after bounded submit/start proof retries #jbrunautobug #agent-doc-bug failure_class=accepted_without_dispatch_start_proof document=/home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md stage=dispatch_only_dispatch_start_proof pane=%83 actor_generation=1214 editor_attempt_id=1783978030079-4 dispatch_proof_state=accepted_without_dispatch_start_proof elapsed_ms=10000 diagnostic_path=none ops_log_path=/home/brian/work/btakita/agent-loop/.agent-doc/logs/ops.log ops_log_marker=route_submit_issue(issue=accepted_without_dispatch_start_proof,phase=dispatch_only_dispatch_start_proof,result=accepted_without_dispatch_start_proof) [symptom-key invariant=run_agent_doc_route_dispatch_failure document=71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad component=route/dispatch_only_dispatch_start_proof content_hash=sha256:3251e072b5c1857ba06e9ba613e1004b5eefef103db7003d7dcf7efe0d698ea8]
  evidence: JetBrains Run Agent Doc route/dispatch failed after bounded submit/start proof retries #jbrunautobug #agent-doc-bug failure_class=accepted_without_dispatch_start_proof document=/home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md stage=dispatch_only_dispatch_start_proof pane=%83 actor_generation=1214 editor_attempt_id=1783978020608-2 dispatch_proof_state=accepted_without_dispatch_start_proof elapsed_ms=10000 diagnostic_path=none ops_log_path=/home/brian/work/btakita/agent-loop/.agent-doc/logs/ops.log ops_log_marker=route_submit_issue(issue=accepted_without_dispatch_start_proof,phase=dispatch_only_dispatch_start_proof,result=accepted_without_dispatch_start_proof)
- [ ] [#steeringobservableset] #agent-doc-bug Phase 6 durable graph adoption: back RealtimeSteeringSet with a lazily reactive OBSERVABLE SET (not a drain-one-head QueueCell) keyed on the body-aware identity (Phase 3.1), reconciled on the reliable-sync plane; session-check reads set-emptiness (all directives settled) instead of re-diffing HEAD↔buffer. Plan: src/agent-doc/tasks/agent-doc/plan-exchange-tree-seqcrdt-and-ipc-unify.md (Phase 6). [recommended]
- [ ] [#jblocalhistverify] [operator-verify] #jblocalhistcrash live proof: install editors/jetbrains/build/distributions/agent-doc-jetbrains-0.2.232.zip (uninstall -> restart -> install -> restart), then with a corrupt Local History store trigger Run Agent Doc and confirm dispatch proceeds (no aborting AssertionError) and idea/ops log shows active_document_save_failed then normal sendToTerminal.
- [ ] [#capturebacklogatomic] [focused-cycle] #agent-doc-bug A finalize that captures the response but fails before recording backlog mutations strands in response_captured with requires_backlog_capture=true + pending_added_this_cycle=false; preflight then refuses closeout and errors "still response_captured after recovery", forcing a manual mv-aside because replaying the capture re-trips the backlog gate. Fix: capture the whole cycle intent (response + backlog mutations) atomically, and let preflight supersede an uncommittable capture instead of looping. Exposed by this turn. Plan: tasks/agent-doc/plan-capture-includes-backlog-mutations.md
- [ ] [#opcapturelazily] [focused-cycle] #sidecarstate op-capture (.agent-doc/op-capture/<doc-hash>.json, agent-doc-op-capture-io #qnodemerge4) is a bespoke JSON sidecar source of truth: durable ordered EditorOps + base_hash that preflight repair / CRDT merge REPLAY as authority then consume+clear (base-hash-guarded so safe). Because it is a separate replay source, clearing captures/ + state/cycles/ aside did not stop the churn this turn. Migrate onto the lazily state backbone (co-located with cycle-phase state) so captured-op authority is one coherent atomically-clearable model. 96-ref cross-crate surface; folds into #sidecarstate. Plan: tasks/agent-doc/plan-op-capture-lazily-state-migration.md
- [ ] [#jbtsiftnosub2verify] [operator-verify] #jbtsiftnosub2 live proof (0.34.74): with NO tmux pane for a doc, trigger JB Run Agent Doc and confirm the reopen trigger SUBMITS (or auto-resubmits) instead of sitting typed-but-unsubmitted; ops.log shows fresh_route_stranded_trigger_resubmit + fresh_route_start_acknowledged_after_resubmit; a stuck submit now fails closed with a visible FreshStart startup-miss.
- [ ] [#sidecarstate] [focused-cycle] #agent-doc-bug Replace hot-path snapshot/cycle/live-buffer sidecars with the lazily state backbone for document authority, lifecycle recovery, and crash-reentrant pipeline stages; auto-start/reconcile the document model before preflight exposes missing CRDT replica errors; reconcile drift inside agent-doc on re-entry. Plan: tasks/agent-doc/plan-lazily-state-backbone-sidecar-retirement.md
- [ ] [#64we] JetBrains Run Agent Doc route/dispatch failed after bounded submit/start proof retries #jbrunautobug #agent-doc-bug failure_class=accepted_without_dispatch_start_proof document=tasks/professional/equityfundingsource.md stage=supervisor_dispatch_start_proof pane=%156 actor_generation=1039 editor_attempt_id=1783140065173-1 dispatch_proof_state=none elapsed_ms=10134 diagnostic_path=/home/brian/work/btakita/agent-loop/.agent-doc/logs/route-submit/1783140083790-supervisor_dispatch_start_unproven-codex-_156-ad5c195b73fc.txt ops_log_path=/home/brian/work/btakita/agent-loop/.agent-doc/logs/ops.log ops_log_marker=route_submit_issue(issue=accepted_without_dispatch_start_proof,phase=supervisor_dispatch_start_proof,result=accepted_without_dispatch_start_proof) [symptom-key invariant=run_agent_doc_route_dispatch_failure document=71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad component=route/supervisor_dispatch_start_proof content_hash=sha256:503e36d813c831fce9bfe367973c345adce5a2f2620dccc4fa0be6e4363b5087]
- [ ] [#adstatechart2] [focused-cycle] #agent-doc-feature Phase E rung 2 (observability): wire session-check/status to READ adstatechart configuration() as an advisory named-snapshot (transport.x editor_sync.x closeout.x supervisor) alongside today's ops.log markers. No behavior change to closeout. Guard semantics must match whatever A-D shipped. PRD: tasks/agent-doc/prd-adstatechart-local-process-statechart.md
- [ ] [#adstatechart3] [focused-cycle] #agent-doc-feature Phase E rung 3 (load-bearing): switch adstatechart to the typed lazily 0.19 ChartBuilder, bump agent-doc-state-backbone to lazily 0.19, use ThreadSafeStateChart for cross-thread status observation, and replace the ad-hoc editor_synced check at the finalize/commit site with chart send(commit, guards) plus a fail-closed fallback when the feature is off. SimWorld: N wedged-finalize->direct-commit cycles never commit from stale disk. Live editor proof gated behind decomposition + install. PRD rung 3.
- [ ] [#adstatechartkt] [focused-cycle] lazily-kt 0.9.0 SHIPPED (ChartBuilder + ThreadSafeStateChart + shared fromStates + context-free engine; GitHub Packages via release.yml, tag v0.9.0; full test suite green). REMAINING: bump the JetBrains plugin's lazily-kt dependency to 0.9.0 and consume ChartBuilder/ThreadSafeStateChart for the merge-ownership state machine (#mergestatemachine3 fold-in).
- [ ] [#adstatechartjs] [focused-cycle] lazily-js 1.1.0 SHIPPED (ChartBuilder + statechart.d.ts builder types + shared fromStates; tag v1.1.0; 169/169 tests green). No thread-safe/async chart in js (no reactive Context, no threads). REMAINING: npm publish is operator-2FA-gated (#lzjsnpmpub: npm login then npm publish --access public), then bump the VS Code plugin @lazily/js dep to 1.1.0 and consume ChartBuilder.
- [ ] [#ipcauthority] [focused-cycle] #agent-doc-bug IPC root-cause A (data-loss authority ambiguity). DONE this pass: durable recovery sidecar for operator text dropped by the content_ours merge (commit 317a336f) — write_policy::content_ours_drops_operator_text + agent_doc_fs::preserve_dropped_operator_buffer wired into all 3 run_entry ContentOurs persist sites; concurrent operator text now lands in .agent-doc/recovery/ instead of vanishing (closes the #qftlossdelta artifact gap). REMAINING: the deeper unification (make the CRDT the single closeout authority, demote content_ours/snapshot to audit-only) so the merge never needs the safety net — larger refactor, own cycle.
- [ ] [#ipcliveness] [focused-cycle] #agent-doc-bug IPC root-cause D (transport/state liveness). DONE this pass: corrupt controller-state auto-quarantine (commit e06301e6) — agent_doc_fs::read_valid_or_quarantine wired into project_controller::read_bootstrap; a 0-byte/unparseable controller-state.json is moved aside + re-bootstrapped instead of wedging start (class 11 auto-recovery). REMAINING (blocked on decomposition landing, dirty files): editor IPC listener read_line no timeout (fd/thread leak on half-open client), connect_sync no connect timeout, file-IPC no negative-ack. Touches ipc_socket.rs, write/ipc/transport.rs, agent-doc-ipc-io, editor plugins.
- [ ] [#ipcverhandshake] [focused-cycle] #agent-doc-bug IPC root-cause B: cross-process version skew has no negotiation (staleness inferred from mtime/inode; drives stale_install + stale-supervisor CRDT corruption). Extends the shipped #stale-plugin-detect. Fix: add a build-id/protocol-version field to the IPC handshake (agent-doc-ipc-protocol) so a newer finalize client refuses or forces-reload on mismatch. status.rs already carries injected_binary_version to extend to the wire.
- [ ] [#ipcpostcommitinv] [focused-cycle] #agent-doc-bug IPC root-cause C: structural invariants enforced on wrong write path. #boundaryaccum root fix landed (git.rs unconditional snapshot collapse) but needs a POST-COMMIT verification gate that fails loudly/self-heals if HEAD carries more than one boundary or a duplicated scaffold. Reuse ipc_corruption.rs detect_duplicated_scaffold. Touches git.rs, ipc_corruption.rs, flow/closeout.rs. Plan: tasks/agent-doc/plan-boundary-accumulation-committed-blob.md.
- [ ] [#staleplugindeploy] After the module-decomposition refactor lands: make install + push the shipped #stale-plugin-detect change (submodule commit 9b38d017). Deferred this cycle: installing would deploy the uncommitted decomposition WIP as the live binary. Verify a live stale plugin then surfaces the stale_plugin preflight warning.
- [ ] [#vscsaveproj] #agent-doc-bug VS Code save_document observability parity: processSaveDocumentSignal (editors/vscode/src/extension.ts) only writes outcomes to outputChannel, while JB saveDocumentViaDocument records StateProjectionBridge surface ops for missing_file/missing_document/saved/failed. Add the VS Code state-projection surface-op analogue so save_document outcomes are observable in the state mirror (not just the log), plus a behavioral test driving processSaveDocumentSignal end-to-end (open doc -> signal -> save -> ack-content).
- [ ] [#restartbleednonroute] #agent-doc-bug Harden auto-install child stdio against fd2==pane: run_auto_install_steps_once (project_controller/rpc.rs) blindly dups fd2 via auto_install_child_stdio_plan(); when SupervisorStderrRedirect is inactive (non-route-owned TUI, or route-owned but maybe_start fell back to inactive on error) fd2 is the agent pane and make-install output still bleeds. Open the session project_root .agent-doc/logs/supervisor-stderr.log and pass its fd to auto_install_child_stdio_plan_to_fd(log_fd) so build output is off-pane regardless of route ownership; add a redirect-inactive regression.
- [ ] [#gd88] [operator-verify] #restartstderrbleed live re-proof on a fixed binary: after the supervisor rebuilds onto a build containing auto_install_child_stdio, trigger a restart/auto-install idle boundary and confirm the agent pane shows NO make/cargo bleed, NO tmux split-window, and NO submitted /usage.
- [ ] [#r4gq] [operator-verify] Live JB Run Agent Doc dispatch-hang proof (#kjw0 fix, shipped 0.34.66 build): after the recycled build is running, trigger Run Agent Doc into a BUSY codex pane (mid-turn, 'esc to interrupt' visible) and confirm it returns FAST with a queued-behind-active-turn outcome (pane shows queued status) instead of hanging ~21s then a dispatch-start-unproven notification. Grep ops.log for route_dispatch_start_queued_behind_active_turn (present on success) and confirm accepted_without_dispatch_start_proof is ABSENT for the busy case. Root cause: route injected the trigger into a mid-turn pane; codex queued it behind the active turn so the UserPromptSubmit hook could not advance within the 10s proof budget, and the direct-pane path paid TWO 10s waits (initial + late-resubmit) then filed a false bug. Fix: short_circuit_dispatch_start_when_pane_busy in route/dispatch.rs detects busy_proof_line and returns RoutedDispatchStartProof::AcceptedQueuedBehindActiveTurn (accepted, not fail-closed).
- [ ] [#af88] [focused-cycle] #agent-doc-bug IPC wedge hardening follow-ups from the 2026-07-01 audit (wedges C + A FIXED this cycle: supervisor IPC listener now spawns per-connection threads so a half-open client cannot freeze all supervisor dispatch; editor IPC send now has set_send_timeout). Remaining: (E) pty.rs stdin.lock() held across the blocking read — evaluate a raw-fd read that preserves the restart/quit-prompt stdin coexistence in start.rs; (B/D) editor IPC listener per-connection read_line has no timeout (thread/fd leak under a persistent half-open client) — add a bounded backstop; (F) connect_sync has no connect timeout (mitigated today by the 3s wait_for_controller deadline loop). Also decide the AGENTS.md 'No async' convention nuance for the server-scale roadmap.
- [ ] [#claimnewpane] [focused-cycle] #agent-doc-feature Claim for Tmux Pane should open a new document in a fresh pane in the SAME tmux session (never replace the current pane or move the window to another session). Plan: tasks/agent-doc/plan-claim-new-pane-same-session.md
- [ ] [#boundaryaccum1] [focused-cycle] #agent-doc-bug Scattered agent:boundary markers accumulate in committed content (3 in this doc: d0dd7c4e, qactsync1, ftimm0001) — violates the exactly-one-boundary invariant. Root cause: reposition_boundary_in_snapshot (git.rs:1613) hands the collapse to the finalize/IPC path via the active-run guard AND delegates the working-tree rewrite to the IDE plugin via the IPC-listener guard; when finalize wedges on stale_install and the response lands via direct commit (and the stale plugin never applies the IPC reposition signal), nothing re-enforces the single-boundary invariant on the binary's own staged/committed blob, so one boundary accretes per wedged cycle. Fix: always collapse boundaries in the binary-owned snapshot/staged blob independent of the guards (git stages a snapshot, not the live editor buffer, so it never races user text) and scope the guards to the working-tree disk rewrite only; add a post-commit invariant check that reconciles or fails loudly if HEAD carries >1 boundary; promote the aspirational reposition_boundary_removes_stale/apply_patches_boundary_invariant tests to real regressions + SimWorld coverage for N wedged-finalize→direct-commit cycles. Distinct from #fullboundary (convergence gate reduces the wedge trigger; this makes the invariant self-healing regardless of write path). Plan: tasks/agent-doc/plan-boundary-accumulation-committed-blob.md
- [ ] [#qactsync] [focused-cycle] #agent-doc-feature Bidirectional realtime sync of queue-activation state: when the operator adds/removes go on agent:queue OR changes the queue: frontmatter value, the realtime system converges BOTH representations to the operator edit (operator-authoritative, never revert). Today explicit_queue_go_mode reads both but they are not synced, so halting needs editing both (go-marker churn). Define a canonical QueueActivation state + edit-provenance propagation; deterministic on two-sided conflict; fixed-point on synced input. SimWorld both directions + conflict; make tmux-ci + live editor proof. Plan: tasks/agent-doc/plan-queue-activation-state-sync.md
- [ ] 🚧 [#ftimmediate] #agent-doc-feature Free-text prompts begin work immediately: a free-text exchange/queue prompt is an executable directive that should start work at once; if the agent determines a dedicated/fresh session is needed (high accretion or focused-cycle scope), it should auto /clear or request a PCP restart and RUN the work, not park it as an operator-gated defer. Tighten the #drain-no-defer / #qfocsup boundary so focused-cycle free-text prompts get run-after-clear rather than left for the operator.
- [ ] [#qeditrace] [focused-cycle] #agent-doc-bug Pre-run queue edits partially reverted: when the operator adds multiple queue heads before a run (or JB Run Agent Doc) with no active turn, only the FIRST added head survives and the rest revert to the pre-edit queue. Distinct from the during-turn race - the preflight queue-maintenance reconcile drops all-but-first operator addition. Investigate preflight/maintenance.rs: queue_journal record + record_live_buffer (#qftloss mode-6), #mirrorall backlog-to-queue mirror, #rz3a freshly-added hold-out, #7r2s manual-add pin, #qdedupsync dedup - hypothesis is convergence keeps the first new head (pin/anchor match) but journals-without-replaying the rest. Repro 2026-07-01 reitrades.md; live dup heads in bugs2 (do #tmuxsynccrashlog x2, do #60ev x2). Add SimWorld coverage for multi-line pre-run operator-add survival + fail-closed recoverable artifact. Plan: tasks/agent-doc/plan-queue-edit-race-consolidated.md
- [ ] [#jbcompactcommit] [operator-verify] Live JB Compact Exchange no-uncommitted-changes proof: after the fixed binary is installed + recycled, run JB Compact Exchange on a large session doc and confirm HEAD lands the compacted content (no green git gutter / no snapshot>visible drift on next preflight). Grep ops.log for compact_commit_head_mismatch (must be ABSENT on success; present = the new fail-closed guard fired with a reset --from-current recovery). Rust fix shipped in agent-doc-orchestration/src/compact.rs commit_compacted_authoritative (#jb-compact-commit-left-uncommitted).
- [ ] [#tmuxsynccrashlog] [operator-verify] Replacement for #tmuxsynccrash: JB Sync Tmux Layout live repro log review. After the operator runs repeated JB Sync Tmux Layout against the repro layout, inspect idea.log, agent-doc ops.log, and tmux server pid/session evidence; close only if the server pid stays stable with repair_layout no-op/passive supersede markers, otherwise file a follow-up bug with the failing log markers. Plan: tasks/agent-doc/plan-sync-tmux-layout-crash.md
- [ ] [#60ev] plan-dynamic-context-injection-ledger.md
- [ ] [#2y8j] [operator-verify] #qbasehashmemo live typing proof: after the supervisor auto-installs 0.34.49 and recycles, type a sustained burst in a large doc (equityfundingsource.md or agent-doc-bugs2.md) in the JB editor and confirm typing is smooth; grep ops.log to confirm crdt_merge_base no longer fires per keystroke during a typing burst.
- [ ] [#cdtcutover] [focused-cycle] #agent-doc-feature FUTURE/SEPARATE rung — wire the evented CellDocTree model into the live finalize/write path (replace whole-tree reconcile at finalize with update_to per-node deltas; expose SemTree queries to preflight/supervisor drainable computations). Large, live-supervisor-critical, gated, own focused cycle + operator live proof. Plan: tasks/agent-doc/plan-celldoctree-evented-reflection.md
- [ ] [#coredecomp] [focused-cycle] Keep the former `agent-doc-core` God-crate surface decomposed into owned pure crates. Audit `pending.rs`, `queue_item_lifecycle.rs`, `template.rs`, `crdt_sync.rs`, `component.rs`, and related modules; move element-local semantics into `agent-doc-element-*`, document composition into `agent-doc-document`, merge/CRDT merge policy into `agent-doc-merge`, and FFI ABI into `agent-doc-ffi`. Do not recreate `agent-doc-core` as a compatibility facade or migration alias, and do not move runtime IO, snapshots, commits, IPC, tmux, or controller effects into pure crates.
- [ ] [#orchdecomp] [focused-cycle] Decompose the `agent-doc-orchestration` God crate along effect boundaries. Audit turn lifecycle, document realtime scheduling, queue/exchange lifecycle, closeout/commit, editor IPC, supervisor/controller state, tmux integration, recovery/repair, and session routing; move stable responsibilities into `agent-doc-turn`, `agent-doc-document`/document-realtime, `agent-doc-supervisor`/controller-state crates, `agent-doc-editor`, `agent-doc-tmux`, and narrow closeout/recovery crates as appropriate. Leave `agent-doc-orchestration` as a shrinking effect adapter only, not a deprecated compatibility facade; callers should move directly to the owning focused crate and pure merge/document code should stay out of orchestration.
- [ ] [#skillharnessplugin] [focused-cycle] Create the agent-doc integration plugin for `skill-harness`: implement an agent-doc-owned `SkillHarnessPlugin` adapter, define its detection and skill-path contract without depending on `agent-doc-core`, register/document the integration path, and add tests proving `skill-harness` can resolve agent-doc through the plugin boundary after `agent-doc-core` removal.
- [ ] [#crdtauth6] [operator-verify] CRDT authority live proof: verify the existing JB DocumentListener -> CrdtReplicaForwarder.forwardLocalDelta path, Kotlin Unix-socket ReplicaTransport, VS Code koffi replica seam, and Rust fan-out stay connected in a live editor session. This is proof/parity work only; operator-visible text is already authoritative and must not wait on this item. Plan: tasks/agent-doc/plan-crdt-authority-model.md
- [ ] [#nsfz] [operator-verify] #startupdeadline live dogfood: drive a real harness that never reaches a dispatch-ready prompt (e.g. auth-walled/hung TUI) and confirm the session fails closed within 30s with a 'session did not become dispatch-ready in Ns' stderr diagnostic + a recorded startup_miss marker, instead of hanging. Code-complete + tested (make check 5101 green, release build green); needs a live editor/pane eyeball.
- [ ] [#kj5flive] [operator-verify] #mergestatemachine3 live proof replacement for cleared review #kj5f: after the latest JB plugin update is installed, verify JB+VSCode attach/detach/ownership facts flow through the merge-ownership state machine via #lazilystatesync, then run live editor-attached/detached repros for File Cache Conflict and the #6b5h wedge with logs sufficient to prove/disprove the fix before filing follow-up bugs. Plan: tasks/agent-doc/plan-merge-control-state-machine.md
- [ ] [#suprestassoc-live] [operator-verify] Restart the `tasks/professional/equityfundingsource.md` supervisor in a live editor/tmux setup and verify the actor/admin projection keeps EFS bound to its own pane/session without re-associating to `tasks/agent-doc/agent-doc-bugs2.md`.
- [ ] [#agentsignals] [future/separate] Add a realtime agent:signals element after core realtime convergence is stable. Start with deterministic built-in signals for tests, queue, supervisor, and ipc; design custom user-defined signals as plugin-backed tracked work with Rust as the first target and future multi-language support; use tasks/software/realtime-signals-patchboard.md as the GUI/semi-programmatic reference; keep MLP enhancement optional/experimental and not load-bearing for deterministic states.
- [ ] [#elementgraphprd] [future/separate] Define the single-document element layout for both single-model sessions and multi-harness orchestration, including typed graph/workers/jobs/schedule elements plus an advisory perceptron graph that scores visible signals without directly mutating runnable work. PRD: tasks/agent-doc/prd-element-layout-perceptron-graph.md
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
- [/] [#qftlossdelta] #agent-doc-bug focused invariant-loss follow-up from failed #qftlossproof live proof: `.agent-doc/logs/ops.log:4338964-4338997` shows ACK/live-buffer content at len 16676 hash `83f08ab7e6be49a671f96f7c0ffbb71fe28bba858929b883c5904140cabd5192`, then closeout committed len 16669 hash `dc278d260b5b2a3efffeb12c428f0f81f5be1c8373fdd3ef044eee4b39adda4f`; `live_buffer_classify` suppressed the differing operator-capable buffer with `reason=write_provenance_newer_than_buffer` and no durable sidecar preserving the 16676-byte content was found. Fix the operator-wins merge/write-provenance path so concurrent operator text is preserved end-to-end or the write fails closed with a recoverable artifact. Plan: tasks/agent-doc/plan-queue-edit-race-consolidated.md
- [/] [#falsetyping-guard] #agent-doc-bug provenance-aware visible-write guard. INTEGRATED to main (merge on 5bcfc07b) + binary/cdylib installed + recycled; guard tests 7/7 green; JB 0.2.205 zip + VS Code 0.2.38 compiled. REMAINING (operator): install JB plugin zip (build/distributions/agent-doc-jetbrains-0.2.205.zip) + reload IDE, install VS Code 0.2.38, then live proof — type during replica churn (no wedge) + genuine unsaved edit still fails closed. NOTE: bugs2.md can still wedge until the IDE plugin is reloaded (loaded plugin still calls FFI v2 → conservative fail-closed).
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
- [ ] [#exttasksources] #agent-doc-feature Add external task source adapters that project PM-tool tasks into the active document/work graph as source-backed nodes, keeping local queue/closeout authority document-owned and making remote push-back explicit and audited. PRD: tasks/agent-doc/prd-external-task-sources.md
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
