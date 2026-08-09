---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
resume: a3c686da-ff02-4516-9fcf-f66e27413bef
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
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find all contributing factors to fix.
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#simworld': Strengthen Simworld tests.
  '#spec-test-build-install-commit-push': red-green-refactor. update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': red-green-refactor. update spec + tests. commit + push
queue: go
---

## Status

<!-- agent:status patch=replace -->

completed
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260809-011731.md`*

Compacted content:
- Archived 5 response topic(s): do [#hvw0publishgrep]; do [#dedupepresettle]; haiven-dev.md wedge and the manifest diagnosis; 2 more
- Prior summary/context: prior compacted content: Archived 4 response topic(s): do [#ptsubmitmetric]; do [#claudesplitsubmit]; do [#compactstr...

### Re: file all 3 diagnosed defects into the backlog — opus-5

Filed, in the requested order, with `#panewindowdrift` at the top of both `agent:backlog` and the go queue.

| id | defect | status |
|---|---|---|
| `#panewindowdrift` | editor auto-focus mirrors onto an invisible stashed pane | starting now |
| `#strandeddraftresubmit` | stranded composer draft not resubmitted before the ready-wait | queued |
| `#preflightprojpass` | preflight never opens the current-document projection pass | queued, blocker recorded |

Each item carries the measured evidence rather than a restatement of the symptom, so none of it has to be re-derived:

- **`#panewindowdrift`** — doctor/registry said `sdk.md -> pane=%926 window=@894` (visible `0:0 agent-doc`) while tmux had `%926` live in `@904` (`0:1 stash`). The layout projection still counted it as the co-visible column-2 pane and applied focus to it, then bounced to the genuinely-visible pane ~5s later. `session doctor` reports `no issues detected` for this exact state because it never compares the recorded `window=` against the pane's live window.
- **`#strandeddraftresubmit`** — zero `tmux_input_event` to `%926` between 02:27:00 and 02:27:29, so the draft was already stranded; route waited 26.3s on `route_wait_for_ready_override`, then injected a *second* trigger onto it. The `pass_through_draft_resubmit` repair exists but is wired to the post-dispatch path. Also records that `dispatch_start_unproven` is being derived from a null `turn_admission_projection`.
- **`#preflightprojpass`** — ~49 redundant full-document CRDT resolutions per preflight (~186ms each). The item explicitly says **do not write a new cache** — `current_document_projection.rs` already is the right one, revision-keyed — and records the blocker: only `commit-io:891` invalidates, while preflight's seven mutating phases invalidate zero times, so opening the pass before making those mutators self-invalidate would serve stale text.

Starting on `#panewindowdrift`.

### Re: start with #1 — opus-5

`#panewindowdrift` diagnosis half is landed, tested, and installed (`6f1af3f6b`, pushed).

**What was wrong:** `collect_doctor_issues` compared `actor.window_id != entry.window` — record against record. Both are captured at bind time and neither is re-derived, so a pane later moved into `stash` keeps naming the visible window in *both* records and the drift is invisible. That is why doctor said `no issues detected` while the projection focused a stashed pane.

**Fix:** `pane_window_binding_drifted()` as pure policy in `agent-doc-controller/src/pane_layout.rs`, plus a live-window observation in `build_context`. Strict tightening, mirroring the cross-repo owner guard — unknown or empty on either side is never drift, so a transient tmux read cannot cause a false repair.

**Verified against the live drift, not a fixture.** Before: `doctor: no issues detected`. After:

```
- actor window @894 for pane %926 is stale — the pane now lives in @904
- registry window @894 for pane %926 is stale — the pane now lives in @904
```

`agent-doc-bugs2.md` (pane `%25`, recorded `@894`, live `@894`) still reports `no issues detected`, so it is not a blanket false positive. `make check`: **8753 passed, 0 failed**. `make install` done, controllers recycled.

**Not done, and the item stays open for it:** doctor now *reports* the drift but nothing *prevents* it — the survey still admits a stashed pane as a visible column, so auto-focus is still wrong until that lands. The seam is `apply_pane_layout_focus_effect` and its call sites already hold a tmux handle, so the guard is cheap; what is missing is a decision about which window the projection considers "visible". Guessing that reference would break legitimate multi-window layouts, so I narrowed the backlog item to exactly that decision rather than pick one from log-reading.

**Unblock now:** run Sync Tmux Layout (or `agent-doc sync`) — the doctor-backed repair restores `0:agent-doc` / `1:stash` and focus follows again. That is why the manual action has always worked and the automatic path has not.

### Re: do [#strandeddraftresubmit] — opus-5

Fixed and installed (`cc2badb87`, pushed; superproject pointer `06aabb509d`). Binary is 0.35.185, controllers recycled.

**The bug had two halves and both are closed.**

**1. Route injected on top of a draft it never looked at.** `dispatch_routed_reopen_with_mode` went straight to `send_command_once_checked` / `send_command_checked`. `repair_pass_through_stranded_draft` runs *after* that send, so it can only ever fix a draft the same call created — a draft stranded before the ready-wait was invisible to it. That is exactly the trace: nothing sent to `%926` for 29s, 26.3s on `route_starting_actor_ready`, then 74 bytes appended to whatever was already sitting there.

The check now runs before either submit policy. `classify_pre_dispatch_stranded_draft_action` is pure policy over one capture and it is a strict tightening — only an idle, dispatch-ready composer still holding *this* trigger diverts; capture failure, no draft, and a busy pane all take the old path byte-for-byte. On a divert it presses one bare submit key and awaits turn admission: projected → `RoutedDispatchStartProof::StrandedDraftSubmitted` (`dispatch_start` scope) and **no injection**; anything else falls through, so the worst case is the behavior you had.

The "still unsubmitted" test deliberately does **not** use `pane_composer_has_pending_trigger`. That substring-matches the whole capture and is only sound on the brand-new fresh pane `#jbtsiftnosub2` wrote it for; this path runs on long-lived panes whose scrollback holds every previously submitted trigger, so it would read consumed history as a strand (`#autotriggerscrollbackecho`). It uses `ready_prompt_candidate_at_cursor` + `route_trigger_visible_in_current_draft` instead.

**2. `dispatch_start_unproven` derived from a null projection.** This was the more damaging half, because it is what *generated* second triggers. A null `turn_admission_projection` was read as "not submitted", which called `observe_effect_failed()`, which bumped `effect_retry_epoch` and rearmed the drain — a blind retry that writes a second trigger over the first. And a legitimately no-op turn never projects an admission at all, so that reading is wrong on its own terms.

`classify_queue_dispatch_start_observation` now returns three outcomes, not two. It takes accept-only pane-busy proof (the precedent the auto-trigger path already had under `#autotriggeradmissionstall`) and requires a *visibly pending payload* before it will claim `Unproven`. Everything else is `Unobservable`, which releases the in-flight flag through the new `observe_effect_unobservable()` **without** bumping the retry epoch — the un-consumed document edge still lets the next tick re-observe and reach the existing pending-payload `ResubmitEnter` repair. "Could not observe" no longer rearms an injection.

8 new tests. All four behavioral guards are **mutation-proven**: reverting the pre-dispatch classifier to always-dispatch-fresh reddens 3, and making `observe_effect_unobservable` rearm like a failure reddens the 4th. `make check`: **8762 passed, 0 failed**. CI for `cc2badb87` is queued (previous run green).

### Re: 3 tmux panes and auto-sync going from acceptable to extremely laggy — opus-5

Measured it. **It is `#preflightprojpass`, and the pane count is not the cause.**

Between 03:20 and 03:28Z this document logged **170 full-document CRDT resolutions** — up to **14 in a single second** — and every one returned the identical `len=22846 hash=83f91d63`. Nothing changed between them.

What I ruled out, so you don't have to take that on faith:

| suspect | measurement | verdict |
|---|---|---|
| pane-layout survey | `pane_layout_survey_timing elapsed_ms=5-29` across the whole window | not the cost — 3 panes are cheap |
| controller RPC health | `admin detect` 0.05s, `session status` 0.60s | controller is serving fine |
| identical CRDT resolutions | 170 in 8 min, 14/sec peak, one hash | **this is it** |

The important new fact: this is **not** preflight-scoped. The hot consumer is the realtime relay — `realtime_doc_resolve authority=editor_buffer reason=crdt_relay_current source=crdt_relay` — and `#preflightprojpass`'s recorded blocker does *not* gate it. That blocker is specifically that preflight's seven mutating phases never self-invalidate; the relay consumer has no such precondition. So the operator-visible latency win is available *first*, ahead of the preflight half. I rewrote the item to say that explicitly and to carry these numbers, so the next drain starts from the cheap half instead of re-deriving the blocker.

I did not expand into it this turn — it is the next queue head and the loop takes it immediately.

One thing I could not measure from here: the editor-driven sync path itself. `agent-doc sync` refuses without explicit `--col` (it requires the editor to publish its visible projection), so the end-to-end editor latency is only observable from a live IDE. The resolution count above is the underlying cost either way.

On the 3 panes: window `0:0 agent-doc` holds `%25` (agent-loop root), `%926` (`src/haiven-dev`), `%13` (agent-loop root), and `0:1 stash` holds 5. `%926` is the same pane from the `#strandeddraftresubmit` trace. If a column there is one you cannot see in the editor, that is the unfixed half of `#panewindowdrift` — `session doctor` now reports the drift but nothing prevents it, and `agent-doc sync` (Sync Tmux Layout) is still the manual repair.

### Re: are you tracking the unsubmitted prompt storm? — opus-5

No, I was not — it is a different defect from the one I closed above, and my write-up for that one contained a claim that this disproves. Fixed and installed (`61d47dd27`, pushed; superproject pointer `fe692bf164`).

**One 22-byte pane capture explains the whole storm.** Preserved verbatim, because a rendered-frame check written against a paraphrase would not have caught it:

```
ESC[38;5;246m ❯ NBSP ESC[39m      ← the entire capture, 22 bytes
payload_already_pending=false  dispatch_ready=true  action=dispatch_fresh
```

A bare prompt glyph and nothing else — pane `%20` sampled mid-render while layout churn moved it between windows. `capture_pane` returned `Ok`, so the drain treated it as authoritative and concluded three wrong things at once: composer empty, prompt dispatch-ready, and by omission that no turn was running. It injected. 31s later it sampled the same unrenderable frame and did it again. Eleven times, `03:38:07`→`03:43:13`, into a pane 19 minutes into an active turn.

**A successful capture is not an informative one.** `#idlerevisionreactive` applies to a pane exactly as it does to a controller probe: "I looked and the composer is empty" and "the frame I got cannot contain a composer" are different facts, and the drain had no way to say the second one. Its `DeferUnobservable` branch was reachable only when the capture *failed*.

Three defects, in the order they matter:

| # | fix | note |
|---|---|---|
| A | `pane_frame_answers_composer_state` — a frame whose only non-empty line IS the prompt candidate answers nothing | the proven cause |
| B | observation carries `pane_busy`; the drain had **no busy input at all** | latent, not the proven cause here — the 22-byte frames hid the busy cue anyway |
| C | `IDLE_QUEUE_UNPROVEN_INJECTION_BUDGET = 2` per (head, payload) | backstop: no payload can be written unbounded |

A's rejection runs **before** `payload_already_pending` and `dispatch_ready` are consulted — order is load-bearing, since both are computed from that same meaningless capture. The predicate is deliberately not keyed on harness footer text, which varies per harness and permission mode and is what the pane-draft-unblocker work already had to repair twice.

**Correcting my previous entry.** It said an unconsumed edge would "reach the pending-payload resubmit instead of re-injecting." That was wrong, and A is why: the next tick re-read the same unrendered frame and re-ran `DispatchFresh`. That commit fixed the route path and the post-send *proof*; it left the drain's pre-send *precondition* trusting an unreadable frame. Same failure family, opposite end of the same dispatch.

6 new tests, all four guards mutation-proven — including one asserting that `pane_dispatch_ready_at_cursor` genuinely does return `true` on those 22 bytes, so the frame gate is the only thing that can reject them. `make check`: **8768 passed, 0 failed**.

**Not yet proven live, and I am not claiming it is.** The binary is installed and every controller recycled, but zero drain decisions have run since the install, and the long-lived supervisor for that document (PID 3854486, started Aug 8) still holds the pre-fix image until its next idle-boundary reexec. Confirmation is a `frame_rendered=` field appearing in an `idle_queue_payload_observation` line — automatically checkable, so it is a backlog item rather than an operator-gated review item.

One correction on my own diagnostics: I initially read a controller's `--handoff-state preparing` argv as a live wedge. That is launch argv, not live state — RPCs answered in 50ms. Disregard it.

Thanks for the screenshot. The 31s cadence and the stacked-identical-payload shape are what made this findable at all.

### Re: do [#unrenderedframestormlive] — opus-5

> **Queue prompt:**
>
> do [#unrenderedframestormlive]


Both automatic proofs pass. `#unrenderedframestorm` (61d47dd27) is live in production and its guard has fired.

**Proof 1 — the new observation shape is running.** `frame_rendered=` now appears in `idle_queue_payload_observation`, first at `2026-08-09T03:59:07Z`, 13 seconds after the 03:58:54Z install:

```
[2026-08-09T03:59:07Z] idle_queue_payload_observation file=tasks/agent-doc/agent-doc-bugs2.md
  harness=claude pane=%25 ... dispatch_ready=false frame_rendered=true pane_busy=true
  action=defer_pane_busy
```

Four lines carry it: three in `agent-loop/.agent-doc/logs/ops.log` (03:59:07Z, 04:00:08Z, 04:00:59Z) and one in `src/haiven-dev/.agent-doc/logs/ops.log` for `tasks/sdk.md` at 04:06:47Z. Every `idle_queue_payload_observation` before the install lacks the field, so the split is exactly at the install boundary — this is the new image, not a coincidence of formatting.

**Proof 2 — a guard actually fired, and it is a new one.** Three `idle_queue_dispatch_deferred` lines, each tagged with the fix id:

```
[2026-08-09T03:59:07Z] idle_queue_dispatch_deferred file=tasks/agent-doc/agent-doc-bugs2.md
  harness=claude reason=defer_pane_busy payload_kind=trigger unproven_injections=0
  ... (#unrenderedframestorm)
```

`git show 61d47dd27` confirms `defer_pane_busy` is not a pre-existing reason being re-observed — the commit introduces the `DeferUnrenderedFrame` / `DeferPaneBusy` / `DeferInjectionBudgetSpent` variants together. So these deferrals are the new code path, not old behavior with a new label.

**The decisions are also correct, not merely logged.** Post-install idle-queue outcomes across every project: `defer=3, dispatch_fresh=0` in agent-loop, `defer=0, dispatch_fresh=1` in haiven-dev. The three deferrals all read `frame_rendered=true pane_busy=true` — a *rendered* frame that truthfully showed a busy pane, so the drain correctly declined. The single dispatch read `frame_rendered=true pane_busy=false`. Both directions are exercised, and no injection storm has recurred.

**The supervisor concern in the item is resolved, but a different one replaced it.** PID 3854486 (`tasks/haiven-dev.md`, started Sat 08-08 03:42:57) was flagged as holding the pre-fix image. It does not: `readlink /proc/3854486/exe` returns the path with no `(deleted)` marker, and the resolved inode matches the installed binary. Since `cargo install` replaces by rename, a process still mapping the old image would necessarily show `(deleted)`. It re-exec'd at an idle boundary as designed — a re-exec preserves PID and start time, which is why the old start timestamp misled the original diagnosis.

Surveying all eight live supervisors that way turned up one that genuinely has not: **PID 4069526** (`src/boost-client/tasks/monsterrodholders.md`, started Wed 08-05 19:12:50) still maps `(deleted)`, while logging normally at 03:59:58Z — a minute *after* the install. It is the only one launched without `--route-owned`, which points at the idle-boundary re-exec being gated on the route-owned lane. That is the separate stale-supervisor-reexec defect the item said to file rather than fold in; it is now `#stalesupervisorreexec` at the top of the backlog with the PID, start time, and the hypothesis to test first.

No code changed this cycle — this was the verification the item asked for.

### Re: do [#stalesupervisorreexec] — opus-5

Fixed, shipped as 0.35.186 (`d7e322cd1`, tag pushed). The hypothesis I filed last cycle was wrong, and the real cause is worse — and better — than a policy gap.

**The `--route-owned` hypothesis was wrong.** `spawn_idle_queue_watch_thread` is called unconditionally from `run_with_reap_policy_and_resume`; route ownership gates nothing. And the staleness probe itself is fine: `resolve_agent_doc_binary_from_env` explicitly rejects a `(deleted)` `current_exe` and falls through to PATH, so `refresh_binary_stale()` compares the running inode `95833830` against the installed `95833211` and correctly answers *stale*. The supervisor knew.

**The defect is control flow, not policy.** The idle-queue watch does two unrelated jobs per tick — drain the go-mode queue, and decide whether a stale supervisor should hot-reload — and the recycle decision is sequenced *after* the drain. The drain has several legitimate early exits, and every one of them `continue`d the whole **tick**:

| site | early exit | when it fires |
|---|---|---|
| `idle_watch.rs` `QueueHeadObservation::AuthorityUnavailable` | `continue` | controller projection unavailable |
| context-clear in flight (3 arms) | `continue` | a `/clear` is settling |
| clear-cooldown resume | `continue` | cooldown just expired |

A supervisor parked in any of those states never reached `let supervisor_stale = supervisor_stale_fast;` — so it never recycled, never published `binary_stale`, and never wrote the `⚠ STALE SUPERVISOR` pane marker. **The failure suppressed its own diagnostic**, which is why it survived four days unnoticed.

That matches PID 4069526 exactly: `crdt_projection_observe_deferred ... reason=controller_projection_unavailable` in its ops log, 227 `stale_supervisor_cp_recycle_requested ... request_status=requested` lines, and **zero** `supervisor_binary_stale_*` lines across 198,660 entries. The CP wrote the recycle marker 227 times and nothing was ever on the other end to read it.

**The fix.** The drain now runs inside a `'drain:` labeled block, so an early exit leaves the *drain*, not the tick, and the recycle evaluation always runs. The facts the drain would have produced are hoisted with conservative "nothing observed" defaults — which is precisely what the policy already wants: `stale_recycle_safe_checkpoint` (stale binary + no IPC handler in flight) licenses a recycle without needing a turn boundary or a pending head, so a starved supervisor recycles on the first tick after the fix lands. A new `supervisor_stale_recycle_drain_fallthrough` ops-log receipt makes the guard provable from a live log instead of only from source.

A detail worth keeping: **`rustc` rejects a bare `continue` inside a labeled block (E0695)**, so the literal regression can no longer compile. I confirmed that by reintroducing it — the build fails rather than the test. The test file therefore covers the half the compiler accepts (a `continue 'label` aimed past the block) and pins the drain-before-recycle *ordering* the fix depends on, so a future reorder can't make the guard pass vacuously.

**Coverage, verified red-then-green.** SimWorld had faithfully reproduced the bug — its `supervisor_idle_queue_tick` carried the same `return Ok(())` at the clear-cooldown-resume boundary, ahead of its own recycle section. Repairing the model gave a real behavioral test: `stale_supervisor_recycles_even_when_the_drain_bails_early` fails on the pre-fix model with `supervisor_recycles: left: 0, right: 1` and passes after. `make check`: **8772 passed, 0 failed** (clippy + nextest + TLA+), twice — once before the version bump and once after. Installed 0.35.186, controllers recycled, CI run 31295857171 in progress at push time.

**One supervisor cannot be rescued by this fix.** PID 4069526 is stranded precisely because it does not run the code that fixes it — the fix is preventive, not retroactive. Freeing it means interrupting a live Claude child (PID 2653032), which is an operator call rather than an agent-doable step, so it is queued as `#stalesupervisorreexecdrain` with the command and the verification. Every other live supervisor already maps the current inode.

### Re: do [#panewindowdrift] — opus-5

> **Queue prompt:**
>
> do [#panewindowdrift]


Fixed and shipped in **0.35.187** (`d1faeb55a`, tag pushed).

**The blocker, resolved.** The reference window is the projection's *own* target window — `invocation.window` when it is already a window id, otherwise the `agent-doc`-named window in the project's configured tmux session. That is not a new heuristic: it is exactly what `tmux_layout_sync_state_for_invocation_with_effect_assignment` already resolves before it lists the layout's panes (and it already refuses any target window not named `agent-doc`). Because it is per-invocation, a legitimate multi-window layout keeps its own reference — nothing is inferred from the active pane, the first column, or where the majority of panes happen to be. Only a window **id** is ever used as the reference, since the live side of the comparison is `#{window_id}`; a window *name* falls through to the by-name lookup rather than being compared against an id.

**The guard.** `apply_pane_layout_focus_effect` now takes a `PaneLayoutFocusCoVisibility { layout_window, observe_pane_window }` and, after resolving the focus pane but *before* taking the worker mutex (it is a tmux read — it must not run under the lock), compares the layout window against the pane's live window through the landed pure predicate `pane_window_binding_drifted`. On drift it returns `focus_pane_not_co_visible:<file>:<pane>:live_window=<w>:layout_window=<w>` with `applied: false`.

That receipt is load-bearing without further plumbing: convergence is already `report.synced && (!focus_receipt.required || focus_receipt.applied)`, so a refusal cannot report converged. In the reusable-structure fast path the refusal drops through to the structural reconcile — which is the thing that can actually surface the pane — and the retry focuses it once it is back in the layout window. The refusal is a *deferral*, not a dead end.

**Stop counting it toward `observation=synced`.** The survey had the same blind spot on the observation side: `expected_panes` and `expected_focus_pane` were resolved from the effect/reuse assignment, which still names the stashed pane. They now resolve from assignments filtered to the target window's own ordered pane list (`panes`), which the survey already has in hand — no extra tmux call. A pane outside that list is not a visible column, so it cannot satisfy a layout slot or stand in as the focus target; with focus requested the report goes `focus_pane_not_found` instead of quietly agreeing.

**Strict tightening throughout**, mirroring the predicate and the cross-repo owner guard: an unresolved window on either side is never drift. No configured session, no `agent-doc` window, or a tmux that cannot answer → focus proceeds as before. A transient read cannot turn into a refused focus.

**Coverage.** Four tests in `agent-doc-controller-io`: the stashed-pane refusal (asserting `select-pane` is never reached and the exact receipt reason), the four unknown-window shapes, the id-vs-name reference resolution, and the survey-side slot/focus rejection. Plus the SimWorld `pane_layout_stashed_column_model` for the stashed-pane column: a two-column layout whose left pane is moved to `@904` while both records still say `@894`, with `selected_pane_is_co_visible` as the operator-visible invariant. It carries its own record-only mutation (`record_only_guard`) that reproduces the pre-fix behavior — records agree, focus is applied, the invariant goes red — so the passing tests demonstrably depend on the live comparison rather than passing vacuously.

`make check` green. Built, installed (0.35.187 live), pushed, tagged, `admin recycle` marked. CI for the tag is queued at check time — not waited on.

One follow-up, and it is genuinely operator-gated: the live eyeball. Everything agent-doable is done; what remains is watching a real editor/pane prove that stashing a column no longer steals focus. Queued as `[operator-verify]` so go-mode defers it.
<!-- agent:boundary:8c608826 -->
<!-- /agent:exchange -->
## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" go -->
- ~~do [#panewindowdrift]~~
- do [#preflightprojpass]
- do [#loopcontractseal]
- do [#respondbaselineflag]
- do [#hookhashfalsepositive]
<!-- /agent:queue -->

## Notes

<!-- agent:notes -->
<!-- /agent:notes -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#operatorverifylive] [operator-verify] #panewindowdriftverify Live-pane proof for #panewindowdrift (0.35.187): with a two-column agent-doc layout, move the focused document's pane into the stash window, then trigger an editor tab/focus change for that document. Expect NO select-pane onto the stashed pane (ops.log shows pane_layout_projection ... focus=focus_pane_not_co_visible:...), the structural reconcile surfaces the pane, and the retry focuses it in the agent-doc window. Unblocks on a human watching a real IDE + tmux session.
- [ ] [#hookhashfalsepositive] agent-doc's coined-id-pre-tool-use hook rejects C preprocessor directives as untracked backlog ids. Writing a file containing a line starting with a hash — pragma once, include <string> — is blocked with "would write coined id(s) #include, #pragma ... not tracked in agent:backlog". The same class of false positive already applies to CSS ids, shell comments, and agent-doc's own anchor tags quoted from SKILL.md. It is not a hard block (a bash heredoc bypasses the PreToolUse hook entirely, which is itself the interesting part — the guard is trivially avoidable while being obstructive to correct writes) but it forced a workaround into emitted C++: src/sdk-spike/ts/src/wire/code.ts assembles the directives via String.fromCharCode(35) with a comment explaining why. The guard should recognise language syntax — preprocessor directives, CSS selectors, shell/YAML comments, markdown headings — or scope itself to prose contexts. Fix belongs in agent-loop src/agent-doc, not here; remove the workaround in code.ts once it lands.
- [ ] [#preflightprojpass] #agent-doc-bug Preflight never opens the current-document projection pass, so one preflight makes ~49 redundant full-document CRDT resolutions (98 controller_crdt_current_text / crdt_current_text ops-log lines for a 13.8KB doc) at ~186ms each -- the entire 15-18s preflight wall time, and the cost auto-sync inherits. Measured: the process sits in unix_stream_read_generic for essentially the whole run; session-check alone accounts for 11 logical reads. NOT git (46 subprocesses = 0.05s), NOT tmux (24 = 0.03s), NOT controller serialization (admin detect answers in 0.06s even mid-preflight), NOT machine load (reaping 866 leaked controllers took load 18.59 to 2.08 and preflight stayed 20.84s). Do NOT write a new cache: agent-doc-document-realtime-io/src/current_document_projection.rs already is the right one, keyed by the live Yrs state vector (or disk hash when detached) and read via try_resolve_current_document_with_source then resolve_projection; its doc comment names the trap ('memoizing only by path can hide operator edits made during the pass'). commit-io, closeout-runtime-io and compact-io all open the pass and passes nest; agent-doc-preflight-command-io/src/run.rs run_with_options does not. BLOCKER (PREFLIGHT HALF ONLY) -- wrapping preflight is unsafe today: only agent-doc-commit-io/src/lib.rs:891 calls invalidate_current_document_projection (session-check calls its alias at 10+ sites), while preflight's own mutating phases call it ZERO times because with no pass open none is needed -- exact-replay converge (run.rs:300), auto-repair base index (354), repair (479), run_pending_maintenance (537), commit (~709), run_queue_maintenance (858), baseline realign (936). Safe order for that half: make each preflight mutator SELF-invalidate (the discipline session-check already follows), THEN open the pass in run_with_options, THEN a regression test that a post-mutation read observes the new text. RE-MEASURED 2026-08-09 03:20-03:28Z, operator reported auto-sync going from acceptable latency to extremely laggy: 170 resolutions on agent-doc-bugs2.md (22.8KB) in 8 minutes, up to 14 in a SINGLE second, and EVERY one returned the identical len=22846 hash=83f91d63 -- the redundancy is NOT preflight-scoped. The hot consumer is the realtime relay / auto-sync path (realtime_doc_resolve authority=editor_buffer reason=crdt_relay_current source=crdt_relay), and the preflight blocker above does NOT gate it: no preflight mutator has to self-invalidate first. DO THE RELAY CONSUMER FIRST for the operator-visible latency win, then the preflight half behind its stated safe order. Ruled out as causes of that lag: pane-layout survey (pane_layout_survey_timing elapsed_ms=5-29 across the whole window, so pane count is not the cost) and controller RPC health (admin detect 0.05s, session status 0.60s).
- [ ] [#loopcontractseal] #agent-doc-bug SETTLE THEN FIX: the haiven-dev session stopped mid-drain with queue_continuation_required=true and 17 heads, claiming '/loop re-entry can't seal a preflight contract'. That claim is UNVERIFIED — I could not confirm it from another session. .claude/settings.json wires UserPromptSubmit -> `agent-doc hook preflight-user-prompt-submit`, which seals the contract for every submitted prompt; the open question is whether a ScheduleWakeup-fired /loop re-entry counts as a UserPromptSubmit event in Claude Code. Experiment: start a /loop over an active queue, then grep .agent-doc/logs/ops.log for a preflight event with no operator-typed trigger. If the contract IS sealed, the stop was an unjustified stall and SKILL.md's Claude Code auto-loop section needs an explicit 'a /loop re-entry seals its own contract; do not stop for this' line. If it is NOT sealed, the Claude Code auto-loop as documented cannot work and Claude Code needs the same in-turn self-drive OpenCode already has (#ocdrainstall Part A: preflight -> respond -> write --commit in the SAME turn until queue_continuation_required==false). Either way the current instruction surface lets an agent hand 17 drainable heads back to the operator, which #drain-no-defer forbids.
- [ ] [#respondbaselineflag] #agent-doc-bug SKILL.md's MANDATORY persist step documents `agent-doc respond <FILE> --baseline-file <preflight.baseline_file> --stream --origin skill`, but neither `respond` nor `write` accepts `--baseline-file` in 0.35.184 (`agent-doc respond --help` has no baseline/snapshot flag). An agent following the hot-path skill verbatim gets a clap parse error on the one step the skill calls mandatory, and the documented 'use preflight's baseline, do NOT save your own' contract has no way to be honoured. Decide one way: either add `--baseline-file` to respond/write and thread it into the diff baseline, or remove it from SKILL.md + every generated harness surface (.claude/.codex/.opencode/.cursor) and the runbooks. Reported by the operator 2026-08-08 while diagnosing the haiven-dev wedge.
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
- [/] [#stalesupervisorreexecdrain] [operator-verify] One supervisor is still stranded on the pre-fix image and cannot self-rescue: PID 4069526 (`agent-doc start --resume 019fd336-7ce8-7ed1-8141-d77021a00085 src/boost-client/tasks/monsterrodholders.md`, started 2026-08-05 19:12:50 EDT) maps a DELETED binary and, by the very defect #stalereexecstarve fixes, can never reach the recycle decision to adopt 0.35.186. The fix is preventive, not retroactive -- it lives in code that process does not run. Recovery requires interrupting a live Claude child (PID 2653032), which is an operator call, not an agent-doable step: either let it retire naturally when that session ends, or `kill 4069526` and re-route the document. Verify afterwards with `readlink /proc/<newpid>/exe` showing no `(deleted)` suffix. Every other live supervisor (336290, 523099, 577794, 837794, 3268523, 3854486, 3956615) already maps the current inode.
- [/] [#stickymdpaneverify] [operator-verify] #stickymdpane live proof: restart a JetBrains IDE — plugin 0.2.350 is ALREADY INSTALLED in all four IDE configs, so only the restart is missing, and that one restart also satisfies the install precondition for #operatorverifyrestart, #rebootselfhealverify and #falsetyping-guard — then open a two-column split with an agent-doc document in each and switch ONE column to a source file. Expect tmux to keep both panes with that column still showing its last document, instead of collapsing to one pane. Then close that split entirely and confirm tmux DOES collapse — the fallback must be memory, not invention. Unit tests pin the pure resolution but cannot prove the live IDE wiring.
- [/] [#operatorverifyrestart] [operator-verify] Restart a running JetBrains IDE (installed plugin is 0.2.350; this item originally named 0.2.340, long superseded — the single restart in #stickymdpaneverify covers it), then move from fpe-service-topology.md in a one-pane surface to agent-doc-bugs2.md in its two-column surface and confirm the first selection restores two tmux panes without a second navigation.
- [/] [#rebootselfhealverify] [operator-verify] Live proof for #rebootselfheal after restarting the IDE (installed plugin is 0.2.350; this item originally named 0.2.339 — reload-lib swaps the cdylib but NOT the Kotlin package, so a restart is still required, and the restart in #stickymdpaneverify covers it). Reboot the host (or stop the controller and leave a stale .agent-doc/controller.sock behind), then run Sync Tmux Layout and Run Agent Doc from the IDE and confirm both succeed without the `pane-layout state projection publish failed ... Connection refused` notification and without a human deleting the socket. Then confirm the passive lane is still free: click between editor tabs with NO controller running and verify no controller is launched (editors/SPEC.md requires the surface-observation lane to never launch one) - grep ops.log for a controller bootstrap around the tab clicks. Both reboot states are already proven end-to-end against the installed cdylib; only the in-IDE path needs a human eyeball.
- [/] [#hvw0] [operator-verify] Rotate the CARGO_REGISTRY_TOKEN secret on github.com/btakita/skill-harness — the current one returns 403 authentication failed, so CI cannot publish to crates.io (v0.1.3 was published manually from the passage token btak/CARGO_REGISTRY_TOKEN). Needs a repo-secret write, which is operator-gated. CORRECTION 2026-08-08: the claim that every tagged release now FAILS loudly at the publish step (intended behavior after 32836ad) is UNTESTED — no tag has run since. The newest Release run is v0.1.3 from 2026-07-18 and its `Publish to crates.io` step reports success, which is exactly the false green. #hvw0publishproof carries the static check.
- [/] [#c8tb] [operator-verify] Live #jbroutebusystall proof (0b49105e1): with the supervisor socket down (route logs `route_dispatch_only_authoritative_degraded_direct_pane supervisor_health=no_socket`) and the pane mid-turn, click JB Run Agent Doc. Expect `route_start` -> `route_success` in the attempt ledger to drop from ~17s to under ~2s, with `route_existing_pane_busy_active_turn_skip_wait` in ops.log and the SAME outcome as before (`route_existing_pane_already_running`, pane focused, no duplicate injection). Unit tests pin the cue gating but cannot prove the live latency win.
- [/] [#operatorverifylive] [operator-verify] #relaylockpoison live proof: with the JB plugin loaded, force a replica panic (the crash-2026-07-09 shape: agent_doc_replica_apply_local) and confirm later replica calls still succeed instead of returning -1/null for the life of the IDE. This is the behavior the whole migration exists to guarantee and it cannot be proven without a host IDE.
- [/] [#mtds] [operator-verify] #panedraftunblocker live proof: on a Claude pane whose footer reads `⏵⏵ bypass permissions on · 1 shell · ← for agents` (the variant without `(shift+tab to cycle)`), with an EMPTY composer and no active turn, trigger JB Run Agent Doc. Expect dispatch to succeed instead of refusing with `timed_out` / `unblocker=wait_for_dispatch_ready_prompt`. Then repeat with unsent text typed in the composer and expect the new refusal naming the draft with `unblocker=submit_or_clear_pane_draft`, and NO injection over that text. Finally confirm a genuinely mid-turn pane (live spinner, bare prompt) still refuses as busy. Fixed in 17848a927. This item named binary 0.35.6 and a pending idle-boundary recycle; the installed binary is now 0.35.174 and has recycled many times, so only the live eyeball remains.
- [/] [#falsetyping-guard] #agent-doc-bug provenance-aware visible-write guard. INTEGRATED to main (merge on 5bcfc07b) + binary/cdylib installed + recycled; guard tests 7/7 green. JB install step DONE — the installed plugin is 0.2.350 in all four IDE configs, superseding the 0.2.205 zip this item named; only an IDE restart is missing, covered by #stickymdpaneverify. VS Code 0.2.38 install is UNVERIFIED — no agent-doc extension found under ~/.vscode/extensions. REMAINING (operator): restart the IDE, then live proof — type during replica churn (no wedge) + genuine unsaved edit still fails closed.
<!-- /agent:review -->

<!-- agent:icebox -->
- [ ] [#agentsignals] #agent-doc-feature [future/separate] Add a realtime agent:signals element after core realtime convergence is stable. Start with deterministic built-in signals for tests, queue, supervisor, and ipc; design custom user-defined signals as plugin-backed tracked work with Rust as the first target and future multi-language support; use tasks/software/realtime-signals-patchboard.md as the GUI/semi-programmatic reference; keep MLP enhancement optional/experimental and not load-bearing for deterministic states.
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
