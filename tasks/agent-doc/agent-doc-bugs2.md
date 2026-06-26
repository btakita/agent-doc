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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260626-230407.md`*

Compacted content:
- Archived 47 response topic(s): Is CellDocTree too specific to lazily?; queue state and review cleanup; IPC proof diagnostic (interrupted-cycle recovery); 44 more
- Prior summary/context: prior compacted content: Archived 10 response topic(s): lazily CellGraph reactive document substrate; CellTree per-ce...

### Re: IPC proof diagnostic (interrupted-cycle recovery) — agent-doc

```text
**IPC proof issue dogfood log**
Appended automatically during interrupted-cycle recovery to record the editor IPC issue.
This is binary-authored diagnostic content, not a user prompt, so it does not require a separate response cycle.
Issue class: `ipc_proof_insufficient`
Affected component: editor IPC / writeback

ipc_proof_insufficient file=tasks/agent-doc/agent-doc-bugs2.md source=socket_ack_content patch_id=5763e16d-7cf8-4d26-9f7d-56ec49a3f151 invariant=live_prompt_drift_after_preflight recovery=content_ours_snapshot_next_cycle snap_source=ack_content_sidecar candidate_len=111311 candidate_hash=5a7016ec672a4c0a0af21dd22a762e2da41c94e3a575a6fda749fe3c8227af29 content_ours_len=113340 content_ours_hash=f10b4b17efc726dd885def872cb187fe4c9984817b96c0bc57c2cc6c2d834ce8 doc=agent-doc-bugs2 session=b26b9957-8de6-4eae-bb98-5d7fe4d6b781 turn=cycle-1782513543058
```

### Re: distinguishing operator vs agent-doc authorship — operator actions must be authoritative — opus-4-8

Yes — we can and should, and this is the **root of the whole bug class**, not three separate bugs. Today agent-doc does **not** record provenance; it *infers* it from content: snapshot↔working-tree diffing, the IPC source of a writeback, and content heuristics (`content_ours`, "looks like a manual cleanup", the stale-editor resurrection guards). Inference from content is what mistreats your edits.

**Why each symptom is the same gap:**
- *"I deleted items and they reappeared"* — the backlog→queue mirror (`sync_backlog_into_queue`) re-adds any active `queue`-attr backlog id that's *absent* from the queue. Absence is ambiguous: it can't tell "never added" from "operator deleted it," so your delete is undone next preflight. No tombstone records that the delete was an authoritative operator act.
- *manual edits treated as suspicious* — content-cleanup/resurrection heuristics fire on text shape, not author.
- *agent text in your queue* — whole-document `crdt::merge` splices console/response content into `agent:queue` (a console line of mine landed as a queue item again this turn).

**The principle, stated as an invariant:** operator-origin text is authoritative — **never reset, never resurrected-after-delete, never flagged suspicious.** Provenance must be a recorded per-edit fact, not a content guess.

**It's partly working already.** This very cycle the per-cell merge (#qcellmerge) hit a conflict on your "don't add duplicates" line and resolved it `chosen=ours (operator: resolve; ours kept active)` — it kept *your* content. The gaps: it leaks raw `<<<<<<< ours` conflict markers into the live queue, and agent content still reaches the queue cell at all.

**The mechanism** is CRDT transaction-origin tagging: tag every yrs transaction `Operator` (editor DocumentListener delta over the #crdtauth IPC seam) / `Binary` (finalize/write/preflight) / `Mirror` (auto-sync), persist origin per delta, then enforce the invariant at the two mis-classification sites (recovery heuristics; the mirror). I filed the consolidated design at **`tasks/agent-doc/plan-operator-provenance-authority.md`** and queued four phases:
- **#provauth2** (smallest standalone win — closes *both* your reports this cycle): operator-delete tombstones the mirror honors + dedup against all ids on a line so auto-add never duplicates. Can ship from snapshot-vs-working-tree operator deletes without full origin tagging.
- **#provauth1**: the origin enum + tagging foundation.
- **#provauth3**: replace `content_ours`/manual-cleanup/resurrection heuristics with origin checks (operator-origin ⇒ authoritative; content heuristics become the unknown-origin fallback only).
- **#provauth4**: per-cell provenance (rides #qcellmerge) so agent content can never land in an operator cell.

I did **not** hand-clean the conflict markers / leaked line now — the IPC listener is active and manual queue edits race it. #provauth2 is the right place to fix it properly. Want me to take #provauth2 in a focused cycle next?

### Re: should we remove the post-commit IPC worktree reconcile? — yes, in realtime; keep only for headless — opus-4-8

You're right to suspect it. The whole post-commit tower — `reconcile_postcommit_worktree_to_head` (#pcwc), the per-component exchange/queue reconciles (#pcwcwarn / #pzjy), `content_ours`, and the "skipping working-tree boundary reposition — IPC listener active" + reposition-signal dance — is **turn-based file-sync legacy**, and it is the direct source of the corruption you've seen recur every cycle this session.

**What value it had (turn-based model):** the agent took a turn, committed a response, and the editor's on-disk buffer was now *stale* — it didn't know about the response and its `(HEAD)` boundary marker was in the wrong place. So the binary had to (1) get the response into the visible buffer, (2) reposition the boundary, (3) do it **without** an out-of-band disk write that pops the JB *File Cache Conflict* dialog, and (4) without a disk read-modify-write that races a typist and duplicates the structural tail (#xbs3). The reconcile-to-HEAD tower is how it kept `working-tree == HEAD` after a commit under those constraints. Real value — *when disk is the sync channel*.

**Why it's harmful now (realtime CRDT model):** with a live CRDT replica (the #crdtauth seam), the editor already holds the converged state — the response arrived as `Binary`-origin deltas as it was written; the boundary is just part of the shared doc. The commit should be a **pure git persistence of the converged CRDT**, nothing more. The four constraints above all dissolve: there's no stale buffer to refresh (#1/#2 done by delta application), and there's no out-of-band disk write at all, so the FCC dialog (#3) and the #xbs3 race (#4) are *moot*. What the tower does instead is **re-derive the buffer from a stale snapshot and overwrite the live one** — exactly the splice/`<<<<<<< ours`/console-leak corruption I hit (and recovered from) repeatedly this turn.

**So: don't rip it out unconditionally — make it bimodal on "is a live CRDT replica attached?"**
- **Live replica (realtime):** commit = persist CRDT → git. Skip the entire post-commit reconcile/reposition/`content_ours` path. The corruption class disappears because nothing re-derives the buffer.
- **No live editor (pure-CLI / headless, the #6b5h case):** keep the disk-write reconcile — there, disk genuinely *is* the only sync channel.

This is the realtime cutover; I filed it as **#pcwcrt** and it rides #provauth1 (origin tagging — the signal that tells the commit which mode it's in), #crdtauth6 (the live replica), and #cdtcutover (per-node deltas at finalize instead of whole-tree reconcile). It belongs in the same series as the provenance work: once edits are origin-tagged, the binary *knows* the editor already converged and can stop guessing.

**On the provauth ids you flagged:** correct — my `--pending-add 'provauth2=…'` was wrong; the id needs `#` (`[#provauth2] text`, or `id=provauth2 ` with a trailing space). They landed under auto-ids (#f693=1, #q9w3=2, #fpyh=3, #818s=4) with the intended tag in the text. I tried to re-key them but there's no backlog-remove flag and `--pending-edit` embeds `[#provauth2]` as text rather than re-keying, so I stopped rather than churn this buffer further — the plan (`plan-operator-provenance-authority.md`) maps phase→id. A clean rename needs `--pending-clear`+re-add (destroys all 10 backlog items) or a manual edit when the IPC listener is quiet.

Given this session is corrupting on nearly every write (live_prompt_drift + postcommit splice each cycle — itself the #pcwcrt evidence), I recommend we **recycle the supervisor / start a fresh session** before more document work. Want me to take **#provauth2** (the smallest standalone win — closes the "deleted items reappear" + "no duplicate adds" reports) in a fresh focused cycle?

### Re: do #pcwcrt + all four #provauth — started foundation-first; #provauth2 slice shipped — opus-4-8

I'm treating this as the full provenance+commit-cutover program. It's deep core CRDT/commit-path work and I'm executing it **foundation-first, in the submodule, `make check`-gated, no mid-session install** (installing a new binary against this live supervisor is what destabilizes it). Honest status:

**Shipped this turn — #provauth2 (slice 1: multi-id dedup), committed `cd8f3522`, `make check` green:**
- Root cause of the duplicate proliferation you saw: `sync_backlog_into_queue` deduped off `do_prompt_id`, which reads **only the first `#id`** on a line. So your manual `[#a] [#b] [#c]` line registered only `#a` as present and the mirror re-added `#b`/`#c` as duplicate `do [#id]` lines every preflight.
- Fix: `do_prompt_ids`/`entry_do_ids` (multi-id aware); `existing_ids` now collects **every** id on every queue line. Operator multi-id lines are never duplicated by auto-add. +2 regression tests.

**Remaining program (sequenced; each its own focused cycle on a clean session):**
- **#provauth2 slice 2** — operator-delete **tombstone**: a queue id the operator deletes is suppressed from re-mirror (durable sidecar) until they re-add it or the backlog item closes. This is what closes "I deleted items and they reappeared" (the dedup slice closes "don't add duplicates").
- **#provauth1** — CRDT transaction-origin tagging (Operator/Binary/Mirror) in crdt.rs/crdt_sync.rs. The foundation the rest keys off.
- **#pcwcrt** — realtime commit cutover: skip the post-commit reconcile tower when a live CRDT replica is origin-proven converged. *Depends on #provauth1* — without the origin signal it's just another `is_listener_active` branch in an already-5-branch function that still corrupts (today's evidence).
- **#provauth3** — replace `content_ours`/manual-cleanup/resurrection heuristics with origin checks.
- **#provauth4** — per-cell provenance (rides #qcellmerge) so agent content can't splice into operator cells. This is the path that wrote the `<<<<<<< ours` conflict markers into your queue this session.

**Why I'm not ramming the rest through now:** this session's in-memory CRDT is corrupting on nearly every write (live_prompt_drift + the conflict-marker splice — itself the #provauth4 evidence). The responsible path is: install `cd8f3522` + **recycle the supervisor** (clears the corrupt replica, picks up the new binary), then I drain #provauth2-slice-2 → #provauth1 → #pcwcrt → #provauth3 → #provauth4 against a clean supervisor. Also captured your new bug: **JB `Run Agent Doc` floods duplicate dispatch prompts** — it's the documented #vgat/#jbrundispatch no-submit-retry class (root-cause fix is a separate dispatch cycle).

Recommend: recycle now, then I continue the program. Proceed?
<!-- agent:boundary:901a15ae -->
<!-- /agent:exchange -->
## Queue


<!-- agent:queue priority preset="#spec-test-build-install-commit-push" priority go -->
- 🚧 [#f693] [#q9w3] [#fpyh] [#818s]
- [#syncbarrier] [#qheadstrikeauto] [#actorprune] [#suprestassoc]
- [#2y8j] [#cdtcutover] [#qeditdup] [#crdtauth6] [#nsfz] [#kj5flive]
- I deleted items from the queue but they reappeared
- When auto-adding queue items, verify that the item is not already in the queue. Do not add duplicates.
- JB `Run Agent Doc` flooded the prompt with duplicate `agent-doc .../agent-doc-bugs2.md` dispatch prompts. Please fix the root cause.
- do [#q9w3]
- do [#f693]
- do [#fpyh]
- do [#818s]
- do [#qheadstrikeauto]
- do [#actorprune]
- do [#suprestassoc]
- do [#cdtcutover]
- do [#qeditdup]
- do [#crdtauth6]
- do [#nsfz]
- do [#kj5flive]
- do [#pcwcrt]
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
- [ ] [#f693] provauth1=#agent-doc-feature CRDT transaction-origin tagging: tag every yrs transaction in crdt.rs/crdt_sync.rs as Operator (editor DocumentListener delta over IPC, the crdtauth seam) / Binary (finalize/write/preflight writeback) / Mirror (backlog-queue auto-sync); persist origin per delta so any span author survives restart. Foundation for provenance authority. Plan: tasks/agent-doc/plan-operator-provenance-authority.md
- [ ] [#q9w3] provauth2=#agent-doc-bug operator queue deletes resurrected + auto-add duplicates. sync_backlog_into_queue (Append/Sync) re-adds any active queue-attr backlog id absent from the queue, so an operator delete is undone next preflight; multi-id/manual lines also defeat the existing-id dedup. Fix: record operator-origin deletes of do-id lines (snapshot vs working-tree) as a session tombstone the mirror honors, and dedup against ALL ids on a line + existing entries so auto-add never duplicates. Smallest standalone win. Plan: tasks/agent-doc/plan-operator-provenance-authority.md
- [ ] [#fpyh] provauth3=#agent-doc-bug replace content-based recovery heuristics (content_ours / manual-cleanup / stale-editor resurrection guards) with origin checks where origin is known: an Operator-origin working-tree change IS the authoritative prompt and is never reset or treated as suspicious cleanup; content heuristics remain only as the unknown-origin fallback. Depends on provauth1. Plan: tasks/agent-doc/plan-operator-provenance-authority.md
- [ ] [#818s] provauth4=#agent-doc-feature per-cell provenance riding qcellmerge per-cell merge isolation so agent console/response content can never splice into an operator-authored cell (queue). Live proof captured: a console line appeared as a queue item. Depends on provauth1 + qcellmerge. Plan: tasks/agent-doc/plan-operator-provenance-authority.md
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
