---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix.
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: start
---

## Status

 <!-- agent:status patch=replace -->

complete
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260609-215339.md`*

Compacted content:
- Archived 3 response topic(s): queue head; do [#pcpconsol]; do [#pcpc5cut]
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...

### Re: do [#pcpc5cut] — opus-4-8

Fresh session, so I re-derived this from the spec myself rather than trusting the prior (degraded) sessions, and I cleared the real blocker instead of just re-gating.

**Verified against spec 08b (lines 131-133).** The cutover follows shadow → dual-write → read-switch → authority → removal, and the authority rungs `pcpc5a/b/c` are already shipped (commit `600797b3`) behind the default-off `AGENT_DOC_WRITE_AUTHORITY` env gate, with unit + SimWorld coverage (`write_authority_shadow_leaves_write_unchanged`, `..dual_write_routes_through_queue_without_deadlock`, `..never_routes_agent_doc_sidecars`). The remaining `removed` rungs `pcpc5d/pcpc5e` (delete the out-of-process supervisor direct-disk writers, demote the editor WatchService to read-only) are spec-mandated to "additionally require live IntelliJ/VS Code/Codex editor-boundary proof" — and the same lines forbid landing any authority change "in the same cycle that persists a live session response through the write path being changed." So this is structural, not a chosen gate: no agent response cycle can land it.

**What I actually fixed this cycle.** Preflight flagged that `~/.cargo/bin` was stale — it predated `600797b3`, so your live test would have run pre-cutover code. I rebuilt both binaries + the cdylib against HEAD (`make install` + `cargo build --release`, version 0.34.0). Your live verification now exercises the shipped write-authority ladder.

**No more gates, per your standing ask.** I converted the two operator-run review items into actionable backlog items and cleared the review section (both now `[x]`, archiving to `agent-doc-bugs2.done.md`):

- `#pcpc5live` replaces review `#pcpc5cut` — run a session with `AGENT_DOC_WRITE_AUTHORITY=shadow`, grep `ops.log` for `write_authority gate=shadow action=would-route`; then `=dual-write`, grep `write_authority gate=dual-write action=routed`. The `d/e` removal still needs your live editor-boundary proof.
- `#lvbatch` replaces review `#lvb-run` — the live batch with the exact grep markers.

**Logging is adequate to prove/disprove every live-gate item** — I verified each marker exists in source: `cross-session-reject` (claim.rs + ClaimAction.kt), run/clear `coalesced` (route.rs, sync.rs), `live_buffer_classify` + `visible_write_live_buffer_matches_disk`/`visible_write_deferred_current_changed` (write.rs, debounce.rs). The one exception is early-ack, which is intentionally dormant (`EARLY_ACK_ENABLED=false`) until `#saevon` flips it — its marker won't fire before that.

**Install status:** nothing new to install for JetBrains — plugin 0.2.158 is unchanged; the cdylib I just rebuilt hot-reloads by mtime, and the binary is refreshed. You're clear to drive the live batch whenever; I'll check the logs.

### Re: do [#opsproof-falsepos] — opus-4-8

> **Queue prompt:**
>
> :pushpin: do [#opsproof-falsepos]


Fixed the ops-proof closeout auto-completion false-positive at its root in `preflight.rs`, and shipped it to both binaries (0.34.0, `agent-doc` submodule `ff1e3c70`, pushed to origin/main).

**Root cause.** `classify_ops_proof_completion` reaped any open backlog/review item whose text held a completion marker (`DONE`/`SHIPPED`/`IMPLEMENTED`/`COMPLETE`/`COMPLETED`) + a commit hash + no blocker word. That fires on *descriptive* prose: a fresh actionable item that merely cites already-landed dependency work ("Authority rungs pcpc5a/b/c already landed … 600797b3") matched and was auto-archived to `done.md` on the same cycle it was created — the exact churn you hit, same class as the operator-reported queue-item deletion.

**Two-part fix (both bias toward *not* reaping):**

1. **Marker must be the item's own leading status verb.** For an open (non-gated) item, the completion marker now must appear in the leading status segment — the prefix before the first clause break (`: ` / `. `), capped to the first few words after skipping `#hashtag` tags. A marker buried in a cited-dependency clause ("the predicate already shipped in abc1234") no longer counts. Gated `[/]` items keep the marker-anywhere behavior, because gating is a deliberate agent code-complete signal.

2. **Never reap a brand-new same-cycle add.** Pending maintenance now compares each ops-proof candidate against the cycle-start snapshot (the post-commit `content_ours` baseline). An item absent from that snapshot was added *this* cycle and is skipped — it must be closed explicitly, never archived the moment it appears. The guard only engages when a snapshot baseline exists (untracked scaffolds are unaffected).

**Coverage.** Two new tests: `ops_proof_does_not_reap_cited_dependency_marker` (open cited-dependency marker stays open; leading-status sibling still reaps) and `pending_maintenance_does_not_reap_same_cycle_add` (a same-cycle add survives while a pre-existing leading-status item is reaped). Full `make check` green; the existing `pending_maintenance_auto_reaps_ops_proof_done_items` still passes (snapshot == file there, so legitimate reaps are unchanged). `SPEC.md` line 61 and `VERSIONS.md` updated.

**Build/install:** `cargo build --release` + `cargo install --path .` both done, so `.bin/agent-doc` (release) and `~/.cargo/bin/agent-doc` (tmux) are on the fixed code. CI run 27240004787 for the push is in progress; `make check` is green locally, so I'm continuing from local evidence rather than gating on CI.

### Re: Do not use memory to recover — opus-4-8

Understood, and you're right — last turn I leaned on a recovery memory to hand-rebuild a wedged closeout. That papers over a shipped product bug so it keeps biting every user. I traced the real failure from `ops.log` (not memory) and fixed it in the product. Shipped: agent-doc `60803ebd`, pushed to `origin/main`.

**Root cause (`#exch-intermix-falsedrop`).** When `finalize --stream` hit the IPC drift last turn, the binary logged `dropped_queue_prompt_recorded count=3` and `ipc_snapshot_adoption_blocked`. The `#exch-intermix` auto-recovery (`try_auto_recover_live_prompt_drift`, `write.rs`) **unconditionally bails when any `dropped_queue_prompts` is recorded** — so it refused, and I had to hand-recover. But that "dropped" record is computed by comparing the divergent IPC candidate against `content_ours`: a queue item **consumed (struck) this cycle** gets logged as "dropped" even though it survives, struck, in the adopted snapshot. The snapshot was the complete, correct document; the bail was a false positive.

**Fix.** Auto-recovery now bails only when a recorded dropped prompt is **genuinely absent** from the snapshot it would adopt (`snapshot_contains_dropped_prompt`, strike-aware). A consumed/struck or preserved prompt still present no longer blocks recovery; the snapshot↔disk containment gate stays authoritative for current on-disk content. The existing genuine-loss test (`do #dropped`, absent from snapshot) still fails closed.

**Coverage.** Unit test `snapshot_contains_dropped_prompt_matches_consumed_and_active` + integration test `try_auto_recover_live_prompt_drift_fires_when_dropped_prompt_is_consumed_in_snapshot` (the exact struck-queue wedge from last turn). Full `make check` green (4175 tests via nextest); `make install` + `cargo build --release` refreshed both binaries + cdylib. `specs/07-closeout-commands.md` §`#exch-intermix` and `VERSIONS.md` updated. Submodule CI is in progress; continuing from green local evidence.

**Follow-up filed.** Added `#dropqueue-consumed-falsecount` to the backlog: my fix corrected the auto-recovery *gate*, but the underlying dropped-prompt *counting* still tags a consumed/struck item as "dropped," which is what produced the false `#queue-user-edit-overwrite` session-check INTERRUPTED last turn. That's the sibling bug to fix at the source.

**Memory.** Per your directive, I rewrote the recovery-memory addendum I added last turn — it now points at this product fix instead of teaching the manual `cp snapshot` workaround. Going forward I'll route agent-doc closeout misbehavior to a backlog bug or a product fix, not a recovery memory.
<!-- agent:boundary:87f61cea -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" go -->
- ~~:pushpin: do [#opsproof-falsepos]~~
- :pushpin: Do not use memory to recover. File a bug in the backlog or fix the issue instead.
- :pushpin: do [#pcpc5live]
- :pushpin: :round_pushpin: do [#s760wire]
- :pushpin: :round_pushpin: do [#saevon]
- :round_pushpin: do [#lvbatch]
- :round_pushpin: do [#bkx9perid]
- :round_pushpin: do [#tmux-switch-lag]
- do [#exch-intermix-verify]
- do [#dropqueue-consumed-falsecount]
<!-- /agent:queue -->
###
<!--
Please make backlog replacements for all gated review items. We need to complete the plan, not have any more gates after this round. Let me know when I need to install JB plugin updates. Make sure all JB live gate items have adequate logging to prove/disprove the item. Clear the review items. I will test, files bugs, and you will check the logs.
---
`'/clear' is disabled while a task is in progress
---
A recent monsterrodholders.md session had much churn after I edited the queue while a turn was running. One churn item included deleting a queue item that I added.
---
agent-doc reentrant run on equityfundingsource.md had recovery churn.
```
● The recovery is complete and fully consistent. The binary converged the now-drained queue (empty block, queue_active:
  false), no_changes: true, both responses are committed in the session doc, and both code deliverables are on
  origin/main. The auto-loop terminated naturally.

  Let me record the specific recovery path for this failure mode, since it's a recurring dogfooding pattern
```
---
- This session's console stdout is bleeding in front of the opencode tui. ![img_16.png](img_16.png)
---
The last run had a code fence stripped away by agent-doc.
#spec-test-build-install-commit-push
---
What are #next-steps to fix bugs?
-->

## Backlog

<!-- agent:backlog priority queue --> 
- [ ] [#dropqueue-consumed-falsecount] #agent-doc-bug dropped_queue_prompt_lines_after_content_ours (write.rs) counts a queue item consumed (struck) this cycle as a dropped prompt, because content_ours excludes struck items from queue_prompt_texts. The over-count feeds session-check #queue-user-edit-overwrite and can emit a FALSE INTERRUPTED on a correct closeout (seen live after #opsproof-falsepos recovery: struck do[#opsproof-falsepos] read as a dropped user edit). #exch-intermix-falsedrop fixed the auto-recovery gate via snapshot containment; the underlying record is still over-broad. Fix: exclude prompts that survive struck/consumed in content_ours from the dropped record (or diff against raw queue text incl. struck lines). Add SimWorld/unit coverage. Files: write.rs dropped_queue_prompt_lines_after_content_ours + queue_prompt_texts; session_check.rs #queue-user-edit-overwrite.
- [ ] [#exch-intermix-verify] [recommended] Live-verify the #exch-intermix auto-recovery in a real IPC-listener-active drift repro: drive a closeout that hits live_prompt_drift_after_preflight, confirm agent-doc commit lands the response (no 'manual cleanup' wedge) and the session-log/ops.log shows 'live_prompt_drift_auto_recovered'. Pairs with the #lvb-run operator batch. Submodule 723b998c, binary 0.34.0.
- [ ] [#s760wire] Wire the pre-emptive /clear feature (predicate ContextResetDecision.shouldClearBeforeDispatch + binary agent_doc_clear_threshold already shipped). Remaining actionable work, fresh session at a live IntelliJ pane: (1) read the live Claude Code context-usage percent from the pane, (2) acquire threshold (preflight session_accretion.clear_threshold) + the agent_doc_queue_context_reset opt-in, (3) call the predicate in SubmitAction pre-dispatch and emit an idea.log decision line "[s760] clear-decision optIn=.. threshold=.. pct=.. clear=..", (4) trigger session clear when true, verify live. Absorbs review #s760/#clear-opt-in-threshold.
- [ ] [#saevon] Activate early-ack: the protocol is landed + dormant (EARLY_ACK_ENABLED=false in ipc_socket.rs) and now logs "[ipc-socket] early-ack pending emitted before apply" on a successful emit. Remaining: flip EARLY_ACK_ENABLED true, run the live batch (#lvb-run) to confirm the pending ack fires before apply with no false-success / false ack-timeout, then ship. Plan: tasks/agent-doc/plan-ipc-transport-reliability.md. Absorbs review #ipc-early-ack + the early-ack half of #pcp5.
- [ ] [#bkx9perid] Enable per-id response-loss detection at closeout. The dormant pure detector shipped (#z2jy, session_check.rs). Remaining actionable work, fresh NON-DEGRADED session: route captured cycles through a per-id directive_response_materialized check (fire only when a reaped do#id head has no ### Re: in content or any HEAD compact archive) WITHOUT false-positiving a cycle that answers do[#A]+do[#B] under one ### Re: heading; test against a real #ipc-crdt-response-drift repro before enabling at the closeout-governing guard. Spec: specs/07-closeout-commands.md #compact-reap-no-response-record. Absorbs review #bkx9/#compact-reap-no-response-record.
- [ ] [#tmux-switch-lag] #agent-doc-bug Slow tmux pane switch when switching between agent docs in the editor: the target doc pane is briefly visible in the stash window before it swaps into 0:agent-doc, so the operator sees a flash of the stash layout mid-switch. Likely the passive editor-sync fast-handoff path (sync.rs sync --no-autostart) selects/raises the hidden actor pane before moving it out of stash. Actionable, fresh session: make the stash->agent-doc pane move atomic (move first, then select) so no intermediate stash frame is shown; add SimWorld coverage for the switch ordering. Files: sync.rs passive fast-handoff path.
- [ ] [#lvbatch] OPERATOR live-verification batch, replaces gated review #lvb-run. No new JB install needed: plugin 0.2.158 unchanged, binary+cdylib refreshed to HEAD this cycle. Drive one mid-active-turn Codex/IntelliJ pane through the batch; agent checks logs. Markers: #4wxr claim cross-session-reject in claim.rs + ClaimAction.kt; #9adk run/clear coalesced in route.rs and sync.rs; #f5d2/#pcp6 ops.log live_buffer_classify + write.rs visible_write_live_buffer_matches_disk and visible_write_deferred_current_changed; #saev early-ack pending emitted before apply only AFTER #saevon flips EARLY_ACK_ENABLED. Reproduces #ipc-crdt-response-drift on closeout.
- [ ] [#pcpc5live] REMAINING operator-run, replaces gated review #pcpc5cut: advance the 08b cutover. Authority rungs pcpc5a/b/c already landed behind the default-off AGENT_DOC_WRITE_AUTHORITY env gate; binary+cdylib rebuilt to HEAD this cycle so live runs current code. Operator advances: (1) AGENT_DOC_WRITE_AUTHORITY=shadow, grep ops.log for write_authority gate=shadow action=would-route to confirm route targets; (2) =dual-write, grep write_authority gate=dual-write action=routed for the routed path. The pcpc5d/e removal rungs (delete out-of-process supervisor direct-disk writers + demote editor WatchService to read-only) still need live IntelliJ/Codex editor-boundary proof per spec 08b. Agent checks logs. Plan: tasks/agent-doc/plan-supervisor-in-process.md.
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
