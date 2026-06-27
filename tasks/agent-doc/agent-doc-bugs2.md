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

❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260627-000645.md`*

Compacted content:
❯ - Archived 6 response topic(s): IPC proof diagnostic (interrupted-cycle recovery); distinguishing operator vs agent-doc authorship; should we remove the post-commit IPC worktree reconcile?; 3 more
❯ - Prior summary/context: prior compacted content: Archived 47 response topic(s): Is CellDocTree too specific to lazily?; queue state and revie...

### Re: do #provauth1 #provauth2 #provauth3 — provauth3 shipped; 1 & 2 already live — opus-4-8

**#provauth1 and #provauth2 were already committed and in HEAD** — I verified rather than redo them:
- **#provauth1** (`3c9dd556`, current submodule HEAD) — per-component authorship merge (operator-owned cells theirs-win clean, conflict markers default OFF). The architectural reframe holds: `crdt.rs` is a per-cell text merge with no yrs transaction origins, so provenance lives at the *merge boundary* (ours=binary, theirs=operator), and it's in.
- **#provauth2** (`cd8f3522` multi-id dedup + `08df743c` operator-delete tombstone) — backlog→queue mirror never duplicates operator multi-id lines and never resurrects an operator-deleted head.

Both are in the installed 0.34.56 binary. Marking `#f693`/`#818s`/`#fpyh` done.

**#provauth3 — implemented this cycle (`296ffe51`, v0.34.57, `make check` green / 5434 tests).** This is the origin-over-content replacement, and it closes the *exact* bug this session opened on: resuming after `/clear`, the stale pre-compact snapshot (33119 b) vs the compacted file (19123 b) both (a) tripped `guard_no_stale_snapshot_reset_drift`'s "looks like a manual cleanup" refusal — I had to hand-recover with `reset --from-current` + `commit` — and (b) had the preflight `repair` prompt-prefix normalizer stamp every `### Session Summary` / `*Compacted…*` / `- Archived N response topic(s)…` line with `❯`, so the classifier + `session-check` read the binary-authored compaction as an unresolved user prompt and falsely INTERRUPTed closeout. Both are the #provauth thesis in miniature: a *content* diff can't tell a binary-authored compaction from operator text.

The fix consults a **recorded origin fact** instead of guessing:
- `classify_stale_snapshot_visible_rebase` (`write/converge.rs`) no longer hard-requires a live `turn_scope` (which is absent after `/clear`). It reads the existing binary-authored `recent_exchange_compaction_timestamp` marker (it survives `/clear`): an exchange-only safe reduction rebases the stale snapshot when there's *either* a live turn scope (the prior in-session path) *or* a recorded binary compaction. Non-exchange drift still needs the scope; a compaction-shaped shrink with **no** provenance still fails closed with the `reset --from-current` guidance (safety rail kept + tested).
- New `line_is_binary_authored_compact_summary` (`agent-doc-core/src/diff.rs`), mirroring the `line_is_binary_authored_ipc_proof_diagnostic` pattern, tolerates a stray `❯` and is wired into `is_recovery_artifact_line`, `classify_prompt_bearing_block` (an all-summary block → `RecoveryArtifact`, never `PromptTarget`), the normalizer's insert filter (never stamp `❯`), and the `session-check` unresolved-tail guard. A real prompt that merely mentions "compacted" still classifies normally.

**Scope boundary (honest):** that's the binary-origin half — where origin is *known today*. The operator-origin half of #provauth3 ("an operator working-tree change IS authoritative; retire `content_ours` / stale-editor resurrection heuristics") needs the live operator-origin delta signal from the `#crdtauth6` seam, which isn't live yet — so I filed it as **#provauth3b** rather than fake it with more content heuristics.

**Next (doing now, agent-doable):** `make install-full` → `admin recycle` so 0.34.57 goes live, then push. After recycle, the operator-eyeball proof is simply: this session no longer needs a manual compaction recovery and no `❯` appears on a Session Summary.

### Re: drain #actorprune — dead actor records now pruned (v0.34.58) — opus-4-8

Draining the go-queue on the fresh 0.34.57 supervisor. Took **#actorprune** — the most self-contained non-blocked id in the priority head (#pcwcrt is blocked on #crdtauth6, #syncbarrier is large, #suprestassoc needs a live repro).

**Shipped (`23e0e4ff`, v0.34.58, `make check` green / 5438 tests):**
- Root cause: `close_stale_starting_actors` only *transitions* `Starting`→`Closed`; nothing ever *removed* `Closed` records, so the controller actor store (`documents` table) and `admin list` grew without bound — your 251 dead `session=session-clear` rows.
- New `prune_dead_actors` (`project_controller.rs`): hard-removes a record when **all** hold — state is `Closed`, `last_transition.timestamp` older than `DEAD_ACTOR_PRUNE_AFTER` (1h, matching the stale-`Starting` window), **and** no fresh/alive supervisor lease owns its document/generation (`supervisor_lease_is_fresh_or_alive`, whose pid-alive check means a dead supervisor's lingering lease never blocks the prune, and a live actor is never pruned).
- New `delete_actor_document_tx` (`state_store.rs`): removes the `documents` + `actor_transitions` + `supervisor_leases` rows for one document_id in one transaction.
- Wired into `gc.rs` default-on (dry-run honored), so **`agent-doc gc` cleans the accumulated 251 now**. Every prune is logged (`gc_pruned_dead_actor … reason=dead_closed_record`).
- Subtlety caught by a test: the prune refreshes the legacy `session-actors.json` projection **directly from the post-delete db**, not via `emit_actor_projection`/`load_actor_store` — those re-run `migrate_legacy_actor_projection`, which once the prune empties `documents` would re-import the just-deleted rows from the stale json and *resurrect* them. 4 new tests cover remove/keep-recent/keep-live-lease/dry-run-preserves.

**Also fixed:** 0.34.57's version bump moved only the top-level `[package]` version, leaving the sub-crate versions + inter-crate dep refs at 0.34.56 — which breaks `cargo publish` (`test_manifest_uses_publishable_dependency_contract`). All workspace versions are now bumped together to 0.34.58.

Marking `#actorprune` done. Next: install 0.34.58 + recycle, push, then the loop continues on the fresh binary. Remaining drainable heads are #syncbarrier (large), #suprestassoc (needs live repro), the JB-flood dispatch bug, and the two resolved-by-provauth2 free-text reports (deleted-items-reappear / no-dup-adds — already closed by the 0.34.x tombstone+dedup, will strike them next).
<!-- agent:boundary:4f2fbec6 -->
<!-- /agent:exchange -->
## Queue


<!-- agent:queue priority preset="#spec-test-build-install-commit-push" priority go -->
- ~~[#f693] [#q9w3] [#fpyh] [#818s]~~
- 🚧 [#pcwcrt] [#syncbarrier] [#qheadstrikeauto] [#actorprune] [#suprestassoc]
- [#2y8j] [#cdtcutover] [#qeditdup] [#crdtauth6] [#nsfz] [#kj5flive]
- I deleted items from the queue but they reappeared
- When auto-adding queue items, verify that the item is not already in the queue. Do not add duplicates.
- JB `Run Agent Doc` flooded the prompt with duplicate `agent-doc .../agent-doc-bugs2.md` dispatch prompts. Please fix the root cause.
- ~~do [#f693]~~
- ~~do [#fpyh]~~
- ~~do [#818s]~~
- do [#actorprune]
- do [#suprestassoc]
- do [#cdtcutover]
- do [#qeditdup]
- do [#crdtauth6]
- do [#nsfz]
- do [#kj5flive]
- ~~do [#provauth3b]~~
<!-- /agent:queue -->
<!--
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
- [ ] [#pcwcrt] #agent-doc-feature realtime cutover — gate/remove the post-commit working-tree reconcile + (HEAD) boundary reposition tower (git.rs reconcile_postcommit_worktree_to_head / #pcwc / #pcwcwarn / #pzjy / content_ours) when a live CRDT replica is attached. It is turn-based file-sync legacy: its value (keep editor buffer==HEAD after a commit, reposition the boundary, avoid the JB File Cache Conflict dialog, avoid #xbs3 disk-RMW racing the typist) is all subsumed by delta application in realtime — and re-deriving the buffer from a stale snapshot is the recurring #postcommit-ipc-worktree-corruption source. Keep the disk-write path ONLY for no-live-editor (pure-CLI #6b5h) sessions. Rides #provauth1 origin tagging + #crdtauth6 live replica + #cdtcutover. Plan: tasks/agent-doc/plan-operator-provenance-authority.md
- [ ] [#syncbarrier] #agent-doc-feature Phase 1 (editor-sync barrier) — epoch handshake so the binary DEFERS dangerous ops (snapshot reset / disk reconcile / merge-base rebuild) while an editor edit is in flight, bounded wait, fail-OPEN to the editor buffer (never a held lock, never snapshot-discard). Poll-check = yrs StateVector exchange, not text compare. Needs JB/VSCode edit_epoch + StateVector over IPC (extend State method). Phase 0 #ipctruncrecover shipped (0.34.53). Plan: tasks/agent-doc/plan-editor-sync-barrier.md
- [ ] [#qheadstrikeauto] #agent-doc-feature auto-strike answered free-text head WITHOUT requiring the agent to quote it as a > **Queue prompt:** blockquote — operator: "the binary should do this automatically...not the agent". Today strike_answered_free_text_queue_heads (queue_consume.rs) only fires on a blockquote/prose-prefix match, so a head answered in plain prose is never struck. Key the strike off the binary drain-target / in-progress (the marker) head identity for the cycle instead of agent prose formatting. Plan: tasks/agent-doc/plan-freetext-queue-head-autostrike.md
- [ ] [#actorprune] #agent-doc-feature auto-prune dead actor records: `admin list` shows 251 actors, ~all `(dead) closed pid=? session=session-clear`; gc only closes `Starting` actors (project_controller.rs:1716), nothing removes long-dead `Closed` records so they accumulate unbounded. Add prune_dead_actors (lease-guarded) + wire into gc + one-shot prune for the current backlog. Plan: tasks/agent-doc/plan-prune-dead-actors.md
- [ ] [#suprestassoc] #agent-doc-bug restart-supervisor re-associates the wrong doc: restarting the equityfundingsource.md supervisor re-bound EFS to the agent-doc-bugs2.md claude session. Actor re-register/lease-replace likely keys on pane/session not (document_id,generation) across the shared project controller. Needs live repro; fix = scope re-association to the restarted doc only + regression. Plan: tasks/agent-doc/plan-restart-supervisor-doc-reassociation.md
- [ ] [#2y8j] [operator-verify] #qbasehashmemo live typing proof: after the supervisor auto-installs 0.34.49 and recycles, type a sustained burst in a large doc (equityfundingsource.md or agent-doc-bugs2.md) in the JB editor and confirm typing is smooth; grep ops.log to confirm crdt_merge_base no longer fires per keystroke during a typing burst.
- [ ] [#cdtcutover] [focused-cycle] #agent-doc-feature FUTURE/SEPARATE rung — wire the evented CellDocTree model into the live finalize/write path (replace whole-tree reconcile at finalize with update_to per-node deltas; expose SemTree queries to preflight/supervisor drainable computations). Large, live-supervisor-critical, gated, own focused cycle + operator live proof. Plan: tasks/agent-doc/plan-celldoctree-evented-reflection.md
- [ ] [#qeditdup] [operator-verify] Queue items duplicated + manually-struck/deleted items resurrected when the operator edits the queue during an active turn. #queuestatemachine idempotent convergence + the Live<Struck lattice (merged, 0.34.48) should resolve this — VERIFY on the recycled binary; if it persists live, a deeper editor-edit-during-cycle guard is needed. Plan: tasks/agent-doc/plan-queue-edit-race-consolidated.md
- [ ] [#crdtauth6] [operator-verify] crdtauth5 live hookup: thin JB DocumentListener calling CrdtReplicaForwarder.forwardLocalDelta + a Kotlin Unix-socket ReplicaTransport client (writes NDJSON register/deregister/update, delivers inbound update_b64 to applyRemoteUpdate); plus the deferred VS Code koffi replica seam (editors/vscode/src/native.ts out-param marshaling). Rust fan-out + Kotlin seam already merged + tested. Plan: tasks/agent-doc/plan-crdt-authority-model.md
- [ ] [#nsfz] [operator-verify] #startupdeadline live dogfood: drive a real harness that never reaches a dispatch-ready prompt (e.g. auth-walled/hung TUI) and confirm the session fails closed within 30s with a 'session did not become dispatch-ready in Ns' stderr diagnostic + a recorded startup_miss marker, instead of hanging. Code-complete + tested (make check 5101 green, release build green); needs a live editor/pane eyeball.
- [ ] [#kj5flive] [operator-verify] #mergestatemachine3 live proof replacement for cleared review #kj5f: after the latest JB plugin update is installed, verify JB+VSCode attach/detach/ownership facts flow through the merge-ownership state machine via #lazilystatesync, then run live editor-attached/detached repros for File Cache Conflict and the #6b5h wedge with logs sufficient to prove/disprove the fix before filing follow-up bugs. Plan: tasks/agent-doc/plan-merge-control-state-machine.md
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
- [/] [#provauth3b] provauth3 operator-origin half=#agent-doc-feature replace the remaining content-based recovery heuristics (content_ours / stale-editor resurrection guards) with operator-origin checks: an operator-origin working-tree change IS the authoritative prompt, never reset or treated as suspicious cleanup; content heuristics become the unknown-origin fallback only. Blocked on the live operator-origin delta signal from #crdtauth6 (origin is not known for operator edits until that seam is live). The binary-origin half shipped in 0.34.57. Plan: tasks/agent-doc/plan-operator-provenance-authority.md
- [/] [#qftloss] [operator-verify] Free-text prompt loss (mode-6) — CODE-COMPLETE on main (8b900bf4): binary now journals operator prompts from the plugin live-buffer sidecar (.agent-doc/live-buffer/<hash>) into the #qdurcrash store before any convergence/recovery can clobber the unsaved buffer, replayed if absent. Remaining: live IDE eyeball that an unsaved prompt typed during a concurrent finalize survives end-to-end. Plan: tasks/agent-doc/plan-queue-edit-race-consolidated.md
- [/] [#midturnresumeb] Phase B (focused follow-up) — actively CONSUME the #durablerecycle turn checkpoint (cycle_state.rs:486, currently written but never read to resume) on a fresh supervisor boot to re-dispatch a turn that was genuinely interrupted (child died across the recycle), idempotent vs the surviving-child path. This is the full "restart reliably restarts the turn"; Phase A only makes the wedge impossible. Also add a deferred-too-long escalation counter so a never-closing cycle cannot starve the recycle indefinitely. Plan: tasks/agent-doc/plan-midturn-recycle-resume-wedge.md
- [/] [#supstaledetect] stale_install warning false-positives in the dogfood edit->build->install->commit flow: it compared artifact mtime to the HEAD commit timestamp (300s grace), but the binary is always built BEFORE its covering commit, so an ~11min build->commit gap (binary 03:16:22 vs commit 03:27:51) flagged a FRESH binary as stale — disagreeing with the auto-install signal, which uses newest source-FILE mtime and correctly saw it fresh. FIX CODE-COMPLETE on branch supstaledetect-mtimebasis (agent-doc commit b983f144, make check green): warning now uses newest_crate_source_mtime_secs (same basis as auto-install). Remaining: merge -> install -> verify the false warning is gone AND genuine staleness still flags.
- [/] [#ipcproofnostall] DONE (code-complete, branch ipcproofnostall-fix b934263c4, make check green) — root cause: the binary-authored ipc_proof_insufficient recovery diagnostic, when #postcommit-ipc-worktree-corruption SEPARATES its structured line from the "### Re: ...interrupted-cycle recovery" heading, the bare line forms its own [user+] block and hits the closes_exchange_tail->PromptTarget fallthrough in classify_prompt_bearing_block (agent-doc-core/src/diff.rs) — the existing heading-only exemption missed it. Fix: token-specific line_is_binary_authored_ipc_proof_diagnostic (event token + invariant=/recovery= fields, or the self-description sentence) wired into the core classifier (regardless of heading adjacency) + defense-in-depth in unresolved_exchange_prompt_in_content. Non-regression tests prove a real ipc/drift-mentioning user prompt still flags. Remaining: merge -> install (bundle). This closes the false-INTERRUPT stall class.
- [/] [#suptmuxstale] DONE (code-complete, branch suptmuxstale-indicator b433bda2, make check green) — tmux pane now surfaces "⚠ STALE SUPERVISOR" in the turn-in-progress pane title (turn_status.rs) when the supervisor binary is stale, so the operator sees at a glance which sessions are stale. The turn-status hook is a separate process from the supervisor holding the binary_stale atomic, so it is bridged via an on-disk marker (.agent-doc/supervisor-stale) written by idle_watch next to the existing binary_stale.store. Pure pane_title_for_status helper + 5 unit tests. NOTE: mirrors the process_binary_is_stale PROBE (separate signal from the #supstaledetect preflight-warning fix); display-only, non-destructive. Remaining: merge -> install (bundle with #supstaledetect/#qimpstrike).
- [/] [#qimpstrike] DONE (code-complete, branch qimpstrike-fix ac466a74, make check green 5375 tests) — recurring-imperative queue heads (deploy/commit/push/build/install/release/test/sync/recycle/publish + preset tokens) were auto-struck instead of dispatched. Fix: single canonical classifier is_recurring_imperative_head (queue_continuation.rs) wired into BOTH strike sites (#qftbklgstrike lexical scorer in memory_cmd.rs AND #qheadresidue residue guard) so they skip recurring-command heads; the head stays drainable + is dispatched through the normal command path, struck only once actually executed. Non-regression test proves a genuine restatement-of-tracked-work head still strikes. Remaining: merge -> install (operator nod; bundle with #supstaledetect). Plan: tasks/agent-doc/plan-queue-recurring-imperative-head-wrongly-struck.md
- [/] [#midturn-recycle-resume] Phase A DONE — cycle-open recycle interlock: execve recycle/restart now defers every arm (auto/admin/wedge-escalation/fresh-binary-flush) while an agent-doc cycle is open or IPC is in-flight, so a recycle can no longer fire mid preflight->finalize and sever the in-flight finalize (the live_prompt_drift_after_preflight wedge root cause). Merged agent-doc main 27bf61a6, installed, supervisor recycle-marked. make check green + unit/SimWorld coverage. Remaining = operator live zero-drift eyeball: drive a recycle in the preflight->finalize window on a live editor, confirm NO live_prompt_drift_after_preflight. Plan: tasks/agent-doc/plan-midturn-recycle-resume-wedge.md
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
