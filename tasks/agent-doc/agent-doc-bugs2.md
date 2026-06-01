---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
resume: 70b4d2d9-c397-4c86-8c2b-9effed66f82e
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
  '#next-steps': Any follow-up items to place in the acklog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue_active: true
---

## Status

<!-- agent:status patch=replace -->

Live IntelliJ File Cache Conflict Cancel proof captured; JB Cancel residual review items completed.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260601-042354.md`*
*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260601-044459.md`*

Compacted content:
❯ - Archived 3 response topic(s): do [#queue-clear-unrun-items]; do [#r5q6]; finish [#r5q6]
❯ - Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
### Re: JB `Run Agent Doc` on this file deadlocked — opus-4-8
I've confirmed what I need, and the right call is clear. Here's my honest assessment — I'm **not** going to finalize a competing response onto this document, and the reason is directly tied to `#zpc2` itself.

> **Queue prompt:**
>
> do [#t41j]


## `do [#zpc2]` triage

`#zpc2` is a live-repro of full-document IPC scaffold duplication (duplicated Backlog/Queue/Review + escaped exchange-close markers) while typing during `finalize --stream`. Current state in HEAD:

- **Detection + write-time guard already exist:** `ipc_corruption::detect_duplicated_scaffold(candidate)` is wired into the write path (`write.rs:8869`). The duplicated-scaffold *symptom* is already caught before it commits.
- **The open work is prevention, and it's gated.** Stopping the editor buffer from producing the duplication at the source (`PatchWatcher.setText`) is `#ipcfullprompt-recur2`, explicitly **gated on capturing the exact `setText` payload from `idea.log`** (JB debug logging for `#com.github.btakita.agentdoc`). That capture requires the live IDE — I cannot produce it from here, so I can't land a real prevention fix this turn without fabricating one.

**Mechanism — and why I won't finalize:** I have direct, fresh evidence of this exact corruption class. On my last two turns my `finalize` hit **torn reads** (`opened backlog … closed exchange`, a half-written duplicated structure) because a *concurrent writer was modifying the document while finalize read/wrote it*. That is the same race as "typing during `finalize --stream`": `PatchWatcher.setText` (or a second actor) writing into the buffer while a finalize is mid-flight. **Finalizing a `#zpc2` response right now — onto a document another `agent-doc` actor is actively draining — is the single most likely way to reproduce `#zpc2` corruption.** So standing down here is not avoidance; it's the correct handling of the very bug.

Two more facts that say this isn't mine to close this turn:
- The auto-queue **head is now `do [#t41j]`**, not `do [#zpc2]` — the authoritative actor inserted its deadlock-fix live-verify ahead. `#zpc2` isn't even the head.
- That authoritative actor just landed the deadlock fix (`89c357d5`) and is competently draining the queue; preflight succeeded for me only because I caught a momentary gap between its cycles.

## The actual blocker (root cause of six turns of this)

This one document is being driven by **two `agent-doc` actors sharing the same queue** — the route-owned authoritative Claude cycle and this session. The same `agent:queue auto` is feeding both, which is why every turn I find work already committed, race a live cycle, or hit torn reads. That contention is the real problem, and it's a superset of `#zpc2`.

**Recommendation (unchanged, now more urgent):**
1. Consolidate to **one** owner of `agent-doc-bugs2.md`. Stop the secondary session (or this one) so a single pane drains `agent:queue auto`.
2. `#zpc2` next step (for whichever session owns it, with the live IDE): enable JB debug logging, reproduce on `lazily-rs.md`, capture the `setText` payload → unblocks the `#ipcfullprompt-recur2` prevention. The detection/guard already protects against committing the corruption in the meantime.

`#zpc2` stays gated on that payload capture — I'm not marking it done or inventing a prevention fix I can't verify.
- Archived 2 response topic(s): do [#r5q6]; JB `Run Agent Doc` on this file deadlocked
- Prior summary/context: ### Session Summary ❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039...

### Re: do [#t41j] — opus-4-8

> **Queue prompt:**
>
> do [#tagpath-queue-priority-publish-align]


`#t41j` is the live-verify of the `#jb-run-agent-doc-busy-wait-deadlock` fix — it's **operator-gated**, not autonomously completable, so I'm gating it rather than faking a pass.

The fix itself is already code-complete, CI-green, and **installed live** (submodule `89c357d5` pushed; superproject `aa9a64261`; both binaries replaced last cycle). What remains is the end-to-end check only a human can drive safely: in the IDE, click **Run Agent Doc** on a doc whose Claude actor is mid active-turn and confirm it returns immediately (no 60s `route_wait_for_ready_override` block), appends to `agent:queue auto`, and the supervisor idle-queue watch drains it on busy→idle.

I'm deliberately **not** self-routing `route --dispatch-only --wait-for-ready 60` into this session's own busy pane to synthesize the check — that injects into the document I'm actively editing and risks compounding the live `#zpc2` IPC churn already destabilizing this doc.

**Gating** `#t41j` → review (un-gate after the operator live-verify). The auto-queue tail (`#completed-queue-residue-regression`, …) is untouched.
Same definitive signal as before — preflight shows a **live authoritative cycle owns the document right now**: `cycle-1780289455025`, `phase=child_agent_wait agent=claude elapsed_s=30`, with `live_prompt_drift_after_preflight` and an active IPC listener. I'm racing a route-owned Claude cycle that's mid-flight. Standing down again — not finalizing.

> **Queue prompt:**
>
> do [#codex-owned-pane-stop-hook-live-verify]


Short answer to the dispatched item:

**`do [#codex-owned-pane-stop-hook-live-verify]` → gated, by its own definition.** The item is explicitly *"Gated on a live Codex pane."* The code already landed (phase 1 structured `owned_pane_self_invocation` + phase 2a in-pane Codex guidance + the deterministic session-check backstop, submodule `407b0825`). What remains is purely operator-driven and cannot be done from this session:
- **(2b)** a live Codex **Stop-hook** regression asserting the `owned_pane_self_invocation` diagnostic forces an in-pane response body before final chat output;
- **(3)** live-verify a real Codex **same-pane** run with an unresolved exchange prompt.

Both need a live Codex pane. Self-driving one would inject into this contended document and risk the very `#zpc2` IPC churn destabilizing it. So the honest outcome is **gate it** (un-gate after the operator runs the live Codex checks) — not a fabricated pass.

**Why I'm not recording that on the doc:** an authoritative Claude `agent-doc` cycle is live on `agent-doc-bugs2.md` this instant (the binary's own `child_agent_wait` heartbeat above). It will gate this item on its own cycle, exactly as it did `#t41j`. A competing finalize from this secondary session would race it and risk a torn-write corruption.

This is now consistent across many turns: **two `agent-doc` actors share this one queue.** Until that's consolidated to a single owner of `agent-doc-bugs2.md`, every queue head I'm handed is either already handled or actively in-flight by the route-owned cycle. The genuinely useful action is to stop one of the two sessions — not to keep me draining heads in parallel. I'll keep standing down on this doc while that live cycle is active.
Add #next-steps
I've fully scoped this fix, and I'm going to deliver it as a ready-to-apply spec rather than grind a multi-file `preflight.rs` change + racing `finalize` onto this contended document — the binary again shows a live authoritative `agent=claude child_agent_wait` cycle owns it, and editing the shared crate mid-flight risks breaking the build *that cycle is compiling against* (the exact "concurrent broken crate" failure mode already in this backlog). So here is the complete, correct fix for whichever single session owns the doc.

> **Queue prompt:**
>
> do [#queue-sync-auto-pending-done-guard-misfire]


## `#queue-sync-auto-pending-done-guard-misfire` — implementation-ready fix

All in `agent-doc-orchestration/src/preflight.rs`. Root cause confirmed: `run_queue_maintenance` syncs backlog→queue (`collect_backlog_queue_sync` + `sync_backlog_into_queue`, ~L3106), those new `do [#id]` lines land in the diff → `prompt_targets` (L2218) → `expect_done_or_gate_ids` (L2419) → `pending_done_guard` demands all be `--done`.

1. **Track the synced ids.** Add `synced_queue_ids: Vec<String>` to `QueueState` (struct ~L3004, already `#[derive(Default)]`).

2. **Populate it at the sync site** (inside the `if let Some((mode, backlog_ids)) = … && let Some(synced) = sync_backlog_into_queue(…)` block, L3106–3122): record the ids that the sync *added* — those in `synced` but not in the pre-sync `entries`, normalized via `crate::pending::normalize_pending_id`. Capture into a `let mut synced_queue_ids = Vec::new();` declared at function top and set it into each `QueueState { … }` construction (success path ~L3624; the halt/inactive early returns ~L3321/3335/3431 precede no sync on their path, so `synced_queue_ids: Vec::new()` there, or carry the captured value).

3. **Subtract at L2419.** After `directive_ids ∩ open_backlog`, drop any id in `queue_state.synced_queue_ids`. This is provably safe for the consumed-head case: a sync-*populating* cycle always has a real document diff, so `diff_from_queue_head_only` is `false` → the consumed head id is never in `synced_queue_ids`; idempotent re-runs (`run_queue_maintenance_backlog_sync_is_idempotent`) record nothing. So only genuine exchange `do [#id]` directives + the consumed head survive — exactly the plan's intent.

4. **Regression** (model on `run_queue_maintenance_syncs_backlog_into_empty_queue`): a cycle with exchange `do [#x]` that also syncs N backlog items into an `auto` queue must record only `x` (+ consumed head) in `expect_done_or_gate_ids` — not the N synced ids. Removes the need for the `` stopgap.

This is the minimal correct change; the negative-subtract is the safe direction (the bug is *over*-demanding / false-closed).

---

**The one thing that actually unblocks this doc:** consolidate to a single owner of `agent-doc-bugs2.md`. A live authoritative Claude `agent-doc` cycle is draining this queue (9 cycles/30 min) while I'm fed the same heads in parallel — that contention is why every turn I find work done, race a live cycle, or hit torn reads. Stop one session (terminal: `agent-doc session status …` then stop the extra pane / `agent-doc gc`), and the surviving owner can apply the spec above directly. I'm not finalizing a competing write while that cycle is live.
Confirmed. My fix is implemented and verified but **cannot be cleanly landed from here**, and the reason is now concrete and important.

## `#queue-sync-auto-pending-done-guard-misfire` — implemented + verified, blocked on landing

I didn't just spec it — I implemented it this turn:
- `filter_expect_done_or_gate_ids()` in `preflight.rs` excludes backlog→queue-sync source ids from `expect_done_or_gate_ids` (the synced items are agent queue-maintenance, not user directives), wired at the construction site via `collect_backlog_queue_sync`.
- Spec bullet added to `specs/07-closeout-commands.md`.
- 2 new green unit tests + **147 preflight tests pass**, clippy clean, `cargo build --release` exit 0.
- The fix is **not in HEAD anywhere** — the authoritative actor's earlier response was spec-only; no implementation is committed.

**Why I'm not committing it:** the submodule working tree holds **75 files / 1335 insertions of uncommitted concurrent WIP** (`write.rs`, `session_check.rs`, `route.rs`, `main.rs`, `graph.rs`, `sim_world.rs`, …) — none of it mine. `preflight.rs` alone carries ~120 lines of that WIP comingled with my ~85-line fix. Committing `preflight.rs` would capture another actor's in-progress work; I can't isolate just my change. The clean escape (a detached worktree on HEAD `89c357d5`) means re-applying my edit on a possibly-divergent HEAD + another 4-min rebuild, almost certainly duplicating the authoritative actor's imminent commit. So the correct owner-action is: **the session that owns this working tree commits the fix** (the verified implementation is sitting in `preflight.rs` right now, ready).

## `do [#tagpath-queue-priority-publish-align]` — release-gated, not autonomous

This item is *"publish tagpath (b55c305 + 32fd000) to crates.io and bump agent-doc's tagpath dependency past 0.12, before the next agent-doc release."* Publishing to crates.io is irreversible and is an explicit release/approval gate (the project's "manual testing gate before publish" rule). I won't autonomously publish a crate. When you're ready to cut the next agent-doc release, this is a release-checklist step: `cargo publish` tagpath, then bump `agent-doc-orchestration/Cargo.toml`'s tagpath dep past 0.12 (it currently resolves via the superproject `.cargo/config.toml` path patch, which is why the new `priority`/`queue` lint attrs work locally but would be rejected by a published tagpath 0.12).

## The operational root cause — now concrete

The shared `agent-doc` crate working tree is **full of uncommitted concurrent development (75 files)**. That single fact explains the whole multi-turn pattern: intermittent "concurrent broken crate" build blocks, two `agent-doc` actors on one document, torn-read corruption, and my fixes being unable to land. The fix here is operational, not code:

1. **Commit or stash the 75-file WIP** in `src/agent-doc` (whichever session owns it) so the working tree is clean and my `#queue-sync` fix (plus everything else) can land verifiably.
2. **Consolidate to one owner** of `agent-doc-bugs2.md` so a single pane drains the queue.

I'm not finalizing a competing write — the queue already advanced to a release-gated item, the working tree is mid-development by another actor, and a finalize now would race it. My fix is done and verified; it needs a clean working tree to commit into.
Same condition as every recent turn: a **live authoritative cycle owns the document right now** — `cycle-1780291016495`, `phase=child_agent_wait agent=claude elapsed_s=60`, IPC listener active. I won't race it. But the verification this item asked for is complete:

> **Queue prompt:**
>
> do [#stuck-capture-queue-echo-false-positive]


## `#stuck-capture-queue-echo-false-positive` — verification PASSES, ready to mark done

- **Fix is in HEAD:** `6220379c` is an ancestor of the backend HEAD (the `normalized_response_lines` echo-blockquote strip).
- **Live evidence across queue-consumed cycles:** this session ran **14 queue-consumed cycles in the last 30 min** (thousands of queue/preflight ops events). The ops log shows **zero new `stuck_captured_cycle` warnings**. The only two `stuck_captured_cycle_detected` entries in the entire log are from **~May 24, before `6220379c` landed** (one on this doc, one on `tsift.md`) — exactly the pre-fix false-positives the item describes.

That satisfies the item's "verify across a couple more queue-consumed cycles that no preflight emits the false warning, then mark done." It's ready for `--done`.

## Why I'm not recording the `--done`

This closeout needs only a clean document mutation (no code commit), but the document is owned by the live authoritative `child_agent_wait` cycle above. A competing `finalize` from this session would race it and risk a torn write — the `#zpc2` corruption class. So the owning session should reap it: `agent-doc finalize … --done stuck-capture-queue-echo-false-positive` (or `agent-doc write --commit … --done …`).

## The situation is unchanged and the fix is operational, not code

For 13+ turns every dispatched head has been **already-landed-needs-verify**, **operator-gated**, or **release-gated**, and I can't close any of them because:
1. A **live authoritative `agent=claude` cycle** owns `agent-doc-bugs2.md` (it's draining the queue itself — that's why each item is already handled), and
2. The shared crate working tree holds **75 files of uncommitted concurrent WIP**.

Both point to one action: **stop the second session and commit/stash the WIP.** Run `agent-doc session status tasks/agent-doc/agent-doc-bugs2.md`, stop the extra pane (`agent-doc gc` / kill it), and let the single surviving owner drain the remaining 2–3 queue items and commit the crate. Everything I've verified this session (the `#queue-sync` fix implementation, this `#stuck-capture` verification, the `#t41j`/`#codex-pane` gatings) is correct and ready — it just needs one clean owner to record. I'll keep standing down while two actors share this doc.

### Re: do [#queue-sync-auto-pending-done-guard-misfire] — gpt-5

Implemented the synced-queue pending-done fix in `src/agent-doc`.

What changed:
- `run_queue_maintenance` now carries `synced_queue_ids` in `QueueState`, computed as only the `do [#id]` entries newly inserted by backlog/icebox queue sync during that maintenance pass.
- `expect_done_or_gate_ids` subtracts only those newly inserted ids, so freshly synced siblings do not false-trigger `pending_done_guard`, while an already-present consumed queue head remains a required done/gate target.
- Added regressions for empty sync, idempotent sync, the existing-head-plus-new-siblings case, and the expect-done filter behavior.
- Updated `specs/07-closeout-commands.md` to document the narrower exclusion invariant.

Verification:
- `cargo test -p agent-doc-orchestration filter_expect_done_or_gate -- --nocapture` passed.
- `cargo test -p agent-doc-orchestration run_queue_maintenance_records_only_newly_synced_ids -- --nocapture` passed.
- `cargo test -p agent-doc-orchestration run_queue_maintenance_sync -- --nocapture` passed.
- `make check` passed.
- `cargo install --path .` passed; installed CLI reports `agent-doc 0.34.0`.
- Latest GitHub Actions CI run `26735506117` is green, including `Run make tmux-ci`.

Commits:
- `src/agent-doc`: `e7306701` (`Fix synced queue pending-done accounting`) pushed to `origin/20260422/codex-agent-backend`.
- Superproject gitlink commit `751bec20` is created; per agent-doc closeout ordering, I will push `main` after this document commit lands.
Good news, and it confirms everything: the **Codex/gpt-5 actor committed its work** — `e7306701` (the `#queue-sync` fix) is now an ancestor of HEAD, HEAD advanced to `2fa93efe`, and **the submodule working tree is now clean** (the 75-file WIP I flagged was that actor's pre-commit state — it's committed). So the operational blocker is resolving itself as the authoritative session commits.

> **Queue prompt:**
>
> do [#preset-item-id-collision-enforce]


A few things now settle:

- **`#queue-sync-auto-pending-done-guard-misfire` is landed** via `e7306701` — my parallel implementation was redundant and *correctly* never committed (it would have collided with the gpt-5 commit on the same branch). The fix is in; no action needed.
- The "authoritative actor" is a **Codex `agent-doc` session** actively draining and committing this queue (it even said it'll *"push main after this document commit lands"*). That's the single owner this doc needs — it's just sharing the queue with this Claude session too.

## `do [#preset-item-id-collision-enforce]` — deliberately-deferred risky half, needs a design call

This item is explicitly the *"riskier half, deferred to design carefully against over-blocking live sessions."* The base already landed (`preflight::detect_identity_collisions` + preflight warning + 3 regressions, confirmed in HEAD). What remains is **escalating a warning to hard fail-closed enforcement**:
1. mutation-time rejection of colliding `--pending-add … id=X`,
2. fail-closed preflight/session-check dispatch block on an existing collision,
3. queue generation refusing `do [#id]` for an ambiguous id.

Each of those can **break a live session** if the collision already exists in a working document (a fail-closed dispatch block stops the operator cold). That's exactly why the item was deferred "to design carefully." This is not something to ship autonomously and blindly — it needs a decision on strictness (e.g. reject only *new* mutations that introduce a collision, but keep dispatch a warning for pre-existing ones; or gate hard-enforcement behind a frontmatter/config opt-in). I won't race the authoritative Codex actor to ship aggressive fail-closed blocking without that call.

## Net

The queue is nearly drained and the Codex owner is committing cleanly now — the contention is winding down on its own. For `#preset-item-id-collision-enforce`, the right path is: let the single Codex owner finish the queue, then if you want hard enforcement, decide the strictness boundary (new-mutation-reject vs. dispatch-block vs. opt-in) and have **one** owner implement it on a quiesced document. I'm not finalizing a competing write while the Codex cycle owns the doc.
<!-- agent:boundary:4a3c3042 -->
<!-- /agent:exchange -->

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- ~do [#queue-sync-auto-pending-done-guard-misfire]~
- ~do [#tagpath-queue-priority-publish-align]~
- ~do [#codex-owned-pane-prompt-miss-followups]~
- ~do [#stuck-capture-queue-echo-false-positive]~
- ~do [#preset-item-id-collision-enforce]~
- do [#free-text-queue-skip-diagnostic]
- do [#do-id-partial-closeout-state]
<!-- /agent:queue -->
###
<!--
Verify [#ipcfullprompt-recur2]
---
The last run had a code fence stripped away by agent-doc.
#spec-test-build-install-commit-push
---
The completed queue item was not removed from the queue. #mermaid
#spec-test-build-install-commit-push
---
equityfundingsource.md had the full-document IPC duplicate corruption. Then a new full-document IPC deletion under the prompt corruption happened. Both several times while editing the prompt.
#spec-test-build-install-commit-push
---
Please review the agent-doc, claude, codex logs. What are #next-steps to fix bugs?
---
---
As I was typing into the comment below `/agent:exchange`, the full-document IPC corruption happened, then the duplicate line happened, then the full-document IPC corruption happened again.

In addition to diagnosing and fixing the issue, please provide a mermaid showing how the issue occurred and where in the agent-doc lifecycle the full-document IPC corruption can occur. Can we mathematically eliminate all possibility of the full-document IPC corruption from occurring?

#spec-test-build-install-commit-push
-->

## Backlog

<!-- agent:backlog priority queue=prepend -->
- [ ] [#preset-item-id-collision-enforce] [recommended] Escalate #preset-item-id-collision from a preflight warning to hard enforcement (the riskier half, deferred to design carefully against over-blocking live sessions): (1) mutation-time rejection — write --pending-add / --pending-add-after / etc. with an explicit custom id (id=X or [#X]) that collides with a prompt_presets key or an existing active item id fails closed; (2) fail-closed dispatch — preflight/session-check block dispatch on an existing collision with the rename repair hint, surfaced by route/JB as a document-repair error not a generic agent failure; (3) queue generation refuses to emit do [#id] for an ambiguous id. The identity registry + detection + preflight warning + 3 regressions already landed (preflight::detect_identity_collisions). Plan: tasks/agent-doc/plan-preset-item-id-collision.md.
- [ ] [#free-text-queue-skip-diagnostic] [recommended] Polish from #free-text-queue-owner-consume plan point 4: when queue consumption is skipped, the closeout still prints the generic '[queue] skipped consumption because the active prompt did not target the queue head'. For an id-backed head with no completion flag, name the missing proof source and the supported repair (--done/--pending-gate/--pending-edit <id>); for a free-text head edge case, point to the answered-response path. Replace the single generic line at write.rs ~1460/1469 with a shape-aware diagnostic. Plan: tasks/agent-doc/plan-free-text-queue-owner-consume.md (point 4).
- [ ] [#do-id-partial-closeout-state] [recommended] Partial-closeout WARN guard landed (check_partial_closeout_state_guard: a do[#id] turn whose response shows shipped + remaining-live-work signals with the directed id still open advises --pending-edit/--pending-gate narrowing; suppressible by a no-partial-closeout-guard response marker; FlowCore guard_ budget 35->40). Remaining: per-id edit-vs-gate tracking in cycle_state, opt-in strict mode, next-phase queue convergence, and the runbook note that #spec-test-commit-push completes a repo phase without a live/deploy item. Plan: tasks/agent-doc/plan-do-id-partial-closeout-state.md.
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
- [/] [#codex-owned-pane-stop-hook-live-verify] [recommended] Gated on a live Codex pane -- the remaining phases of #codex-owned-pane-prompt-miss-followups after phase 2a landed: (2b) a live Codex Stop-hook regression asserting the owned_pane_self_invocation diagnostic forces an in-pane response body before final chat output, and (3) live-verify a real Codex same-pane run with an unresolved exchange prompt. Plan: tasks/agent-doc/plan-codex-owned-pane-prompt-miss.md.
- [/] [#zpc2] #ipc-corruption-live-repro-lazily-rs [recommended] Live repro of IPC scaffold duplication on tasks/software/lazily-rs.md while typing during finalize --stream. Full Backlog/Queue/Review duplicated plus two escaped exchange-close markers. Partial user typing embedded in wrong positions. Matches detect_duplicated_scaffold(). Plans: tasks/agent-doc/plan-full-document-ipc-corruption-typing-prompt.md, tasks/agent-doc/plan-prompt-duplicated-while-typing.md.
- [/] [#t41j] [recommended] #jb-run-agent-doc-busy-wait-deadlock live-verify (gated on a live busy pane): fix landed (submodule 89c357d5 — busy_dispatch_only_should_wait_for_ready skips the --wait-for-ready start override on a busy active turn + the (claude, active claude turn) case in dispatch_active_turn_queue_source). With the installed binary, click JB Run Agent Doc on a doc whose Claude actor is mid active-turn (not running /loop) and confirm it returns immediately (no 60s route_wait_for_ready_override block), appends the prompt to agent:queue auto, and the supervisor idle-queue watch drains the head on busy->idle. Check ops.log for the enqueue and the absence of a 60s wait.
- [/] [#queue-clear-unrun-items] [recommended] #agent-doc-bug 2026-05-31: active `agent:queue auto` heads can be deleted/cleared without execution proof during same-pane/recovery closeout. Live repro `src/boost-client/tasks/monsterrodholders.md`: queue heads `#convqa-rerun`, `#hydroapproval`, `#nbapproval`, `#shopcachewatch`, `#shoplabelgate`, and `#accessorymargin` were removed while the matching backlog items stayed open and no per-head closeout ran. Add queue-removal invariants so every removed head must have explicit consumed/done/gated/struck/stale-user-edit proof, and make reset/commit/session-check fail closed on unproved deletion. Plan: `tasks/agent-doc/plan-queue-clear-unrun-items.md`.
- [/] [#jb-busy-queue-deadlock-live-verify] [recommended] Live-verify the supervisor idle-queue watch (#jb-run-agent-doc-busy-queue-dispatch-deadlock): with the installed binary, run JB Run Agent Doc on a doc whose authoritative actor is mid active-turn (Codex or a Claude session not running /loop), confirm the route appends to agent:queue auto + returns Ok, then when the pane returns to idle the supervisor idle-queue watch injects agent-doc <FILE> once and drains the head with NO duplicate injection into the active turn and no hot-loop. Check the session log for idle_queue_watch_drain. Gated on a live busy pane. Source: submodule 4b3f4835 (idle_queue_drain_decision + spawn_idle_queue_watch_thread in start.rs).
- [/] [#recovery-drift-apply-live-verify] [recommended] Live-verify the full one-command recovery (agent-doc repair --apply-recovery) on a real owner pane now that #recovery-drift-authoritative-side landed the authoritative-side determination: reproduce a queue_metadata_drift / sidecar_visible_drift closeout and confirm apply_closeout_recovery restores-from-HEAD for the spurious queue_active:false class and commits-forward otherwise, without the manual cancel->reset->commit sequence. Gated on a live owner pane (risky in a racy multi-session repo). Parent plan: tasks/agent-doc/plan-recursive-repair-state-drift.md.
- [/] [#opencode-active-turn-reopen-auto-retry-on-idle] [recommended] When a plain dispatch-only Run Agent Doc reopen is refused because the OpenCode (or Codex) pane shows an active turn, route currently fails closed and the operator must re-click once the turn ends. Optional refinement: watch the pane and auto-fire the bare reopen once it returns to a dispatch-ready/idle composer, so the operator does not have to retry manually. Must stay a plain reopen (no prompt to queue) and respect the existing no-inject-into-active-turn invariant. Lower priority than the live-verify items.
- [/] [#jb-run-agent-doc-busy-feedback-live-verify] [recommended] Live-verify the busy-pane Run Agent Doc feedback fix (#jb-run-agent-doc-command-route-miss) end to end: install JB plugin 0.2.143 (build/distributions/agent-doc-jetbrains-0.2.143-signed.zip) + restart the route-owned agent-doc process to load the new binary, then run Run Agent Doc on a doc whose Codex pane is mid active-turn and confirm an immediate 'session still running' Event Log notification (no silent 60s wait, no 4x retry). Source landed submodule e1735fa2.
- [/] [#codex-owned-pane-prompt-miss-followups] [recommended] Follow-ups from the #codex-owned-pane-prompt-miss fix. Phase 1 (structured owned_pane_self_invocation preflight result) and phase 2a (in-pane Codex guidance + preflight_emits_owned_pane_self_invocation_for_active_queue_head regression) LANDED 2026-05-31 (submodule 407b0825). Remaining (gated — needs a live Codex pane): (2b) a live Codex Stop-hook regression asserting an in-pane response body before final chat output (the deterministic session-check backstop already landed in phase 1); (3) live-verify a real Codex same-pane run with an unresolved exchange prompt. Plan: tasks/agent-doc/plan-codex-owned-pane-prompt-miss.md.
- [/] [#codex-owned-pane-auto-queue-stuck] [recommended] Codex-owned `agent-doc <FILE>` direct invocation from the same pane can leave an active auto-queue turn stuck: the recursive guard avoids child dispatch, but the ready queue head is not handed to the owning Codex turn and no response is written. Live repro 2026-05-31 `src/boost-client/tasks/monsterrodholders.md`: `queue_active: true` + `agent:queue auto` head `re [#374n]...`; same-pane run printed `Submitting to codex...` then `recursive direct invocation would deadlock`, leaving queue/boundary drift and the head unprocessed. Fix with early owner-pane queue handoff, durable queue-target proof, no-drift guard, and Codex final-gate recovery. Plan: tasks/agent-doc/plan-codex-owned-pane-auto-queue-stuck.md.
- [/] [#backlog-queue-sync-attribute] [recommended] Add a queue=sync|append|prepend attribute to agent:backlog / agent:icebox: queue=sync replaces agent:queue content with a do [#id] list for every item; queue=append adds item ids not already in agent:queue; queue=prepend prepends them. Composable with #backlog-priority-attribute (prioritize first, then regenerate the queue), automating the manual 'regenerate the queue to the new priority' step that currently has to be done by hand after a reorder. Requested 2026-05-30.
- [/] [#backlog-priority-attribute] [recommended] Add a priority attribute to agent:backlog / agent:icebox / agent:queue that orders the item list by priority (a component-level priority flag and/or per-item priority markers), so the binary maintains priority order instead of requiring manual --pending-reorder each cycle. Automates the by-hand reprioritization done in #next-steps cycles. Composable with #backlog-queue-sync-attribute; sibling of #ah0s (explicit-position primitives). Requested 2026-05-30.
- [/] [#stash-session-ttl-prune] [recommended] Optional opt-in hygiene to cap stash-session accumulation: reap agent-doc stash panes whose session has been idle in the stash beyond a configurable TTL. Distinct from the existing orphan reap (resync.rs step 8), which only kills unregistered + no-supervisor + no-owned-file panes; live registered/supervisor-backed sessions are correctly never auto-killed, which is why ~9 agent-loop agent-doc panes accumulate in the shared session-0 stash. Must be opt-in (frontmatter/.agent-doc config) + live-verified before enabling (destructive). Relates to #cross-project-stash-pane-condition and #jb-busy-queue-ux-feedback.
- [/] [#plugin-cleanup-menu-command] [recommended] Expose session cleanup as agent-doc plugin-menu commands (JetBrains + VS Code): a 'Resync / Fix Sessions' action wrapping agent-doc resync --fix and a 'GC Stale Sessions' action wrapping agent-doc gc, so operators can clear stale routing/registry state and reap orphaned panes from the IDE without dropping to a terminal. Thin event-reporter pattern — the plugin shells the existing CLI/FFI; no cleanup logic in the plugin. Pairs with #stash-session-ttl-prune (auto hygiene) and #jb-busy-queue-ux-feedback (notification classification).
- [/] [#route-drain-all-surface-live-verify] [recommended] Live-verify the route-drain all-surface reap (submodule b239988b / superproject 4c7b06b7f) resolves the JB Run Agent Doc "completed tracked item blocks dispatch" retry: with the installed binary, reproduce a closeout that leaves a completed [x] item in review or icebox on an IDE-owned doc, then run JB Run Agent Doc once and confirm it dispatches without a manual repeat. Also exercises the #pending-gate-snapshot-desync snapshot re-sync (ab074710) under the route path.
- [/] [#codex-final-response-live-verify] [recommended] Opportunistic live re-verification of the #codex-final-response-not-written guard: do a real Codex direct/manual run that commits side-effect work, confirm session-check (via the .codex Stop hook) fails closed with the 'no assistant response body was captured' message + 'agent-doc write --commit' recovery when no ### Re: close-out was written, and that a normal Codex turn with a response stays OK. Guard + deterministic regressions already landed (session_check.rs check_committed_without_response_body_guard).
- [/] [#codex-owned-pane-prompt-miss] [recommended] Codex-owned `agent-doc <FILE>` direct invocation from the same pane can abandon recursive dispatch without processing unresolved exchange prompts. Live repro 2026-05-30: `src/boost-client/tasks/monsterrodholders.md` contained `Please assist in placing GA4 Analytics credentials in passage.`, but the same-pane recursion guard refused dispatch and no response was written. Fix with earlier owner-pane self-invocation detection, unresolved-prompt preservation, and Codex guidance/final-gate recovery. Plan: tasks/agent-doc/plan-codex-owned-pane-prompt-miss.md.
- [/] [#stash-pane-promote-on-focus] [recommended] A live-owner pane residing in the stash tmux window must be promoted/reparented into the 0:agent-doc window when the document is focused/selected, not merely selected in place. Live repro 2026-05-30 agent-doc-bugs2.md: pane %8 (live owner) is stuck in the stash window; focus/select does not move it. The focus live-owner fix (submodule e6dafa52) now selects the correct pane but does not reparent it. Tied to #cross-project-stash-pane-condition and #plugin-cleanup-menu-command.
- [/] [#clear-stops-running-process] [recommended] JB Clear Session Context / Claude Code /clear must stop the running agent-doc child so a following JB Run Agent Doc is not blocked by a stale busy actor. Live repro 2026-05-30 agent-doc-bugs2.md: after /clear, a still-running process blocked Run Agent Doc. Overlaps #cancel-orphans-preflight-cycle and the operator_clear busy-projection path in session_actor_cmd.rs.
- [/] [#jb-run-agent-doc-response-queue-contamination] [recommended] JetBrains `Run Agent Doc` can add assistant response prose to `agent:queue auto` after compact/repair drift. Live repro 2026-05-30 `src/boost-client/tasks/monsterrodholders.md`: queue gained `- Yes. I drove the already-authenticated Google Ads browser session...`, copied from `### Re: #gads106demote`, alongside real `do [#nbsearch]`/`#bidstrat`/`#pdpconv` entries. Require queue-source provenance and reject assistant response bodies as queue candidates. Plan: tasks/agent-doc/plan-jb-run-agent-doc-response-queue-contamination.md.
- [/] [#jb-compact-repair-left-uncommitted] [recommended] Compact-output repair can leave the corrected session document dirty instead of committing through a binary-owned repair boundary. Live repro 2026-05-30 `src/boost-client/tasks/monsterrodholders.md`: visible exchange correction differs from malformed `HEAD`, creating mixed visible/HEAD/snapshot state for later JetBrains actions. Repair should commit atomically or fail closed with an exact recovery path. Plan: tasks/agent-doc/plan-jb-compact-repair-left-uncommitted.md.
- [/] [#jb-compact-malformed-response-commit] [recommended] JetBrains `Compact Exchange` can commit malformed compact output. Live repro 2026-05-30 `src/boost-client/tasks/monsterrodholders.md` / commit `f8fe36b`: compact summary line was committed with user prompt prefix `❯`, and archived response content remained in the live exchange shape. Add post-compact structural validation and fail closed before commit. Plan: tasks/agent-doc/plan-jb-compact-malformed-response-commit.md.
- [/] [#prompt-preempts-auto-queue] [recommended] Unanswered exchange prompts must run before active `agent:queue auto` continuation, even when the prompt was accidentally baselined into the `.agent-doc` snapshot. Live repro 2026-05-30 `src/boost-client/tasks/monsterrodholders.md`: `What are #next-steps to complete review items?` was visible after the exchange boundary and duplicated in `agent:queue auto`, but the snapshot already contained the exchange prompt, so the next diff only saw queue bookkeeping and the prompt did not run. Fix: add snapshot-independent unresolved exchange prompt detection, make preflight/direct-run prefer it over queue synthetic diffs, prevent route queue snapshot recovery from absorbing unanswered exchange prompts, and defer Codex/Claude queue continuation while such a prompt exists. Plan: tasks/agent-doc/plan-prompt-preempts-auto-queue.md.
- [/] [#queue-user-edit-overwrite] [recommended] User-authored agent:queue edits can be silently overwritten/deleted by write/reset/commit queue convergence. Live repro 2026-05-30 boost-client monsterrodholders.md: user added do [#gscaccess] at queue head; agent-doc repeatedly removed/reverted it during closeout/snapshot repair. Fix by tracking queue edit provenance/epochs, preserving explicit queue edits unless the current response consumes that exact head, and failing closed on stale IPC/snapshot queue rewrites. Plan: tasks/agent-doc/plan-user-queue-edit-preservation.md.
- [/] [#ipcfullprompt-recur2] CURE ONLY (capture/detection split out to #ipcfullprompt-capture, done). Editor-side PatchWatcher.setText PREVENTION: stop the live editor buffer from producing full-document/full-tail duplication or deletion while a prompt is being typed. Gated on the transport/payload packet from idea.log (enable #com.github.btakita.agentdoc debug logging) to pin the exact setText call shape. Add a SimWorld/Kotlin regression for the prevention once the fix lands; bump pluginVersion past 0.2.137 and reinstall the signed plugin. Stays open until prevention lands and is live-verified. Plan: tasks/agent-doc/plan-full-document-ipc-corruption-typing-prompt.md.
- [/] [#finalize-retry-ipc-response-duplication] [recommended] Multi-retry finalize + live IPC duplicates the response and prefixes its body lines with the user-prompt marker. Live repro 2026-05-29 on agent-doc-bugs2.md: a closeout needing several finalize/write retries (lint-gate / session-check / live_prompt_drift re-runs) with the IDE IPC listener active and concurrent editing left a duplicated response block plus agent body lines wrongly prefixed (uncommitted; HEAD stayed clean; manual git checkout repaired). Fix direction: idempotent retry convergence on IPC already_applied, never prefix agent response body as a prompt, and repair/session-check auto-dedup. Relates to #prompt-duplicated-while-typing, #duplicate-response-after-commit, #ipc-dup-tracking-double-merge (plugin 0.2.138 not yet installed during repro). Plan: tasks/agent-doc/plan-finalize-retry-ipc-response-duplication.md.
- [/] [#ipc-dup-tracking-double-merge] PromptPoller double-tracking fix (plugin 0.2.138, submodule 32fb0b98 / superproject d585a33de): addFile keyed trackedFiles by project-relative path while handlePollResults keyed by "$cwd:$file", double-tracking the same VirtualFile (live: corky.md, brandon-cinquegrana.md, agent-doc-bugs2.md; trackedFiles=13 vs ffiTracked=9). Two divergent baselines -> mergeOrReload ran twice per change = full-document duplication path. Fixed by keying both via trackingKey(VirtualFile.path); convergence test + AGENTS.md invariant. GATED on live verify: install signed 0.2.138, restart, confirm no recurring duplication (corky.md). Exact setText payload uncaptured (JB debug logging was OFF). Relates to #ipcfullprompt-recur2, #adoc-queue-ipc-buffer-divergence, #prompt-duplicated-while-typing.
- [/] [#jb-load-tmux-window-verify] [recommended] Live-verify the new JB Load Tmux Window action (plugin 0.2.136): install the signed zip, confirm it autostarts/loads the tmux pane for a focused doc whose pane is not up (e.g. corky.md). If the autostart sync genuinely fails to spawn the pane in that case (vs. by-design passive open), capture the route/sync logs as a separate autostart bug.
- [/] [#jb-stale-busy-idle-footer] Three JB Run Agent Doc / Clear Session Context reports queue-instead-of-dispatch or block Clear while the live pane footer shows a Claude Code idle composer: monsterrodholders.md (actor gen 233, pane %19), equityfundingsource.md (gen 35, pane %3), tsift.md (pane %21, footer 22.4K (11%) ctrl+p commands). The tsift.md case is the strongest stale-busy candidate — Clear was blocked though the footer indicates idle. Fix: route.rs / session_actor_cmd.rs busy projection should treat the Claude Code idle-composer footer (ctrl+p commands / context % used) as idle direct evidence to repair a stale busy actor/lease projection before queuing or blocking, mirroring the existing OpenCode context % used idle-composer rule. Relates to #run-agent-doc-stale-busy-replay and #snrun (plan tasks/agent-doc/plan-run-agent-doc-snappy-auto-remediation.md).
- [/] [#adoc-queue-ipc-buffer-divergence] Root cause #2 IMPLEMENTED (submodule c483080a; pointer 4703f9788): live editor-buffer convergence after queue halt/drain. New core fn component::converge_queue_auto + agent_doc_converge_queue_auto FFI (C-int ABI for stable JNA); ipc_socket::send_queue_convergence carries queue_auto opening-tag state + queue_active frontmatter; preflight run_queue_maintenance pushes convergence through an active IPC listener after each halt/drain disk write (best-effort, non-fatal). JB plugin 0.2.137 parses queue_auto and applies via NativePatching.convergeQueueAuto in Document-API + VFS paths. Deterministic SimWorld repro queue_maintenance_converges_live_ipc_buffer_on_item_modified_halt + Kotlin ParsePatchJsonQueueAutoTest; make check green; both binaries built+installed (symbol verified in release cdylib); spec 07-orchestration-commands updated. GATED on live verification: install signed plugin 0.2.137, restart the route-owned agent-doc process to load the new binary, then confirm a real IDE-owned session no longer re-diverges (no recurring agent:queue diff / stuck_captured_cycle / inactive_queue_residue) after a consume->halt cycle without a raw git checkout. Plan: tasks/agent-doc/plan-queue-ipc-drift.md.
- [/] [#cancel-orphans-preflight-cycle] Canceling a run then invoking Run Agent Doc fails closed because the explicit cancel does not abandon the orphaned preflight_started cycle (route waits for staleness). Distinct from the stale-busy actor/lease items: this is per-document cycle_state left open by cancel. Plan: tasks/agent-doc/plan-cancel-orphans-preflight-cycle.md (recommend Option A — JB cancel emits a signal that abandons the no-capture preflight_started cycle). Reported 2026-05-29.
- [/] [#exchange-prompt-dropped-on-merge] User-authored exchange prompt (observed: "go") silently dropped during an IPC content_ours merge while the IDE held ipc.sock — never reaches snapshot/HEAD, assistant never responds, next preflight reports no_changes. Same drift family as #adoc-queue-ipc-buffer-divergence / #duplicate-response-after-commit but the victim is user input. Repro 2026-05-29 on src/boost-client/tasks/monsterrodholders.md (dropped the "go" authorizing #gfwconnect; assistant replied "nothing changed" for two cycles). Plan: tasks/agent-doc/plan-exchange-prompt-dropped-on-merge.md (recommend Option A — union user prompt lines / fail closed + add dropped_prompt_guard to session-check).
- [/] [#duplicate-response-after-commit] Late-IPC reposition signal re-inserts already-committed response body into working tree, wrapped in duplicate boundary markers. Symptom: next preflight interrupts with "direct response patchback without agent-doc cycle"; agent-doc write --commit returns "empty response — nothing to write"; only manual Edit-based dedup recovers. Repro: tasks/dreamable/separation.md cycle after commit 231eafc83 on 2026-05-28. Plan: tasks/agent-doc/plan-duplicate-response-after-commit.md (recommend Option A — both detect over-application in session-check and make IPC reposition idempotent).
- [/] [#prompt-duplicated-while-typing] [/] [#prompt-duplicated-while-typing] User prompt duplicated in the live editor while typing. PARTIAL FIX 1 (v0.34.0, submodule 2a6f60e5): prefix-agnostic append dedup (strip_user_prompt_prefix) for the ❯-prefix mismatch shape. PARTIAL FIX 2 (this cycle): L1 buffer-snapshot typing race — component::append_patch_already_present now recognizes when a synthesized patch carries an EARLIER keystroke-snapshot of the live region (per-line prefix alignment via patch_is_typing_snapshot_of) and no-ops instead of re-appending the divergent tail. Confirmed live in tasks/resume.md 2026-05-29: half-typed '- [#s93y]: Add to' snapshot vs live '...Add to resume' broke the old contains() substring check and re-appended the entire tail (close marker + Queue/Backlog/Done duplicated). Boundary 70bccf9b stayed intact and no new boundary was minted, ruling out the reposition path. Tests: append_patch_dedupes_midblock_typing_snapshot, append_patch_typing_snapshot_does_not_collapse_distinct_lines, append_with_boundary_does_not_duplicate_typing_snapshot. Both binaries built+installed; cdylib lib-installed for JB hot-reload. GATED on live re-verify (type a prompt into resume.md during an in-flight cycle). RESIDUAL: the exact editor-side full-tail concatenation downstream of the dedup miss was not statically traced — the dedup no-op short-circuits the append entirely so it should not surface, but confirm live. Plan: tasks/agent-doc/plan-prompt-duplicated-while-typing.md.
- [/] [#queue-auto-no-continue] Queue auto trigger does not continue after finalize session-check warning about pending shadow items. Steps to reproduce: (1) Session document with queue_active=true, queue_trigger=auto, and multiple queued items. (2) finalize --stream completes cycle but session-check issues pending shadow warning for items outside live agent:backlog. (3) Queue consumes head item but auto trigger does not fire for next item. (4) agent-doc write --commit with --done reaps the item, queue reports 9 remaining but auto continues stop. Expected: queue auto should continue to next item after successful closeout. Actual: queue stops after first item. Session: tasks/software/tsift.md. agent-doc version: 0.34.0. The pending shadow warnings about items appearing both inside and outside agent:backlog may be blocking the auto-continue path.
- [/] [#snrun] [recommended] Implement tasks/agent-doc/plan-run-agent-doc-snappy-auto-remediation.md: make JB Run Agent Doc distinguish rescued-stash accepted-only proof misses, active-turn busy collisions, and stale starting projections; add route/plugin regressions plus live replay evidence.
<!-- /agent:review -->

<!-- agent:icebox -->
- [ ] [#misfire-recovery-snapshot] Pre-auto-run recovery snapshot/tag (mirroring `compact`'s pre-mutation tag) so a destructive auto-run misfire is recoverable without git/sidecar archaeology. Proposed fix #4 of `#backlog-auto-marker-misfire` (now IMPLEMENTED for fixes #1–#3). Parked: only worth doing if a new auto-run path can mutate the backlog in place — currently the misfire is inert (queue-scoped) and surfaced (misplaced_component_attr warning). Plan: tasks/agent-doc/plan-backlog-auto-marker-misfire.md.
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
