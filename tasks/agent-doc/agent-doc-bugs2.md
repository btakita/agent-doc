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
  '#advance-review': Go through the review items. Add actionable backlog items to complete or advance the review items.,
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

completed
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260619-213455.md`*

Compacted content:
- Archived 4 response topic(s): Fix equityfundingsource.md "Mixed signals"; Clean up the noise queue heads; Fix live IPC supervisor races on direct queue edits + clean noise after restart; 1 more
- Prior summary/context: prior compacted content: Archived 5 response topic(s): queue drain state; JB `Run Agent Doc` is not submitting; #docd...

### Re: Fix the root cause of already-done / noise heads flooding the queue — opus-4-8

Found and fixed the real root cause. It was never about a stale IDEA buffer — it was that `queue prune-noise` **couldn't see most of the noise**, so nothing could clear it on disk and the convergence kept re-pushing it to the editor.

**Root cause.** The queue has two parsers that disagreed about what a "head" is:
- `queue::parse` (what preflight surfaces) recognizes `:round_pushpin:`/`---`-fenced blocks and bare ```` ``` ```` console pastes.
- `item_nodes` (the *strike* path that `prune-noise`/`consume`/dedup use) recognizes **only bulleted `- …` lines and skips fenced code**.

So every operator-pasted console dump — the bulk of the flood — was invisible to the strike path. `prune-noise` reported "nothing to prune" while 18 KB of pasted blocks sat on disk forever. A `#5eq8` stray id inside a console dump even made it look like a *drainable* head.

**The fix (`#qnoise-multiline-strike`, agent-doc 0.34.15):**
1. New `queue::parse_spans` — one byte-range-aware source of head segmentation. `prune-noise` now excises multiline `---`-fenced noise `Prompt` blocks **and** bare-```` ``` ````-fenced `Freeform` console pastes by exact range, alongside the existing bulleted strike. `is_noise_freeform_line` preserves real `---`/`~~~` separators and `re [#id]` references.
2. The drainability classifier now demotes any **multi-line** head text to noise (kills the `#5eq8`-in-a-dump false positive) — but a single-line `do [#id]` that merely happens to be `---`-wrapped stays drainable, so no real directive is lost. Drain, the `queue_stale_noise_lines` counter, and `prune-noise` now agree.

**Verified on the actual flooded doc (on a copy):** `prune-noise` excised **61** noise entries — 0 ``` fences / `:round_pushpin:` / `agent:boundary` left — while preserving every id-backed directive, including the `---`-wrapped `#tsiftmdcrash`. New regression test + full queue suite (479) + `make check` green.

This directly unblocks `#prunenoise-live` and the "noise heads never auto-deleted" half of `#sqedit-race`. Next: I'll install 0.34.15 + recycle the supervisor, then run `queue prune-noise` on this live doc to clear the flood with matched binaries.
<!-- agent:boundary:ddfe2463 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" priority go -->
- [#sqedit-race]
- [#prunenoise-live]
- [#qpausemix-verify]
- In equityfundingsource.md, I typed in queue items and agent-doc duplicated the queue items.
- In equityfundingsource.md, I killed the pane and autostarted a new pane to resolve a wedge issue. After restarting, JB `Run Agent Doc` stalled.
- Some JB `Run Agent Doc` operations do not submit to Claude Code.
---
:round_pushpin: Fix the root cause of monsterrodholders.md HEAD issue with already done items.
```
  The one thing I can't fix from here — please reload the doc in IDEA
  The queue keeps showing 11 heads of duplicates of already-completed prompts because the live IDEA editor buffer is
  stale and re-injects them every cycle. The supervisor is fresh, so admin recycle won't help — only a file reload in
  IDEA clears it. Until then the duplicate churn persists no matter how many times the queue is "drained."
  Everything in the queue is done / resolved / gated:
  - #134j resolved · WP-layout + localhost + relative-links + corky done & committed
  - #tf52 (prod cutover) — operator-gated on your Cloudflare auth
  - #bk92 remainder (water temp / EPA / state records) + #3yp0 (strategy) — backlog; the next ones need external data
  sources or are Ron-driven direction, better scoped explicitly than auto-drained.
  Reload IDEA and the queue will clear; or point me at the next concrete piece (e.g. water-temp source, or wiring W2/CF
  deploy config) and I'll take it.
```
---
---
:round_pushpin: JB `Run Agent Doc` on equityfundingsource.md
```
resync: pruned 5 stale session(s)
[route] target tmux session: 0
Error: project controller command `dispatch` failed: dispatch blocked for tasks/professional/equityfundingsource.md: failed_stage=queue_paused reason=qflood: supervisor idle-watch is re-injecting agent-doc triggers on an already-drained queue (4 consecutive no-op /agent-doc cycles). All real work shipped+deployed (c9aa3a6 / worker fb99f394); only the gated #5eq8 (design-blocked) remains, which must not auto-drain. Leaving paused until operator adds new work / un-gates #5eq8. receipt_id=3934 ui_outcome_contract=ui-outcome-v1 ui_outcome=blocked_with_exact_unblocker ui_outcome_class=blocked next_action=follow_unblocker unblocker=resume_or_clear_queue_control trigger_bytes=52 trigger_sha256=a4b904c65fa35ab9631efaf329f3f2b595b448d1976b7c8738c6ba9b4a3af3c9
```
---
- [#sqedit-race]
- [#prunenoise-live]
:pushpin: JB `Run Agent Doc` on equityfundingsource.md did not start. It should have started.
```
[route] target tmux session: 0
[route] activated existing inactive agent:queue head "do [#e9fy]" for tasks/professional/equityfundingsource.md (already_present=true, activated=true) before dispatch-only reopen
Error: project controller command `dispatch` failed: dispatch blocked for tasks/professional/equityfundingsource.md: failed_stage=queue_paused reason=qflood: supervisor idle-watch is re-injecting agent-doc triggers on an already-drained queue (4 consecutive no-op /agent-doc cycles). All real work shipped+deployed (c9aa3a6 / worker fb99f394); only the gated #5eq8 (design-blocked) remains, which must not auto-drain. Leaving paused until operator adds new work / un-gates #5eq8. receipt_id=4035 ui_outcome_contract=ui-outcome-v1 ui_outcome=blocked_with_exact_unblocker ui_outcome_class=blocked next_action=follow_unblocker unblocker=resume_or_clear_queue_control blocked_head_bytes=10 blocked_head_sha256=530312e6f606f7822758890ee49c7c43ee49243070f547326736709d7bcbd7f4 trigger_bytes=52 trigger_sha256=a4b904c65fa35ab9631efaf329f3f2b595b448d1976b7c8738c6ba9b4a3af3c9
```
---
:pushpin: do [#tsiftmdcrash]
---
:pushpin: Initial response for a turn renders numeric lists in reverse order:
```
4. **Review back-catalog dry-run** → #review-backcatalog step 1 — read-only segment count, no send.
3. **6/19 ROAS re-audit** → #pmax-pause-decision + #29h5 — read-only PMax + accessories audits; pause/prune stay operator calls.
2. **Post-6/14 GTM-v6 conversion QA** → #convfix-verify (this one **unblocks #nbsl2** non-brand Search) — read-only Ads+GA4 parity.
1. **6/19 offline-conversion re-check** → #2qrx-metrics-lag + #conv-backfill-615-617-verify — read-only Ads action 7649654590 metrics + Woo reconcile.
<!-- agent:boundary:a5b6cae3 -->
```
---
:pushpin: monsterrodholders.md sent duplicate uncommitted response
```
- :pushpin: **migrate** — folded the 8 legacy gated `agent:backlog` items into `agent:review` (clears `legacy_gated_in_backlog`).
- :pushpin: **Resolved 3 genuinely-finished reviews** → archived to `agent:done`: `#2qrx` (apply done; verify handed to archived `#2qrx-verify`), `#7m2s` (GTM v6 fix shipped 6/14; verify owned by open `#convfix-verify`), `#ef5h` (duplicate of archived `#fishloc-curated-rollout`; adapter blocker tracked by open `#3456`).
- :pushpin: **Kept 2 false-positive matches** (real open work): `#2b8q` (live operator-gated NB-Search follow-up — HydroWeb rebalance + phase-2 enable), `#conv-backfill-615-617-verify` (distinct 6/15–6/18 batch, now-due on/after today — not the 5/29–6/14 batch `#2qrx-verify` covered).
- :pushpin: **Kept 2 false-positive matches** (real open work): `#2b8q` (live operator-gated NB-Search follow-up — HydroWeb rebalance + phase-2 enable), `#conv-backfill-615-617-verify` (distinct 6/15–6/18 batch, now-due on/after today — not the 5/29–6/14 batch `#2qrx-verify` covered).
- :pushpin: **Resolved 3 genuinely-finished reviews** → archived to `agent:done`: `#2qrx` (apply done; verify handed to archived `#2qrx-verify`), `#7m2s` (GTM v6 fix shipped 6/14; verify owned by open `#convfix-verify`), `#ef5h` (duplicate of archived `#fishloc-curated-rollout`; adapter blocker tracked by open `#3456`).
- :pushpin: **migrate** — folded the 8 legacy gated `agent:backlog` items into `agent:review` (clears `legacy_gated_in_backlog`).
<!-- agent:boundary:7c96f9ce -->
```
---
:pushpin: JB `Run Agent Doc` on this file
```
[sync] recovered live pane %125 for session b26b9957 via supervisor pid 2252604
[sync] recovered live pane %126 for session 9342c5f7 via supervisor pid 2744553
resync: stash pane %93 is unregistered but still hosts live supervisor 62fe1f41-886c-4f3d-b6fa-48d46106ec27 — skipping kill
[sync] recovered live pane %125 for session b26b9957 via supervisor pid 2252604
[sync] recovered live pane %126 for session 9342c5f7 via supervisor pid 2744553
resync: stash pane %112 is unregistered but still hosts live supervisor tsift-v0.1 — skipping kill
[route] target tmux session: 0
[route] authoritative actor for tasks/agent-doc/agent-doc-bugs2.md projected busy but the live pane proves a dispatch-ready prompt (generation 547); repairing stale busy projection to ready and dispatching instead of queuing
Error: project controller command `dispatch` failed: dispatch blocked for tasks/agent-doc/agent-doc-bugs2.md: failed_stage=queue_paused reason=broken editor IPC: content_ours adoption (live_prompt_drift, #semmerge) loses every exchange-finalize response AND consumes queue heads without landing answers (recovered last response via raw git). Remaining heads are already-answered/filed (virtuousai crash=#routedeadshell), cross-doc (equityfundingsource), or operator-gated. Pausing to stop churn. Resume in a clean session (refresh IntelliJ buffer + recycle binary) and take #smconv (the converge.rs fix for this very bug) first. receipt_id=3986 ui_outcome_contract=ui-outcome-v1 ui_outcome=blocked_with_exact_unblocker ui_outcome_class=blocked next_action=follow_unblocker unblocker=resume_or_clear_queue_control blocked_head_bytes=17 blocked_head_sha256=c5f1145ffb6ebc69ee2482a58184cc574633d8b472888314df6cbc6159cfe443 trigger_bytes=45 trigger_sha256=ce2c7a444f4ae0b6cfce11a6ceac977be5b3811138cfe31a7252a021e9b2c9ad
```
---
:pushpin: More IPC/CRDT drift on virtuousai.md
```
  Note on the session: this hit heavy IPC/CRDT drift — finalize repeatedly raced the live JetBrains editor buffer
  (live_prompt_drift_after_preflight, then a post-commit worktree merge that spliced my stealth bullets into the wrong
  response). I hand-rebuilt the exchange cleanly and reset the sidecars to land a correct commit. The letter and
  recommendation content were unaffected.
```
```
● The editor IPC is persistently wedged this turn (no_ack) — identical to the previous turn, where the queued response
  landed automatically on the next /agent-doc trigger once the JB plugin applied the patch. I've retried the
  editor/CRDT path (finalize → commit → write --commit); all leave the response safely queued. Per the closeout rules I
  will not force a direct disk write and will not auto-loop on this unproven-IPC state.
```
---
:pushpin: JB `Run Agent Doc`
```
resync: pruned 5 stale session(s)
[route] target tmux session: 0
Error: project controller command `dispatch` failed: dispatch blocked for tasks/professional/equityfundingsource.md: failed_stage=queue_paused reason=qflood: supervisor idle-watch is re-injecting agent-doc triggers on an already-drained queue (4 consecutive no-op /agent-doc cycles). All real work shipped+deployed (c9aa3a6 / worker fb99f394); only the gated #5eq8 (design-blocked) remains, which must not auto-drain. Leaving paused until operator adds new work / un-gates #5eq8. receipt_id=3977 ui_outcome_contract=ui-outcome-v1 ui_outcome=blocked_with_exact_unblocker ui_outcome_class=blocked next_action=follow_unblocker unblocker=resume_or_clear_queue_control trigger_bytes=52 trigger_sha256=a4b904c65fa35ab9631efaf329f3f2b595b448d1976b7c8738c6ba9b4a3af3c9
```
---
:pushpin: JB `Run Agent Doc` stalled on this document.
```
[sync] recovered live pane %92 for session b26b9957 via supervisor pid 1535801
[sync] recovered live pane %105 for session 9342c5f7 via supervisor pid 1101234
resync: stash pane %93 is unregistered but still hosts live supervisor 62fe1f41-886c-4f3d-b6fa-48d46106ec27 — skipping kill
[route] target tmux session: 0
[route] authoritative actor for tasks/agent-doc/agent-doc-bugs2.md projected busy but the live pane proves a dispatch-ready prompt (generation 535); repairing stale busy projection to ready and dispatching instead of queuing
Error: project controller command `dispatch` failed: dispatch blocked for tasks/agent-doc/agent-doc-bugs2.md: failed_stage=queue_paused reason=broken editor IPC: content_ours adoption (live_prompt_drift, #semmerge) loses every exchange-finalize response AND consumes queue heads without landing answers (recovered last response via raw git). Remaining heads are already-answered/filed (virtuousai crash=#routedeadshell), cross-doc (equityfundingsource), or operator-gated. Pausing to stop churn. Resume in a clean session (refresh IntelliJ buffer + recycle binary) and take #smconv (the converge.rs fix for this very bug) first. receipt_id=3960 ui_outcome_contract=ui-outcome-v1 ui_outcome=blocked_with_exact_unblocker ui_outcome_class=blocked next_action=follow_unblocker unblocker=resume_or_clear_queue_control blocked_head_bytes=44 blocked_head_sha256=4ec56b3745f04f438f52f8bffc1d7fe8b9e8e0964028b4db7008fffeb4bf4ff5 trigger_bytes=45 trigger_sha256=ce2c7a444f4ae0b6cfce11a6ceac977be5b3811138cfe31a7252a021e9b2c9ad
```
---
:pushpin: JB `Run Agent Doc` on equityfundingsource.md
```
[route] target tmux session: 0
[preflight] review: 14 item(s), 14 gated
[route] active closeout for tasks/professional/equityfundingsource.md could not be drained before reroute; existing queue head "Why is the https://equityfundingsource.brian-takita.workers.dev/admin/pipeline?dialog=deal&dialog_deal_id=7a0c1463-27bb-4d7c-82f7-2281b3c008d3&deal_tab=application > Available deal > Application > Application progress > Accepted not checked?" remains queued behind the closeout ui_outcome_contract=ui-outcome-v1 ui_outcome=queued_behind_owner ui_outcome_class=ok next_action=wait_for_owner_turn_to_drain
```
- [#qpausemix-verify]
- :pushpin: do [#fbwire]
- :pushpin: do [#hap7]
---
:pushpin: Fix equityfundingsource.md "Mixed signals"
```
● Mixed signals: queue_paused: true (an explicit pause flag) coexists with queue_continuation_required: true. Let me
  check whether this is an operator-set pause (with a reason) or transient drain-coordination state before deciding
  whether to loop into the large Phase 4 UI work.
```
:pushpin: - :pushpin: do [#tsiftmdcrash]
:pushpin: monsterrodholders.md sent duplicate uncommitted response
```
- :pushpin: **migrate** — folded the 8 legacy gated `agent:backlog` items into `agent:review` (clears `legacy_gated_in_backlog`).
- :pushpin: **Resolved 3 genuinely-finished reviews** → archived to `agent:done`: `#2qrx` (apply done; verify handed to archived `#2qrx-verify`), `#7m2s` (GTM v6 fix shipped 6/14; verify owned by open `#convfix-verify`), `#ef5h` (duplicate of archived `#fishloc-curated-rollout`; adapter blocker tracked by open `#3456`).
- :pushpin: **Kept 2 false-positive matches** (real open work): `#2b8q` (live operator-gated NB-Search follow-up — HydroWeb rebalance + phase-2 enable), `#conv-backfill-615-617-verify` (distinct 6/15–6/18 batch, now-due on/after today — not the 5/29–6/14 batch `#2qrx-verify` covered).
- :pushpin: **Kept 2 false-positive matches** (real open work): `#2b8q` (live operator-gated NB-Search follow-up — HydroWeb rebalance + phase-2 enable), `#conv-backfill-615-617-verify` (distinct 6/15–6/18 batch, now-due on/after today — not the 5/29–6/14 batch `#2qrx-verify` covered).
- :pushpin: **Resolved 3 genuinely-finished reviews** → archived to `agent:done`: `#2qrx` (apply done; verify handed to archived `#2qrx-verify`), `#7m2s` (GTM v6 fix shipped 6/14; verify owned by open `#convfix-verify`), `#ef5h` (duplicate of archived `#fishloc-curated-rollout`; adapter blocker tracked by open `#3456`).
- :pushpin: **migrate** — folded the 8 legacy gated `agent:backlog` items into `agent:review` (clears `legacy_gated_in_backlog`).
<!-- agent:boundary:7c96f9ce -->
```
:pushpin: More IPC/CRDT drift on virtuousai.md
```
  Note on the session: this hit heavy IPC/CRDT drift — finalize repeatedly raced the live JetBrains editor buffer
  (live_prompt_drift_after_preflight, then a post-commit worktree merge that spliced my stealth bullets into the wrong
  response). I hand-rebuilt the exchange cleanly and reset the sidecars to land a correct commit. The letter and
  recommendation content were unaffected.
```
```
● The editor IPC is persistently wedged this turn (no_ack) — identical to the previous turn, where the queued response
  landed automatically on the next /agent-doc trigger once the JB plugin applied the patch. I've retried the
  editor/CRDT path (finalize → commit → write --commit); all leave the response safely queued. Per the closeout rules I
  will not force a direct disk write and will not auto-loop on this unproven-IPC state.
```
:pushpin: JB `Run Agent Doc` stalled on this document.
```
[sync] recovered live pane %92 for session b26b9957 via supervisor pid 1535801
[sync] recovered live pane %105 for session 9342c5f7 via supervisor pid 1101234
resync: stash pane %93 is unregistered but still hosts live supervisor 62fe1f41-886c-4f3d-b6fa-48d46106ec27 — skipping kill
[route] target tmux session: 0
[route] authoritative actor for tasks/agent-doc/agent-doc-bugs2.md projected busy but the live pane proves a dispatch-ready prompt (generation 535); repairing stale busy projection to ready and dispatching instead of queuing
Error: project controller command `dispatch` failed: dispatch blocked for tasks/agent-doc/agent-doc-bugs2.md: failed_stage=queue_paused reason=broken editor IPC: content_ours adoption (live_prompt_drift, #semmerge) loses every exchange-finalize response AND consumes queue heads without landing answers (recovered last response via raw git). Remaining heads are already-answered/filed (virtuousai crash=#routedeadshell), cross-doc (equityfundingsource), or operator-gated. Pausing to stop churn. Resume in a clean session (refresh IntelliJ buffer + recycle binary) and take #smconv (the converge.rs fix for this very bug) first. receipt_id=3960 ui_outcome_contract=ui-outcome-v1 ui_outcome=blocked_with_exact_unblocker ui_outcome_class=blocked next_action=follow_unblocker unblocker=resume_or_clear_queue_control blocked_head_bytes=44 blocked_head_sha256=4ec56b3745f04f438f52f8bffc1d7fe8b9e8e0964028b4db7008fffeb4bf4ff5 trigger_bytes=45 trigger_sha256=ce2c7a444f4ae0b6cfce11a6ceac977be5b3811138cfe31a7252a021e9b2c9ad
```
---
:pushpin: Fix equityfundingsource.md "Mixed signals"
```
● Mixed signals: queue_paused: true (an explicit pause flag) coexists with queue_continuation_required: true. Let me
  check whether this is an operator-set pause (with a reason) or transient drain-coordination state before deciding
  whether to loop into the large Phase 4 UI work.
```
:pushpin: monsterrodholders.md sent duplicate uncommitted response
```
- :pushpin: **migrate** — folded the 8 legacy gated `agent:backlog` items into `agent:review` (clears `legacy_gated_in_backlog`).
- :pushpin: **Resolved 3 genuinely-finished reviews** → archived to `agent:done`: `#2qrx` (apply done; verify handed to archived `#2qrx-verify`), `#7m2s` (GTM v6 fix shipped 6/14; verify owned by open `#convfix-verify`), `#ef5h` (duplicate of archived `#fishloc-curated-rollout`; adapter blocker tracked by open `#3456`).
- :pushpin: **Kept 2 false-positive matches** (real open work): `#2b8q` (live operator-gated NB-Search follow-up — HydroWeb rebalance + phase-2 enable), `#conv-backfill-615-617-verify` (distinct 6/15–6/18 batch, now-due on/after today — not the 5/29–6/14 batch `#2qrx-verify` covered).
- :pushpin: **Kept 2 false-positive matches** (real open work): `#2b8q` (live operator-gated NB-Search follow-up — HydroWeb rebalance + phase-2 enable), `#conv-backfill-615-617-verify` (distinct 6/15–6/18 batch, now-due on/after today — not the 5/29–6/14 batch `#2qrx-verify` covered).
- :pushpin: **Resolved 3 genuinely-finished reviews** → archived to `agent:done`: `#2qrx` (apply done; verify handed to archived `#2qrx-verify`), `#7m2s` (GTM v6 fix shipped 6/14; verify owned by open `#convfix-verify`), `#ef5h` (duplicate of archived `#fishloc-curated-rollout`; adapter blocker tracked by open `#3456`).
- :pushpin: **migrate** — folded the 8 legacy gated `agent:backlog` items into `agent:review` (clears `legacy_gated_in_backlog`).
<!-- agent:boundary:7c96f9ce -->
```
---
- do [#hap7]
- :pushpin: do [#tsiftmdcrash]
---
<!-- /agent:queue -->
<!--
Please make backlog replacements for all gated review items. We need to complete the plan, not have any more gates after this round. Let me know when I need to install JB plugin updates. Make sure all JB live gate items have adequate logging to prove/disprove the item. Clear the review items. I will test, files bugs, and you will check the logs.
---
`'/clear' is disabled while a task is in progress
---~~~~
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
The last run had a code fence stripped away by agent-doc.
#spec-test-build-install-commit-push
---
What are #next-steps to fix bugs?
-->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#sqedit-race] [operator-verify] #agent-doc-bug Fix the live-IPC-supervisor race on direct queue edits (the "noise heads never auto-deleted - the live IPC supervisor races on direct queue edits" hazard). NOT a single converge-path bug (try_editor_converge is already fail-closed on ack_mismatch); it is the compounding of: (1) multiple live JB plugin consumers re-flushing stale buffers (#8bfz), (2) preflight queue maintenance (#mirrorall mirror + backlog->queue sync + #7r2s pin + live-buffer converge) round-tripping already-malformed entries (double-pins, stray ~~~done / --- runs) and re-mangling them, (3) supervisor idle-watch reading a torn queue. SAFETY-CRITICAL supervisor/commit-path - focused clean cycle with make check + make tmux-ci, NOT a degraded-session drain (same class as the deferred #aqhr/#smconv). Phases: 1 single live plugin owner per doc (absorbs #8bfz), 2 queue-edit lease respected by supervisor idle-watch AND preflight maintenance, 3 idempotent malformed-entry normalization (render/parse fixed point), 4 restart + queue prune-noise cleanup (folds in #prunenoise-live + #qpausemix-verify). Plan: tasks/agent-doc/plan-supervisor-direct-queue-edit-race.md
- [ ] [#prunenoise-live] [operator-verify] Clean the ~20 queue_stale_noise_lines on THIS doc with the new queue prune-noise command (submodule 9c53d423). Deferred from this session: the command only exists in the freshly-built binary, and running a cross-binary queue mutation against the live route-owned supervisor (old binary) risks the IPC/CRDT race that already contaminated this queue. At the supervisor-recycle boundary (shared with #qpausemix-verify): run make install, agent-doc admin recycle, then agent-doc queue prune-noise tasks/agent-doc/agent-doc-bugs2.md and confirm session-check queue_stale_noise_lines drops to 0 while the operator-verify do[#id] heads (#tsiftmdcrash/#hap7/#fbwire) and any drainable heads survive.
- [ ] [#qpausemix-verify] [operator-verify] #agent-doc-bug Verify the #qpausemix session-check fix (submodule 2cf7bcee) live: build+install the new binary (make install: cargo install --path . + cargo build --release + agent-doc lib-install) at a supervisor recycle boundary (deferred from this session to avoid finalize-client-newer-than-supervisor drift), then on a controller-paused queue (e.g. equityfundingsource.md under a qflood pause) confirm session-check stderr now prints the "queue_paused is NOT a contradiction" preamble + recorded reason so an agent no longer stalls deciding operator-set-pause vs transient state. Root cause fixed: session-check printed the bare CONTINUATION_NO_STALL_GUIDANCE constant instead of continuation_guidance(pause_reason); resolution previously only reached preflight JSON.
- [ ] [#hap7] [operator-verify] #agent-doc-bug #qdup Operator-reported (2026-06-19): agent-doc duplicated the operator equityfundingsource.md queue prompt (All upload preview dialogs should be full screen, deploy) on a concurrent edit. Second instance of the queue-prompt duplication family (first: virtuousai.md). Fix per operator spec: constrain the CRDT/semantic merge to the structural part it applies to (per-structural-node merges, not whole-component) so a concurrent operator edit to one queue item cannot duplicate adjacent structure; and when the operator deletes a structure that agent content targets, note it in agent:exchange instead of merging. Merge-core change, the corruption source this session, so NOT safe to rush in the in-session loop; needs a focused cycle. Plan: tasks/agent-doc/plan-scoped-crdt-merge-no-structural-duplication.md
- [ ] [#fbwire] [operator-verify] #agent-doc-bug Phase 2 live wiring of the #fullboundary convergence gate (Phase 1 decision core + playback shipped, submodule 17010e78). Call convergence_gate_decision inside the supervisor idle-queue-watch/drain inter-item dispatch (start/idle_watch.rs to start.rs): gather the four facts (committed via cycle_state, editor converged==HEAD via emit_postcommit_worktree_check, ipc_socket inflight==0, actor idle), defer the next trigger until Dispatch, on ForceDiskFallback do the real --force-disk write + record_force_disk_fallback. Add SimWorld acceptance. Live-supervisor-critical: needs make check + make tmux-ci across two live panes, so NOT in-session-loop-drainable; needs a focused operator-driven cycle. Plan: tasks/agent-doc/plan-fullboundary-convergence-gate.md
- [ ] [#tsiftmdcrash] [operator-verify] Operator reported that a tsift.md agent-doc turn crashed and killed the session process while the tmux pane stayed active. Supervisor pid 1742954 is alive; pane %78 OpenCode TUI shows Thinking normally; tsift-v0.1.log (994KB) shows NO panic/abort/crash signatures — only two intentional supervisor_binary_stale_self_recycled via=execve_preserve_child entries from cargo install + make install (#supkill-bg blue/green drain-and-supersede). Most plausible: operator saw the brief execve-recycle window (current_exe deleted) as a crash. Not reproducible from logs alone. **Operator action: paste the exact crash error text / pane capture from the crash moment, or the specific cycle id, so it can be root-caused.**
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
- [/] [#qpauseux] [operator-verify] [recommended] JB Run Agent Doc surfaces a queue_paused/qflood dispatch block to the operator badly, and can appear not to submit at all. Variants: (1) a deliberate/admin pause renders as a raw red Error: toast instead of an actionable notification; (2) a churn-stop pause (reason: broken editor IPC, pausing to stop churn - the #41a32133 exception that blocks every caller including an explicit operator reopen so the route restarts the stale supervisor first) renders as NOTHING - JB Run Agent Doc just silently stalls with no notification; (3) operator report 2026-06-19: JB Run Agent Doc is not submitting / no visible submission result. Plugin should render a paused-queue block (ui_outcome=blocked_with_exact_unblocker unblocker=resume_or_clear_queue_control) as an info/action notification with a Resume button; for the churn-stop/no-submit variant surface a restart-supervisor / resume action instead of silence. Operator action: install the updated JB plugin, then verify both the paused-queue notification and the churn-stop/no-submit notification render (live gate).
- [/] [#docdriftgrace] [operator-verify] agent-doc thrashes when the session doc changes mid/between turn in ways unrelated to the current prompt: snapshot>visible drift (operator trimmed old committed Re: blocks + harness switch + queue edits) made preflight refuse commit (looks like a manual cleanup with stale snapshot) and forced a manual reset --from-current. Binary should classify drift by component and silently auto-rebase snapshot/CRDT for unrelated changes, surfacing ONLY structural conflicts on the in-flight prompt/response. **Operator action: watch one live re-baseline cycle to verify the silent auto-rebase.** Plan: tasks/agent-doc/plan-graceful-doc-drift-resolution.md
- [/] [#gf5n] #numlistrev #agent-doc-bug Operator-reported: numbered/ordered lists rendered REVERSED (4,3,2,1) on an initial turn, boundary after the block. INVESTIGATED 2026-06-19: NOT reproducible in a clean stream/CRDT finalize — a 1-2-3-4 list renders 1-2-3-4 correctly (verified /tmp repro). It manifested only in the drift-heavy monsterrodholders turn, so it is a SYMPTOM of the CRDT merge-corruption family (#hap7/#qdup/#semmerge: the merge inverting list-item order under a live-buffer race), not an independent ordered-list render bug. Resolves when #hap7 scoped-merge lands. Kept open as a #hap7 verification check: confirm numbered-list order survives a merge-during-finalize cycle.
- [/] [#rt83] #qrepin #agent-doc-bug The active-queue stripped diff perpetually reports a phantom pushpin-add of the operator-verify head #qpauseux every preflight even when the doc and snapshot raw queue bodies are byte-identical, keeping no_changes:false and re-triggering the supervisor idle-watch. Investigated: sync_backlog_into_queue (entry_do_id/do_prompt_id), the #mirrorall mirror, and the #7r2s auto-pin (annotate_manual_queue_additions) all correctly recognize and skip the already-pinned bare-id head, so it is NOT a content mutation. Root is in the preflight stripped-diff/boundary-reposition path: commit repositions the boundary in the snapshot but skips the working-tree reposition while a live IPC listener is active, so the post-commit snapshot queue and the post-maintenance doc queue diverge on the pinned operator-verify head position/strip, surfacing a phantom pin add. Cosmetic (no corruption) but it sustains qchurn. Fix: normalize operator-verify mirror pins symmetrically in the stripped diff, or converge the snapshot boundary reposition with the working tree even under a live listener. Needs instrumented per-phase queue-body tracing.
- [/] [#restartagentmenu] restart-agent Phase 2 (after #agentreloadrestart): add a Restart Agent JB plugin menu command — new RestartAgentAction.kt mirroring RestartSupervisorProcessAction.kt, registered in plugin.xml beside AgentDoc.RestartSupervisorProcess, wired through the TerminalUtil restart path; after Phase 1 re-resolves the harness on restart this brings up a changed agent. Decide alongside-vs-replace the existing Restart Supervisor Process action (do not remove without operator confirmation). Bump pluginVersion (gradle.properties, currently 0.2.170), build unsigned + signed zips. LOGGING (operator-install live-gate prove/disprove): log the action invocation + resolved harness. Plan: tasks/agent-doc/plan-restart-agent.md
- [/] [#agentreloadrestart] restart-agent Phase 1 (foundational): make the supervisor restart path re-resolve the harness from current frontmatter so changing agent (claude->opencode) takes effect on restart, and auto-restart the agent at the next idle/turn boundary when the resolved harness differs from the running one. Today the restart loop reuses the harness resolved once at start (SharedSupervisorState.harness, start.rs:171) and never re-reads fm.agent. Re-resolve via HarnessConfig::from_context (harness.rs:116) before respawn, rebuild child_factory/harness_binary, then gate the boundary auto-restart like the supselfheal auto-recycle (prompt_visible and not turn_active, debounced, never mid-turn) behind a new agent_doc_agent_change_restart knob (default ON). LOGGING (live-gate prove/disprove): emit harness_change_detected old/new, agent_restart_boundary_gate reason, agent_restart_performed to ops.log. SimWorld tests + SPEC + VERSIONS. Live-supervisor-critical: focused clean cycle with make check + make tmux-ci. Plan: tasks/agent-doc/plan-restart-agent.md
- [/] [#cyh0] #fcczero #agent-doc-bug [operator-verify] JB File Cache Conflict still occurring; target is ABSOLUTE zero session-doc disk writes except genuine emergencies (everything via editor IPC). Audit (#smconv cycle) confirms the documented hot-path sites are already listener-guarded: post-commit worktree reconcile skips disk with a listener (#pcwcdiskfree), boundary reposition queues via file IPC and only disk-writes when IPC is Unavailable, queue/pending/status maintenance route through converge_or_disk_write (#fccqueue/#fccaudit); remaining disk writes are recovery/scaffold/migration emergencies by design. So the remaining FCC has an UNIDENTIFIED trigger. **Operator action: capture which exact action raises the dialog (the agent-doc command + ops.log line at that moment, e.g. write_authority action=... transport=disk) so the specific unguarded path can be found.** Likely candidates to check: the ack-content sidecar write, the VCS-refresh/save-document signal, or an editor-IPC convergence that the plugin still surfaces as a conflict. Note: this session most FCC came from manual `git checkout HEAD` recovery (a disk write), which #smconv removes the need for by merging drift instead of dropping responses.
- [/] [#8bfz] #fcconeowner #agent-doc-bug elect a SINGLE live IntelliJ plugin consumer per document so concurrent windows/instances cannot race patch application + saveDocument into File Cache Conflicts. Today each IntelliJ plugin process generates its own consumer id jetbrains-<pid>-<uuid> (TypingTracker.kt) and every live instance that has the project open watches .agent-doc/patches/ and consumes patches independently = N live consumers for one doc when N windows are open. #fccreap (0.34.8) reaps DEAD-pid consumer files but does not dedupe LIVE ones. Add single-owner election: the plugin acquires a per-document owner lease (lock file under .agent-doc/, or via the sessions.json registry) and non-owner instances defer patch application + saveDocument for that doc, so exactly one live consumer applies/saves. Owner lease is short-TTL/self-healing so closing the owner window hands ownership to another live instance. Plugin-side change (PatchWatcher + a lease) + binary lease primitive; operator-verify (needs multi-window live test).
- [/] [#1vhn] #routedeadshell agent-doc-bug: JB Run Agent Doc dispatched the agent-doc FILE trigger into a bare zsh % shell on recruit/virtuousai.md (harness had crashed/exited to a shell), so it did not submit (screenshot img_48.png, ~14:03, during a /model opus switch). REPRO (operator 2026-06-18): Run Agent Doc on virtuousai.md -> supervisor + tmux pane CRASHED 2 times -> 3rd attempt stalled -> operator manually ran agent-doc. So the supervisor/pane crash on this dispatch path is REPRODUCIBLE, not a one-off. Route must positively identify a HARNESS dispatch-ready prompt and treat a bare shell prompt as NOT-ready -> fail closed with claim/restart guidance (or auto-restart) instead of typing into the shell; re-verify the harness prompt immediately before send to close the crash-mid-dispatch race; add a dead-harness route diagnostic + SimWorld coverage. Separately root-cause WHY the supervisor+pane crash repeats (the /model opus switch/restart is the lead). **Operator action: if it recurs, capture .agent-doc/logs/supervisor-stderr.log + the pane scrollback at the crash moment.** Plan: tasks/agent-doc/plan-route-dispatch-into-crashed-harness.md
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
