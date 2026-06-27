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
queue: stop
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

### Re: drain #qheadstrikeauto — free-text drain target now auto-strikes off the 🚧 marker, not agent prose (v0.34.59) — opus-4-8

Drained **#qheadstrikeauto** — the most self-contained non-blocked id in the priority head (`#pcwcrt` is blocked on `#crdtauth6`, `#syncbarrier` is large, `#suprestassoc` needs a live repro, `#actorprune` shipped last cycle).

**Shipped (`07403e17`, v0.34.59, `make check` green / 3 new tests):**
- **Root cause:** the free-text strike only fired when the committed response *quoted* the head as a `> **Queue prompt:**` blockquote. `free_text_head_answered_by_response` (`queue_consume.rs`) matches a head only against `response_blockquote_text(response_body)` — the response's blockquote region — so a head answered in **plain prose** was never struck. Result: the operator had to manually quote every free-text head, and a plainly-answered head churned no-op `#qchurn` loop cycles until cleared by hand. Operator: *"the binary should do this automatically...not the agent."*
- **Fix:** the binary already stamps the cycle's **drain target** with the in-progress `🚧` marker during preflight queue maintenance (`set_first_prompt_in_progress`). A **free-text** head carrying that marker IS the head this cycle was dispatched to drain, so on a committed (non-empty) response it is answered *by definition* — no agent prose required. New `head_carries_in_progress_marker` helper (cosmetic-marker-aware via the existing `strip_priority_markers`/`display_queue_prompt_text` path); `answered_free_text_head_node_keys` now strikes a head when it is the drain-target marker head **OR** the response prose-matches it (the secondary `#ftstrike` signal that still strikes non-marker free-text heads answered at any position).
- **Safety preserved:** the `#qstrikeexplain` Phase-2 baseline gate still defers a `🚧` head that first appeared this turn (in-flight operator edit, never same-cycle struck); P4 still routes id-backed marker heads through `--done` (the free-text pass only ever strikes free-text marker heads).
- **Coverage:** `marker_head_struck_without_blockquote_quote_qheadstrikeauto` (plain-prose response strikes the marker head; precondition asserts the legacy prose-match returns false), `marker_head_not_struck_when_absent_from_baseline_qheadstrikeauto`, `id_backed_marker_head_not_struck_by_free_text_pass_qheadstrikeauto`.

**Deployed (agent-doable steps, `#deploy-just-do-it`):** version bumped (all workspace crates + `pyproject.toml` → 0.34.59), `VERSIONS.md` entry, `make check` green, `make install-full` (0.34.59 live, lib-install auto-recycle marked 2 controllers for the next idle boundary), committed + pushed (submodule `07403e17`, tag `v0.34.59`, superproject pointer `c1ff6ff5e`). Recycling the supervisor onto 0.34.59 next. **Not** published to crates.io/PyPI — the 0.34.x dogfood cadence batches publishing (PyPI is still 0.33.3); that stays a separate operator step.

**Operator-verify (live eyeball):** after the supervisor recycles onto 0.34.59, drain a free-text queue head with a **plain-prose** response (no `> **Queue prompt:**` blockquote) and confirm the head auto-strikes (ops.log `freetext_head_strike` / `free_text_head_struck note=auto_struck_answered`) with `queue_drainable_head_count` dropping — no manual quoting needed.

Remaining drainable heads: `#syncbarrier` (large), `#suprestassoc` (live repro), the JB-flood dispatch bug, and the free-text reports (deleted-items-reappear / no-dup-adds — already closed by the 0.34.x tombstone+dedup).

### Re: drain #pcwcrt — CRDT no longer reverts operator marker/queue edits (#qmarkerauth + post-commit revert tower removed, v0.34.61) — opus-4-8

Recovered the crashed run (tmux session died mid-cycle) and shipped the operator's #S9208 report ("agent-doc deletes attributes added to `agent:queue` / prevent CRDT from reverting operator changes"). The crashed run had built the fix in the submodule but never finished `make check` (one budget-guard test was red), bumped, installed, committed, or recorded the closeout — all done now. Two coherent fixes:

**`#qmarkerauth` — operator marker-attribute edits survive merge (`agent-doc-core/src/crdt.rs` + `cell_doc.rs`):** adding an attribute / `go` activation to a component marker (`<!-- agent:queue priority go -->`) while a turn was running was silently deleted on the next merge. Root cause: `reconcile_component` and the cell-merge component branch both reframed the merged body with `ours_open`/`ours_close` unconditionally, discarding the operator's `theirs` marker. Fix: new `reconcile_marker_operator_authoritative` 3-way merges the marker line — operator (`theirs`) wins when it differs from base, an agent change is honored only when the operator left the marker at base, no base epoch ⇒ operator authoritative. Marker analogue of the field-wise frontmatter merge (`#fmreset`) and the list-order theirs-spine (`#qauthorder`). Coverage: `merge_by_component_preserves_operator_marker_attribute_qmarkerauth`, `reconcile_marker_operator_authoritative_resolution`, + cell-merge equivalents.

**`#pcwcrt` / `#realtimecutover` — post-commit revert tower ripped out (`agent-doc-orchestration/src/git.rs`):** the legacy `postcommit_worktree_lost_committed_content` / `send_postcommit_editor_refresh` tower silently reverted the working tree back to HEAD (or pushed a stale-snapshot-derived buffer to the editor) whenever it judged committed content "lost" — the recurring `#postcommit-ipc-worktree-corruption` source that clobbered legitimate operator edits. Replaced with **observe-only** handling: it logs `postcommit_worktree_check match=false`, preserves any carry-forward superset, and **never** reverts an operator working-tree edit (the realtime replica owns disk reconciliation). ~955 lines removed; the FlowCore `git.rs` `reason=` token budget re-audited 20 → 7 in `tests/test_cli.rs`. Coverage: `postcommit_worktree_observe_only_never_reverts_lost_content`, `postcommit_worktree_preserves_carry_forward_superset`.

This very session was a **live repro**: the crashed run left the `agent:queue` working tree with `:pushpin:`-mangled + triplicated `do [#…]` rows (operator edits reverted/duplicated by the 0.34.60 supervisor). Recovered the doc to clean HEAD; on 0.34.61 the supervisor no longer reverts those edits.

**Shipped (v0.34.61, `make check` green / 5439 tests):** version bumped (all workspace crates + `pyproject.toml`), `VERSIONS.md` entry, `make install-full` (0.34.61 live, cdylib `libagent_doc-0.34.61.so` + auto-recycle marked controllers), committed + pushed (submodule `49f7d5da`, tag `v0.34.61`, superproject pointer `1bf94aab6`), supervisor recycle marked onto 0.34.61. **Not** published to crates.io/PyPI (0.34.x dogfood cadence batches publishing — separate operator step).

**`[operator-verify]` (live eyeball, not agent-completable):** on the recycled 0.34.61 supervisor, add an attribute to the `agent:queue` marker mid-turn and confirm it is NOT deleted on the next merge; and edit/delete a queue row during an active turn and confirm it is NOT reverted/duplicated (ops.log shows `postcommit_worktree_check match=false` observe-only, no revert). Striking `#pcwcrt`; `#qmarkerauth` recorded as shipped.

### Re: #closeoutstall queued under stopped queue — gpt-5

I picked up the Codex handoff. The document now records `agent: codex`, and `#closeoutstall` is present as the top backlog/queue item with its plan at `tasks/agent-doc/plan-closeout-editor-convergence-stall.md`.

I did not start implementation in this cycle because preflight/plan classified the change as a prompt-response cycle and the document frontmatter still says `queue: stop`; inactive queue residue is context, not an executable drain command. No code paths changed and no tests were run.

Preflight also surfaced semantic-completion warnings for unrelated backlog/review items; I left those untouched.

### Re: queue activation and duplicate do rows — gpt-5

I processed the current document edit through preflight + plan. The document is now in `queue: start` with `#closeoutstall` recorded as the in-progress queue head and the existing backlog item/plan at `tasks/agent-doc/plan-closeout-editor-convergence-stall.md`.

I did not start code work in this cycle because `agent-doc plan` classified this diff as a prompt-response / queue-state reconciliation and emitted no `repo_actions`; its only required command is this finalize closeout. The duplicated `do #60ev` / `do #tmuxsynccrash` rows stay open because they were not selected as this cycle's executable repo action and preflight reported no agent-drainable continuation head.

No source files changed, and no tests were run. Preflight still surfaced semantic-completion warnings for unrelated review/backlog items; I left those untouched. Session accretion is at warn due prior no-op/restart churn, but the queue remains active and no compact handoff was requested.
<!-- agent:boundary:9cb24d70 -->
<!-- /agent:exchange -->
## Queue


<!-- agent:queue priority preset="#spec-test-build-install-commit-push" priority -->
- ~~[#f693] [#q9w3] [#fpyh] [#818s] [#pcwcrt]~~
- [#syncbarrier] [#qheadstrikeauto] [#actorprune] [#suprestassoc]
- [#2y8j] [#cdtcutover] [#qeditdup] [#crdtauth6] [#nsfz] [#kj5flive]
- I deleted items from the queue but they reappeared
- When auto-adding queue items, verify that the item is not already in the queue. Do not add duplicates.
- JB `Run Agent Doc` flooded the prompt with duplicate `agent-doc .../agent-doc-bugs2.md` dispatch prompts. Please fix the root cause.
- ~~do [#f693]~~
- ~~do [#fpyh]~~
- ~~do [#818s]~~
- ~~do [#actorprune]~~
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
- [ ] [#closeoutstall] #agent-doc-bug Direct Codex closeout can stall when a proven live editor does not ACK response IPC: write --commit leaves the response only in .agent-doc/patches with recovery=retry_without_disk_write, doctor says editor_convergence operator, repair says clean, and the turn can end without a durable typed handoff. Add first-class EditorConvergenceRequired blocked-closeout state, retry command/session-check diagnostics, and Codex Stop-hook handling. Plan: tasks/agent-doc/plan-closeout-editor-convergence-stall.md
- [ ] [#60ev] plan-dynamic-context-injection-ledger.md
- [ ] [#tmuxsynccrash] #agent-doc-bug HIGH severity: JB Sync Tmux Layout crashes the tmux SERVER (all sessions/panes die; route::auto_start then respawns tmux new-session -d -s 0, which the operator sees as the crash). Operator repro 2026-06-26: ran Sync Tmux Layout twice, tmux crashed both times. Root cause from idea.log/ops.log: full-mode agent-doc sync runs doctor-backed repair_layout (agent-doc-orchestration/src/sync.rs:849) whose fast-path skip (has_target && stash_count==1 && has_exact_stash) never converges after its own repair (logs ping-pong target=true/stash=0 <-> target=false/stash=1), so every sync re-runs destructive move-window/swap-window/join-pane/resize-window -y 1000 churn; JB shouldRerunAfterSupersede loop + automatic editor-tab sync amplify into a rapid storm that crashes tmux 3.7. Server pid proof: current tmux STARTED 23:54:47 mid-storm. Fix: (1) make repair_layout converge (stash_count==0 + correct target = no-op), (2) supersede re-run dispatches passive --no-autostart (no doctor repair), (3) rate-limit destructive repair per session. Needs SimWorld + make tmux-ci + operator live tmux-pid-stable proof. Plan: tasks/agent-doc/plan-sync-tmux-layout-crash.md
- [ ] [#syncbarrier] #agent-doc-feature Phase 1 (editor-sync barrier) — epoch handshake so the binary DEFERS dangerous ops (snapshot reset / disk reconcile / merge-base rebuild) while an editor edit is in flight, bounded wait, fail-OPEN to the editor buffer (never a held lock, never snapshot-discard). Poll-check = yrs StateVector exchange, not text compare. Needs JB/VSCode edit_epoch + StateVector over IPC (extend State method). Phase 0 #ipctruncrecover shipped (0.34.53). Plan: tasks/agent-doc/plan-editor-sync-barrier.md
- [ ] [#qheadstrikeauto] #agent-doc-feature auto-strike answered free-text head WITHOUT requiring the agent to quote it as a > **Queue prompt:** blockquote — operator: "the binary should do this automatically...not the agent". Today strike_answered_free_text_queue_heads (queue_consume.rs) only fires on a blockquote/prose-prefix match, so a head answered in plain prose is never struck. Key the strike off the binary drain-target / in-progress (the marker) head identity for the cycle instead of agent prose formatting. Plan: tasks/agent-doc/plan-freetext-queue-head-autostrike.md
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
- [/] [#suptmuxstale] DONE (code-complete, branch suptmuxstale-indicator b433bda2, make check green) — tmux pane now surfaces "⚠ STALE SUPERVISOR" in the turn-in-progress pane title (turn_status.rs) when the supervisor binary is stale, so the operator sees at a glance which sessions are stale. The turn-status hook is a separate process from the supervisor holding the binary_stale atomic, so it is bridged via an on-disk marker (.agent-doc/supervisor-stale) written by idle_watch next to the existing binary_stale.store. Pure pane_title_for_status helper + 5 unit tests. NOTE: mirrors the process_binary_is_stale PROBE (separate signal from the #supstaledetect preflight-warning fix); display-only, non-destructive. Remaining: merge -> install (bundle with #supstaledetect/#qimpstrike).
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
