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

❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260625-053644.md`*

Compacted content:

### Re: #nsq2 JB mirror eviction on close — root cause of cross-doc symptom — glm-5-2

**`#nsq2` (#jbmirrorevict) — investigated, fixed, shipped + committed (`56f093c3`).** Confirmed the root cause of the "sessions working on other open documents" symptom and fixed it.

**Root cause:** `StateProjectionBridge` holds `mirrors` and `generations` as `ConcurrentHashMap<String, ...>` keyed by `documentHash` (SHA-256 of canonical path). Both maps used `computeIfAbsent` adds with **no eviction ever** — they grew monotonically over the IDE lifetime. A reused path (move/symlink/close-reopen) inherited the prior document's stale mirror + `actor_generation` counters, surfacing cross-document state. (lazily-rs contexts were already confirmed per-document, so this was the leak.)

**Fix:**
- `StateProjectionBridge.evictForFile(path)` — removes the mirror + all `"$docHash:*"` generation entries.
- `EditorTabSyncListener.fileClosed` — evicts on `.md` tab close. Re-subscription lazily re-creates the mirror from a fresh cold snapshot, so aggressive eviction is safe (a live editor in another split just re-fetches).

10 `StateProjectionBridgeTest` tests green incl. new eviction coverage (`evictForFile clears owner-generation counters and restarts fresh`). Plugin **0.2.190 → 0.2.191** (structural: new methods). Signed zip built at `editors/jetbrains/build/distributions/agent-doc-jetbrains-0.2.191-signed.zip`.

**Operator action:** uninstall → restart → install → restart (structural class change — JB caches bytecode on reinstall).
- Archived 9 response topic(s): #n529 JB reactive state mirror via FFI deltas; cycle closeout (IPC wedge recovery); IPC proof diagnostic (interrupted-cycle recovery); 6 more
- Prior summary/context: ❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac...

### Re: IPC proof diagnostic (interrupted-cycle recovery) — agent-doc

```text
**IPC proof issue dogfood log**
Appended automatically during interrupted-cycle recovery to record the editor IPC issue.
This is binary-authored diagnostic content, not a user prompt, so it does not require a separate response cycle.
Issue class: `ipc_proof_insufficient`
Affected component: editor IPC / writeback

ipc_proof_insufficient file=tasks/agent-doc/agent-doc-bugs2.md source=socket_ack_content patch_id=389b42c4-39d7-4783-8d43-fd6f1dace5d6 invariant=live_prompt_drift_after_preflight recovery=content_ours_snapshot_next_cycle snap_source=ack_content_sidecar candidate_len=26507 candidate_hash=7c80b60cd19df366ffdf926683a956b6744bdea9722b78e71b4fb1a98feb4ff9 content_ours_len=27760 content_ours_hash=e301451aceffd5dddeb58965306908b021966ef1f37a9ca040b0072aacfe96d7 doc=agent-doc-bugs2 session=b26b9957-8de6-4eae-bb98-5d7fe4d6b781 turn=cycle-1782364110435
```

### Re: explicit id should be used when adding a backlog item — opus-4-8

> **Queue prompt:** When adding a backlog item, the agent sometimes assigns an explicit id. But agent-doc ignores the explicit id and assigns a new id. The agent's explicit id should be used instead. So instead of `[#kj5f] #mergestatemachine3` the id should be `[#mergestatemachine3]`.

**Fixed, tested, committed + pushed** (`#qexplicitid`, agent-doc `9fd0addc`; submodule pointer `7cf02962e`). Harness switched `opencode → claude` this cycle; preflight resolved it cleanly (no `harness_mismatch`), so Claude is now driving the drain.

**What was wrong:** a leading bare `#tag` in `--pending-add` text was never promoted to the item id — only bracketed `[#tag]` / `id=tag` prefixes were. So `#mergestatemachine3 JB+VSCode report…` got a generated hash (`#kj5f`) with the tag left dangling in the text. The fix (`leading_bare_tag_id` in `agent-doc-core/src/pending.rs`) promotes a clean leading `#tag` followed by whitespace + more text:

- `#mergestatemachine3 JB+VSCode report…` → `[#mergestatemachine3] JB+VSCode report…` ✅
- `#lazilyspecpin=Pin …` → keeps an auto hash (trailing `=` disqualifies — compound topic labels are not id requests)
- bare `#tag` alone on a line → left untouched (a reference, not an id request)
- collision with an existing id → rejected (`inline tag id already exists`)

Wired into both the add path (`op_add_at_with_outcome`) and explicit-id collision enforcement (`explicit_custom_id`). Added 4 unit tests; `make check` green (5132 passed, clippy clean); CI latest run green.

**Note:** this only affects *future* adds. The existing `[#kj5f] #mergestatemachine3` backlog head keeps its hash — I'm not renaming it live (the IPC supervisor would race a manual id rewrite). It'll be picked up as `[#mergestatemachine3]` next time an agent re-adds that item. The new binary goes live via the supervisor's boundary auto-install — no manual `cargo install` this session (avoids finalize-client/supervisor drift).

### Re: IPC proof diagnostic (interrupted-cycle recovery) — agent-doc

```text
**IPC proof issue dogfood log**
Appended automatically during interrupted-cycle recovery to record the editor IPC issue.
This is binary-authored diagnostic content, not a user prompt, so it does not require a separate response cycle.
Issue class: `ipc_proof_insufficient`
Affected component: editor IPC / writeback

ipc_proof_insufficient file=tasks/agent-doc/agent-doc-bugs2.md source=socket_ack_content patch_id=f7da83e9-290b-4dd4-b8c3-4cb9fdd9fd58 invariant=live_prompt_drift_after_preflight recovery=content_ours_snapshot_next_cycle snap_source=ack_content_sidecar candidate_len=22228 candidate_hash=be0e40bd37ff08e494f8eaf87a5c4de55487b5889d715c7e3c859936c50ef042 content_ours_len=23893 content_ours_hash=cd84d474abb1a63c87edb953185e197cce4d5cff3f9bbbdbb977acc9ea67424b doc=agent-doc-bugs2 session=b26b9957-8de6-4eae-bb98-5d7fe4d6b781 turn=cycle-1782368592491
```

### Re: #kj5f #mergestatemachine3 — wedge-class scenario repros shipped — opus-4-8

Drained the queue head `#kj5f` (`#mergestatemachine3`, the merge-control state-machine phase 3). Completed and shipped the **agent-doable, deterministic half**: end-to-end scenario repros that prove the write-ownership SM eliminates the two failure classes the operator reported (`File Cache Conflict` + the `#6b5h` no-ack wedge).

**Shipped** (agent-doc `34d7023d`, submodule pointer `8cdf54beb`/`b74b34323`):
- `file_cache_conflict_repro_never_writes_disk_behind_live_editor` — walks the full editor handshake (Detached → Attached → EditorOwnsBuffer → BinaryWriteRequested → IpcAckProven → Committed) and asserts `disk_write_permitted` is **false at every phase a live editor could be mid-apply**, true only once the patch is ACK-proven. A regression that permits a mid-handshake write is now caught as the File-Cache-Conflict reproduction it would cause.
- `stale_listener_6b5h_wedge_repro_routes_to_disk_instead_of_no_ack` — a registered-but-dead listener demotes to `Detached` on a stale heartbeat, so the write routes to the controller disk path instead of wedging on `no_ack` forever.
- `idle_editor_survives_stale_heartbeat_no_false_disk_write` — inverse safety: a live idle editor (stale heartbeat, live pid) is never demoted, so the binary never raw-writes behind it.

`make check` green (5135 passed, +3; clippy clean); latest CI run green.

**Remaining on `#kj5f` (operator-gated, not agent-completable this session):** the JB + VSCode **thin reporters** that emit attach/detach/ownership facts through the SM (via `#lazilystatesync`), plus the **live editor-attached/detached repro**. Both need a live editor and the plugin install that's currently paused (per your "stop cutting releases until the CRDT merge corruption is fixed" directive), so they can't be verified from a CLI session. I attempted to retag `#kj5f` `[operator-verify]` to reflect this, but the queue-consume reconciliation is wedged on a stale-snapshot drift (the prior cycle's struck explicit-id head is in the snapshot but not the live queue) — I left the backlog untouched rather than fight it against the live supervisor.

**Closeout note:** this session's editor IPC remains degraded — every write mangles the working tree post-commit (recovered via `git checkout HEAD --`), and the queue-edit path is wedged on snapshot drift. The code is safe on `origin/main`. Stopping here rather than auto-looping into more degraded-IPC churn; the supervisor can reconcile the snapshot at its idle boundary.

### Re: IPC proof diagnostic (interrupted-cycle recovery) — agent-doc

```text
**IPC proof issue dogfood log**
Appended automatically during interrupted-cycle recovery to record the editor IPC issue.
This is binary-authored diagnostic content, not a user prompt, so it does not require a separate response cycle.
Issue class: `ipc_proof_insufficient`
Affected component: editor IPC / writeback

ipc_proof_insufficient file=tasks/agent-doc/agent-doc-bugs2.md source=socket_ack_content patch_id=17d49729-8f3d-4c19-9863-eae235a8abc7 invariant=live_prompt_drift_after_preflight recovery=content_ours_snapshot_next_cycle snap_source=ack_content_sidecar candidate_len=25858 candidate_hash=e57fc4f2504133c72064e14aa2419ad96b6a8d0f149898cb8c70d7d36ff48115 content_ours_len=27489 content_ours_hash=15d4e896e9e30ebaafb13017d7cb10fcd12f478ca0d466076a544d465e8ea7cc doc=agent-doc-bugs2 session=b26b9957-8de6-4eae-bb98-5d7fe4d6b781 turn=cycle-1782369643340
```

### Re: agent switch isn't reactive + caused IPC drift — opus-4-8

> **Queue prompt:** JB `Run Agent Doc` on equityfundingsource.md after switching from codex to opencode … any agent change has this issue. I have to kill + restart the tmux pane or recycle the supervisor for the opencode state to pick up. The supervisor should reactively pick up the agent state change. This also caused IPC drift … as seen in a recent monsterrodholders.md session.

**Your diagnosis is correct, and the two symptoms share one root cause: a paused/stale parent route-owned supervisor.**

**The reactive restart mechanism IS shipped** (`#agentreloadrestart` Phase 1a+1b, confirmed in `agent-doc-orchestration/src/start/idle_watch.rs:719-771`): on any `agent:` harness change, route refuses to cold-kill the healthy old pane (the kill-guard, intentional) and **defers to a boundary restart**. The supervisor idle-watch is supposed to pick that up — each tick it re-resolves the harness and, when `agent_change_restart_decision` returns `Restart` (`harness_changed && enabled && prompt_visible && !turn_active`), emits `harness_change_detected` → `agent_restart_triggered` → `agent_restart_performed old=codex new=opencode`. That's the "reactive pickup" you're asking for — it exists.

**Why it dead-ends for you (and why you must manually kill/restart):** the idle-watch only fires at a `prompt_visible && !turn_active` boundary. A **paused or stale supervisor never ticks it**, so `harness_change_detected` is never emitted and the restart never fires. This session is in exactly that state (`queue_paused`, stale in-memory CRDT).

**The IPC drift is the same supervisor.** Every finalize this session hits `live_prompt_drift_after_preflight` + post-commit working-tree corruption (I've recovered it with `git checkout HEAD --` each cycle). That's the **same paused/stale parent route-owned supervisor replaying stale in-memory CRDT** — not a stale binary (the running build matches source), and not the IDE. So "agent switch won't pick up" and "IPC drift" are one failure, exactly as you observed across equityfundingsource / monsterrodholders / this doc.

**Recovery (shipped — `#actorswitchdefer` Part A, `6c7455a8`):** the route defer bail is now operator-actionable and points at the fix. For this session, run:

```
agent-doc session restart-supervisor tasks/agent-doc/agent-doc-bugs2.md
```

(or kill+restart the pane / recycle the IDE). That re-resolves the harness, brings up the new agent, **and** clears the stale-CRDT drift in one shot. `restart-supervisor` already re-resolves frontmatter, so it's the single recovery for every branch.

**Remaining work** is `#actorswitchdefer` Part B (already queued, `[focused-cycle]`): deterministic SimWorld proof that the deferred restart actually fires at the boundary, the paused/stuck regression asserting the recovery hint shows and the deferred restart is never silently dropped, plus an explicit route bail when `agent_change_restart` is disabled. That needs restart-flow SimWorld infra + `make tmux-ci`, so it belongs in a fresh focused cycle on a healthy supervisor — implementing it on this stale-CRDT session would just re-corrupt the write.

**I'm stopping after this and recommending the supervisor restart above** — the in-session loop can't fix its own parent; the operator restart is the unblock.

### Re: queue bookkeeping — #actorswitchdefer pinned, supervisor recovered — opus-4-8

Read your queue edit: marked the prior response `(HEAD)`, moved `[#actorswitchdefer]` to the top, and dropped its `do` prefix. Treating that exactly as written — it's now a **visible pin, not an executing directive**, so I will not auto-drain it. Correct call: `#actorswitchdefer` Part B is `[focused-cycle]` (needs restart-flow SimWorld infra + `make tmux-ci`), so it belongs in a fresh healthy cycle, not this one. Part A already shipped (`6c7455a8`).

**Supervisor has recovered.** The paused/stale parent supervisor the HEAD response diagnosed is gone: `agent-doc admin inspect` now reports `state=ready queue_control=none freshness=controller:fresh,supervisor:fresh`, and preflight's IPC convergence ack'd `status:ok`. Only `projection_lag=true` remains (the state mirror trailing slightly — that's exactly what `#n529b` wires away). No `live_prompt_drift` this cycle; this finalize is going through the clean editor path, not `--force-disk`.

**Why I'm not auto-looping the 34 "drainable" heads.** The count is real but the heads aren't quick drain work:

- **Duplicates** — `#gghx`, `#8g9w`, `#vwj7`, `#cgfx`, `#nsfz` each appear twice (once as a `[#id]` pin, once as `do [#id]`). That's the `#queuestatemachine`/`#qdedupsync` duplication class itself; I'm leaving it to supervisor convergence rather than hand-editing the live queue (manual edits race the supervisor).
- **Focused-cycle / cross-repo units** — the genuine heads are deep plan-backed refactors: `#vwj7`/`#cgfx`/`#brtc` (queue-activity SM), `#gghx`/`#8g9w`/`#6mjw`/`#spy9` (wait-machinery SM), `#n529b`/`#r5at`/`#6n5j` (lazily plugin state-sync). `#gww8` self-declares *"sibling-repo (src/lazily-spec) cycle per scope-discipline"* — by its own text it must run scoped to lazily-spec, not here.
- **Operator-gated** — `#nsfz` is `[operator-verify]`, `#capproofbg`/`#p2j4` need a live JB editor.

**Semantic-completion warnings are follow-ons, not duplicates** — `#n529b` is the consumer-wiring phase after `#n529` shipped the mirror substrate; `#cgfx` is SM route-convergence after `#qdedupsync`'s dedup fix; `#kj5f`'s review remainder is the operator-verify half after `#6b5h`. None are auto-strikable; leaving all three.

**Recommendation:** pick one focused item for a fresh cycle (my suggestion: `#npe1` `#codefencestrip` — a concrete in-scope agent-doc parser bug, fully agent-completable; or `#0bmv` plan-consolidation), or restart the session to let convergence drop the duplicate queue churn first. I'm stopping here rather than force-draining a churned session (16 cycles / 30 min, accretion=warn) through cross-repo and focused-cycle heads.

### Re: IPC proof diagnostic (interrupted-cycle recovery) — agent-doc

```text
**IPC proof issue dogfood log**
Appended automatically during interrupted-cycle recovery to record the editor IPC issue.
This is binary-authored diagnostic content, not a user prompt, so it does not require a separate response cycle.
Issue class: `ipc_proof_insufficient`
Affected component: editor IPC / writeback

ipc_proof_insufficient file=/home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md source=socket_ack_content patch_id=c4187291-eec1-4c6e-8ed3-501a67296999 invariant=live_prompt_drift_after_preflight recovery=content_ours_snapshot_next_cycle snap_source=ack_content_sidecar candidate_len=32972 candidate_hash=7585bc7359e0df199d2bb9f9f79c2a51b75ba77c7485ad5bbfe950c20ef77fca content_ours_len=34893 content_ours_hash=fa17ef3e81a4a04fc86d566453b98ead5f103efbb10b84fcf67b69397b8e9c3d doc=agent-doc-bugs2 session=b26b9957-8de6-4eae-bb98-5d7fe4d6b781 turn=cycle-1782370732724
```

### Re: fixed the queue-churn root cause (#qheadresidue) — opus-4-8

Root-caused and fixed the churn in the **binary** (agent-doc `cadc400c`, submodule pointer `ed91730cb`), so every user benefits — not a per-session workaround.

**Root cause.** The per-cycle `#ftstrike` (`strike_answered_free_text_queue_heads`) only matches **this cycle's** `response_body`. A free-text queue head answered in a **prior** cycle — its `> **Queue prompt:**` echo is in committed `agent:exchange`, but the current response doesn't re-quote it — was **never struck**. It stayed active as "completed residue" that `check_free_text_queue_head_provenance` INTERRUPTs on *every* closeout, while go-mode `backlog→queue` convergence kept re-adding it. That's the loop you saw: the `🚧 JB Run Agent Doc` head reappearing each preflight, 9 no-op closeouts in 30 min. session-check *detected* it ("exchange history contains a `Queue prompt` echo proving they were already answered") but only INTERRUPTed — detection without remediation.

**Fix.** Added a preflight maintenance catch-up pass (`agent-doc-orchestration/src/preflight/maintenance.rs`) — the free-text analogue of the existing id-backed `strike_done_queue_head_prompts`. It strikes any active free-text head already answered **anywhere in committed exchange**, reusing the **same** `free_text_head_answered_by_response` predicate the session-check residue guard uses. So the preflight strike set and the session-check INTERRUPT set are now symmetric: anything that would have INTERRUPTed closeout is struck *before* session-check runs, and the queue advances instead of churning. Two deterministic tests (strikes prior-cycle-answered head; leaves an unanswered head active). `make check` green (5137 passed), release build green.

**To go live:** the fix is committed but the **running** supervisor is still on the old binary (0.34.48). It picks up `cadc400c` via the supervisor boundary auto-install, or immediately on `agent-doc session restart-supervisor tasks/agent-doc/agent-doc-bugs2.md`. I deliberately did **not** `cargo install` mid-session (that makes the finalize client newer than the live supervisor → the exact drift), and did **not** cut a release (your standing "stop cutting releases" hold). The queue stays **paused** until the new binary is live — resuming on the old binary would just churn again. After restart: resume the queue and it will drain the `🚧` residue cleanly on the first pass.

**Scope note:** this fixes the specific churn loop (answered-head residue). The separate IPC `live_prompt_drift` / stale-CRDT issue (the parent route-owned supervisor) is the operator-restart / supervisor-in-process concern, not this strike pass.

### Re: queue-activity state machine — head epic, Phase 1 (#vwj7) — opus-4-8

Post-restart recovery + first focused-cycle phase of the head epic.

**Recovery first.** Preflight tripped the layout guard (`snapshot ≠ HEAD without an open cycle`, 9-char drift). The drift was pure queue-lifecycle churn the supervisor had written (boundary-marker bump, `~~~done` wrap on the actor-switch free-text head, `🚧` on `#brtc`) — no user prose. Cleared it the safe way (`git stash` the file → `reset --from-current --preserve-session` → `stash pop`) so snapshot re-aligned to HEAD and the markers committed as a normal converged diff. `recovered: true, committed: true`.

**#vwj7 — `#queuestatemachine1` done.** Implemented `agent-doc-orchestration/src/queue_item_state_machine.rs` (submodule `ee79b797`): a pure per-item lifecycle model that makes duplicate-injection *structurally impossible* instead of patched after the fact.

- **States** `Authored < Mirrored < InProgress < Answered < Struck < Reaped` form a total order; `transition_queue_item` is the **join** `max(current, event.target())`.
- **The guarantee:** a stale-CRDT / supervisor re-emit of an `OperatorAuthored` / `BacklogMirrored` event for an identity already at `InProgress`/`Answered`/`Struck` is a **no-op** — it can't resurrect the item as a fresh head, so the exact churn we just hand-recovered (`#actorswitchdefer` / `#n529b` residue re-injection) cannot form a visible duplicate. This is the model `#brtc`'s SimWorld will exercise.
- **Identity** reuses `extract_head_id` (so `:pushpin: do [#gww8]`, `do [#gww8]`, `[#gww8]` collapse to one identity) and the free-text dedup normalizer.
- **9 exhaustive tests** prove totality, monotonicity, idempotence, join semantics, re-emit-storm convergence, and identity collapse. `make check` green (5146 tests).

**Not yet wired into convergence** — that's `#cgfx` (`#queuestatemachine2`: route the ad-hoc dedup passes through the SM as transition guards), then `#brtc` (`#queuestatemachine3`: SimWorld re-emit-storm proof). No behavior change is live yet, so no install/recycle needed — consistent with the standing "stop cutting releases" hold.

**Scope note:** no release ceremony (version/tag/publish/install) per your hold; the code is committed to the submodule and the superproject pointer follows.
<!-- agent:boundary:3fc0e730 -->
<!-- /agent:exchange -->

## Queue


<!-- agent:queue priority preset="#spec-test-build-install-commit-push" priority go -->
- ~~:pushpin: [#56ta]~~
- ~~:pushpin: [#vm7g]~~
- ~~:pushpin: [#n529]~~
- ~~:pushpin: do [#fnxy]~~
- ~~:pushpin: do [#t4x1]~~
- ~~:pushpin: [#kj5f]~~
~~~done
:pushpin: JB `Run Agent Doc` on equityfundingsource.md after switching from codex to opencode...any agent change has this issue. I have to kill + restart the tmux pane or recycle the supervisor for the opencode state to pick up. The supervisor should reactively pick up the agent state change. This also caused IPC drift...as seen in a recent monsterrodholders.md session.
```
resync: pruned 2 stale session(s)
[route] target tmux session: 0
Error: authoritative actor record for tasks/professional/equityfundingsource.md is running harness codex, but frontmatter now resolves to opencode; deferring to boundary agent restart instead of replacing live pane
```
```
[route] target tmux session: 0
Error: authoritative actor record for tasks/monsterrodholders.md is running harness codex, but frontmatter now resolves to opencode; deferring to boundary agent restart instead of replacing live pane
```
```
[route] target tmux session: 
Error: authoritative actor record for tasks/agent-doc/agent-doc-bugs2.md is running harness codex, but frontmatter now resolves to opencode; deferring to boundary agent restart instead of replacing live pane
```
~~~
- 🚧 :pushpin: [#brtc]
- :pushpin: do [#gww8]
- :pushpin: do [#n529b]
- ~~~~When adding a backlog item, the agent sometimes assigns an explicit id. But agent-doc ignores the explicit id and assigns a new id. The agent's explicit id should be used instead. So instead of `[#kj5f] #mergestatemachine3` the id should be `[#mergestatemachine3]`.~~ — auto-struck: answered this cycle (#ftstrike)~~
- :pushpin: [#vwj7]
- :pushpin: [#cgfx]
- :pushpin: [#gghx]
- :pushpin: [#8g9w]
- :pushpin: [#6mjw]
- :pushpin: [#spy9]
- :pushpin: [#r5at]
- :pushpin: [#6n5j]
---
:pushpin: JB `Run Agent Doc` on equityfundingsource.md after switching from codex to op. I have to kill + restart the tmux pane or recycle the supervisor for the opencode state to pick up. The supervisor should reactively pick up the new agent state.
```
resync: pruned 2 stale session(s)
[route] target tmux session: 0
Error: authoritative actor record for tasks/professional/equityfundingsource.md is running harness codex, but frontmatter now resolves to opencode; deferring to boundary agent restart instead of replacing live pane
```
```
[route] target tmux session: 0
Error: authoritative actor record for tasks/monsterrodholders.md is running harness codex, but frontmatter now resolves to opencode; deferring to boundary agent restart instead of replacing live pane
```
```
[route] target tmux session: 
Error: authoritative actor record for tasks/agent-doc/agent-doc-bugs2.md is running harness codex, but frontmatter now resolves to opencode; deferring to boundary agent restart instead of replacing live pane
```
---
---
:pushpin: equityfundingsource.md turn has an IPC issue:
`### Re: IPC proof diagnostic (interrupted-cycle recovery) — agent-doc`

```text
**IPC proof issue dogfood log**
Appended automatically during interrupted-cycle recovery to record the editor IPC issue.
This is binary-authored diagnostic content, not a user prompt, so it does not require a separate response cycle.
Issue class: `ipc_proof_insufficient`
Affected component: editor IPC / writeback

ipc_proof_insufficient file=/home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md source=file_ipc patch_id=ca5b06f9-838b-4b60-9ce2-00519736fbb1 invariant=no_ack recovery=retry_without_disk_write timeout_secs=2 patch_file=/home/brian/work/btakita/agent-loop/.agent-doc/patches/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad.json doc=equityfundingsource session=62fe1f41-886c-4f3d-b6fa-48d46106ec27 turn=cycle-1782344406484
```
---
---
:pushpin: lazily-rs.md IPC error
```text
**IPC proof issue dogfood log**
Appended automatically during interrupted-cycle recovery to record the editor IPC issue.
This is binary-authored diagnostic content, not a user prompt, so it does not require a separate response cycle.
Issue class: `ipc_proof_insufficient`
Affected component: editor IPC / writeback

ipc_proof_insufficient file=tasks/software/lazily-rs.md source=socket_ack_content patch_id=- invariant=missing_response_probe recovery=retry_without_disk_write response_sha256=8c56928442bd4b165543ab0aad264d681b41b8898551d10f196018cd15a37304 content_len=11944 content_hash=46b47fdfe5505fb37c9e63df4167cb923df6b4ffc997ecca9009a15979c29a5e doc=lazily-rs session=3d9ff8e4-bafd-4e1f-b48d-550d3c976904 turn=cycle-1782339826601
```
---
- :pushpin: [#nsfz]
- ~~:round_pushpin: do [#rt83qflood]~~
- :pushpin: ~~I'm still seeing duplicate queue items added by agent-doc. Do we need to have state machine(s) represent queue activity to ensure these sort of issues don't happen?~~ — auto-struck: answered this cycle (#ftstrike)
- :pushpin: ~~I'm still seeing JB `File Cache Conflict` prompts. Do we need plugin/binary state machine(s) to control the merges to ensure disk writes don't happen while the editor/FFI plugin is running?~~ — auto-struck: answered this cycle (#ftstrike)
- :pushpin: Still getting JB `File Cache Conflict` dialogs.
- :pushpin: Opencode responses are reverse ordering numeric lists.
- :pushpin: do [#npe1]
- :pushpin: do [#p2j4]
- :pushpin: do [#4pdp]
- :pushpin: do [#0bmv]
---
:pushpin: JB `Run Agent Doc` on equityfundingsource.md after switching from codex to op. I have to kill + restart the tmux pane or recycle the supervisor for the opencode state to pick up. The supervisor should reactively pick up the agent state change.
```
resync: pruned 2 stale session(s)
[route] target tmux session: 0
Error: authoritative actor record for tasks/professional/equityfundingsource.md is running harness codex, but frontmatter now resolves to opencode; deferring to boundary agent restart instead of replacing live pane
```
```
[route] target tmux session: 0
Error: authoritative actor record for tasks/monsterrodholders.md is running harness codex, but frontmatter now resolves to opencode; deferring to boundary agent restart instead of replacing live pane
```
```
[route] target tmux session: 
Error: authoritative actor record for tasks/agent-doc/agent-doc-bugs2.md is running harness codex, but frontmatter now resolves to opencode; deferring to boundary agent restart instead of replacing live pane
```
---
---
:pushpin: JB `Run Agent Doc` on equityfundingsource.md after switching from codex to op. I have to kill + restart the tmux pane or recycle the supervisor for the opencode state to pick up. The supervisor should reactively pick up the new agent state.
```
resync: pruned 2 stale session(s)
[route] target tmux session: 0
Error: authoritative actor record for tasks/professional/equityfundingsource.md is running harness codex, but frontmatter now resolves to opencode; deferring to boundary agent restart instead of replacing live pane
```
```
[route] target tmux session: 0
Error: authoritative actor record for tasks/monsterrodholders.md is running harness codex, but frontmatter now resolves to opencode; deferring to boundary agent restart instead of replacing live pane
```
```
[route] target tmux session: 
Error: authoritative actor record for tasks/agent-doc/agent-doc-bugs2.md is running harness codex, but frontmatter now resolves to opencode; deferring to boundary agent restart instead of replacing live pane
```
---
- :pushpin: do [#actorswitchdefer]
- :pushpin: do [#capproofbg]
- :pushpin: do [#nsfz]
- :pushpin: do [#gghx]
- :pushpin: do [#8g9w]
- :pushpin: do [#6mjw]
- :pushpin: do [#spy9]
- :pushpin: do [#vwj7]
- :pushpin: do [#cgfx]
- ~~do [#nsq2]~~
- ~~:pushpin: do [#t4x1]~~
- :pushpin: Are the lazily-rs contexts distinct for each running agent document? I'm seeing sessions working on other open documents.
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
- [ ] [#n529b] #lazilystatesync3b Wire MirrorProjectionSummary into EditorCommandStateMachine / dispatch-ready / busy consumers (PromptPoller, RunAgentDocAttemptLedger) so they read StateProjectionBridge.mirrorSummaryForFile instead of the cold projectionSummaryForFile pull, then dogfood live in JB. Mirror substrate shipped in 0.2.190 (#n529). Plan: tasks/agent-doc/plan-lazily-plugin-state-sync.md
- [ ] [#gww8] #lazilyspecpin=Pin agent-doc state type_tag vocabulary (8 node kinds) + conformance snapshot/delta pair in lazily-spec as schemas/agent-doc-state.json (versioned) so kt/js/rs import one canonical vocabulary. Design pinned in tasks/agent-doc/plan-lazily-plugin-state-sync.md (Wire-shape definition section). Sibling-repo (src/lazily-spec) cycle per scope-discipline.
- [ ] [#vwj7] #queuestatemachine1 Define queue-item identity + lifecycle-state model (Authored→Mirrored→InProgress→Answered→Struck→Reaped) + pure transition fn with exhaustive (state×event) tests. Replaces ad-hoc dedup passes. Plan: tasks/agent-doc/plan-queue-activity-state-machine.md
- [ ] [#cgfx] #queuestatemachine2 Route convergence through the SM; reduce dedup passes to transition guards; property test idempotence (re-emit=no-op) over #rt83qflood/#qdedupsync/#qauthorder duplication shapes. Plan: tasks/agent-doc/plan-queue-activity-state-machine.md
- [ ] [#brtc] #queuestatemachine3 SimWorld: stale-CRDT/supervisor re-emit storm converges to one item per identity; operator position-lock survives. Plan: tasks/agent-doc/plan-queue-activity-state-machine.md
- [ ] [#r5at] #lazilystatesync4 VSCode plugin: lazily-js mirror graph applying FFI state deltas. Plan: tasks/agent-doc/plan-lazily-plugin-state-sync.md
- [ ] [#6n5j] #lazilystatesync5 Parity+tests: lazily-spec cross-editor conformance (extend EditorCommandStateMachineTest to reactive path) + FFI round-trip proving kt/js mirror converges to rs authoritative graph. Plan: tasks/agent-doc/plan-lazily-plugin-state-sync.md
- [ ] [#gghx] #waitmachine1 Wait-machinery state machine core: add agent-doc-orchestration/src/wait_machine.rs with a pure WaitState/tick transition + single GLOBAL_HANG_CEILING (10s) + typed ReinstallPause exemption. Plan: tasks/agent-doc/plan-wait-machinery-state-machine.md
- [ ] [#8g9w] #waitmachine2 Migrate scattered waits (start.rs AutoTriggerMonitor 30s, route/startup.rs wait_for_agent_ready 30s, route.rs busy/shell waits, IPC ack) onto WaitMachine and lower budgets to the 10s ceiling (operator: never hang >10s). Plan: tasks/agent-doc/plan-wait-machinery-state-machine.md
- [ ] [#6mjw] #waitmachine3 Deterministic bound proof: per-state transition tests + exhaustive (state×signal) property test asserting Σdwell≤10s on every exempt-free path + SimWorld hang-class coverage. Plan: tasks/agent-doc/plan-wait-machinery-state-machine.md
- [ ] [#spy9] #waitmachine4 Lean model: formal/wait_machine/ encoding WaitState/Step mirroring Rust tick, theorem no_hang (exempt-free ⇒ dwell≤10s) + reinstall-pause sole-exemption lemma + Rust↔Lean parity test. Plan: tasks/agent-doc/plan-wait-machinery-state-machine.md
- [ ] [#nsfz] [operator-verify] #startupdeadline live dogfood: drive a real harness that never reaches a dispatch-ready prompt (e.g. auth-walled/hung TUI) and confirm the session fails closed within 30s with a 'session did not become dispatch-ready in Ns' stderr diagnostic + a recorded startup_miss marker, instead of hanging. Code-complete + tested (make check 5101 green, release build green); needs a live editor/pane eyeball.
- [ ] [#capproofbg] Make the JB Run Agent Doc capacity proof non-blocking: dispatch immediately and run the capacity proof in the background, surfacing failure asynchronously instead of stalling every dispatch.
- [ ] [#npe1] #codefencestrip=Template patch parser strips trailing code fences at the exchange boundary
- [ ] [#p2j4] #jbcfdiag=JB File Cache Conflict: investigate remaining write-path triggering the JB VFS dialog beyond IPC-first writes
- [ ] [#4pdp] #ocreverselist=OpenCode reverse-ordered numeric list rendering investigation
- [ ] [#0bmv] #qeditrace=Consolidate queue-edit-during-active-turn race plans (plan-qdurcrash + plan-supervisor-direct-queue-edit-race)
- [ ] [#actorswitchdefer] [focused-cycle] Part B: NEW SimWorld restart-flow tests (codex→opencode: assert route defers + idle-watch emits harness_change_detected→agent_restart_triggered→agent_restart_performed; paused/stuck regression asserts defer-bail recovery suffix + no silent drop) + route-side agent_change_restart-disabled explicit bail. Part A shipped (6c7455a8); SPEC.md:141 defer contract reconciled (d2312da7). Needs make tmux-ci + new restart-flow SimWorld infra. Plan: tasks/agent-doc/plan-actor-switch-defer-recovery.md
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
- [/] [#kj5f] [operator-verify] #mergestatemachine3 remaining: JB+VSCode thin reporters emit attach/detach/ownership facts through the merge-ownership SM (via #lazilystatesync) + live editor-attached/detached repro for File Cache Conflict + #6b5h wedge. DONE: SM core Phases 1-2 + deterministic scenario repros for both wedge classes (agent-doc 34d7023d). Remaining needs the operator-paused plugin install + a live editor to verify. Plan: tasks/agent-doc/plan-merge-control-state-machine.md
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
