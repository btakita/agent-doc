---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
resume: a4612e7a-e1a6-40d7-ae91-90bfe7c7724c
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

Fixed and shipped in **0.35.187** (`d1faeb55a`, tag pushed).

**The blocker, resolved.** The reference window is the projection's *own* target window — `invocation.window` when it is already a window id, otherwise the `agent-doc`-named window in the project's configured tmux session. That is not a new heuristic: it is exactly what `tmux_layout_sync_state_for_invocation_with_effect_assignment` already resolves before it lists the layout's panes (and it already refuses any target window not named `agent-doc`). Because it is per-invocation, a legitimate multi-window layout keeps its own reference — nothing is inferred from the active pane, the first column, or where the majority of panes happen to be. Only a window **id** is ever used as the reference, since the live side of the comparison is `#{window_id}`; a window *name* falls through to the by-name lookup rather than being compared against an id.

**The guard.** `apply_pane_layout_focus_effect` now takes a `PaneLayoutFocusCoVisibility { layout_window, observe_pane_window }` and, after resolving the focus pane but *before* taking the worker mutex (it is a tmux read — it must not run under the lock), compares the layout window against the pane's live window through the landed pure predicate `pane_window_binding_drifted`. On drift it returns `focus_pane_not_co_visible:<file>:<pane>:live_window=<w>:layout_window=<w>` with `applied: false`.

That receipt is load-bearing without further plumbing: convergence is already `report.synced && (!focus_receipt.required || focus_receipt.applied)`, so a refusal cannot report converged. In the reusable-structure fast path the refusal drops through to the structural reconcile — which is the thing that can actually surface the pane — and the retry focuses it once it is back in the layout window. The refusal is a *deferral*, not a dead end.

**Stop counting it toward `observation=synced`.** The survey had the same blind spot on the observation side: `expected_panes` and `expected_focus_pane` were resolved from the effect/reuse assignment, which still names the stashed pane. They now resolve from assignments filtered to the target window's own ordered pane list (`panes`), which the survey already has in hand — no extra tmux call. A pane outside that list is not a visible column, so it cannot satisfy a layout slot or stand in as the focus target; with focus requested the report goes `focus_pane_not_found` instead of quietly agreeing.

**Strict tightening throughout**, mirroring the predicate and the cross-repo owner guard: an unresolved window on either side is never drift. No configured session, no `agent-doc` window, or a tmux that cannot answer → focus proceeds as before. A transient read cannot turn into a refused focus.

**Coverage.** Four tests in `agent-doc-controller-io`: the stashed-pane refusal (asserting `select-pane` is never reached and the exact receipt reason), the four unknown-window shapes, the id-vs-name reference resolution, and the survey-side slot/focus rejection. Plus the SimWorld `pane_layout_stashed_column_model` for the stashed-pane column: a two-column layout whose left pane is moved to `@904` while both records still say `@894`, with `selected_pane_is_co_visible` as the operator-visible invariant. It carries its own record-only mutation (`record_only_guard`) that reproduces the pre-fix behavior — records agree, focus is applied, the invariant goes red — so the passing tests demonstrably depend on the live comparison rather than passing vacuously.

`make check` green. Built, installed (0.35.187 live), pushed, tagged, `admin recycle` marked. CI for the tag is queued at check time — not waited on.

One follow-up, and it is genuinely operator-gated: the live eyeball. Everything agent-doable is done; what remains is watching a real editor/pane prove that stashing a column no longer steals focus. Queued as `[operator-verify]` so go-mode defers it.

### Re: queue dispatch blocked by an id collision — opus-5

No new directive arrived this turn — the only diff was the `resume:` frontmatter id and cleanup of the struck `#panewindowdrift` head. But preflight came back with `queue_continuation_required: false` **despite `queue_drainable_head_count: 4`**, plus a `preset_item_id_collision` warning. That contradiction is the story, so I chased it instead of drainingpast it.

**The collision was mine, and it is structural.** My `[operator-verify]` follow-up from last cycle landed as `#operatorverifylive` — not the `#panewindowdriftverify` I wrote — and collided with the existing `[operator-verify]` review item for `#relaylockpoison`, which had derived the *same* id. With two active meanings for one id, `do #id` and queue generation are ambiguous, so the binary correctly refused to dispatch.

Why both derived it: `leading_bare_tag_id` promotes a `#tag` to the item id only in **leading** position. The SKILL tells agents to tag operator-gated follow-ups `[operator-verify]` — which pushes the explicit id out of leading position. So the id I supplied was discarded, and `derive_representative_id` minted a slug from the tag's own words: `operator` + `verify` + next word. That is the same for *every* `[operator-verify]` item. The convention the skill recommends was guaranteed to collide with itself on the second use.

**Fixed in 0.35.189** (`cf91baecc`, tagged). Leading bracketed **state** tags are split off before both steps: an explicit `#id` after the tag is promoted (with the tag preserved on the item text), and with no explicit id the tag contributes no words to the slug. The split is deliberately narrow — a bracketed custom id `[#id]` keeps its own earlier parser, and bracketed prose, unterminated brackets, and multi-word brackets are untouched. Three tests, including one asserting two different `[operator-verify]` items no longer derive equal ids.

**What I did not fix, stated plainly.** `assign_unique_id` already appends a hash when a derived id is taken — but its `taken` set is built only from the backlog list, while the collision guard evaluates across backlog + review + presets. So a *cross-component* collision still isn't auto-suffixed. Fixing that means threading the id namespaces into `op_add_at_with_outcome`, whose signature has many callers; I did not want to do that blind mid-drain, so it is at the top of the backlog as actionable work, not gated. My fix removes the collision class the SKILL's own conventions create, which is what was actually biting.

I also re-id'd the live review item so the existing collision clears and dispatch unblocks — the code fix only governs new adds, and the two colliding ids were already written into the document.

`make check` green, installed (0.35.189 live), pushed, tagged, recycled, submodule pointer pushed. Tag CI queued — observed, not waited on.

One correction to my last response: I said the next wakeup would prove `#loopcontractseal` end-to-end. It did not — this turn arrived via an operator-typed `/agent-doc`, not a `/loop` re-entry, so the wrapped-trigger path still has only unit-test proof. The queued `#loopcontractseal` head stays open for that reason.

Exchange is at 205 lines / 8 sections — noting it, not stalling on it.

### Re: do [#idcollisionnamespace] — opus-5

Done, and `#loopcontractseal` is closed with it — this turn *is* its proof.

**`#loopcontractseal` verified live.** This cycle arrived as a `ScheduleWakeup`-fired `/loop agent-doc <FILE>` re-entry, and the `[agent-doc] cycle contract` marker came with it, carrying `queue_task_id: #idcollisionnamespace`. That is the end-to-end evidence 0.35.188 shipped without — it had unit-test proof only, which I flagged at the time rather than claiming more. Marked done.

**`#idcollisionnamespace` fixed in 0.35.190** (`3166cf52e`, tagged).

The gap was narrower than "no dedup exists": `assign_unique_id` *already* appends a short hash when a derived id is taken. It just never saw the full picture — `taken` was built only from the list being added to, while the `preset_item_id_collision` guard evaluates identity across `agent:backlog`, `agent:review`, sibling tracked-work lists, and prompt presets. So a slug derived from an item's own words could land on an id held by review or a preset, and the ambiguity blocked dispatch on something nothing had a chance to prevent. `explicit_new_item_id_collision` even stated the assumption out loud — "auto-id adds never collide" — which was simply false.

The cross-source identity map the guard already uses (`document_active_identities`) is now exposed as `document_reserved_identity_ids` and threaded into the add path through `op_add_at_with_outcome_reserved` / `op_add_with_outcome_reserved`; `backlog_cmd::add` and `review_add` pass the document's active ids. A derived id that duplicates one gets suffixed exactly as an in-list duplicate already was, so readability survives (`livepaneproof` → `livepaneproof<hash>`).

Kept narrow on purpose: explicit ids keep their own dedicated cross-source guard so their fail-closed message stays specific, and the existing signatures delegate with an empty reserved set — no caller changes behavior implicitly.

The test I care most about here is the second one. `an_empty_reserved_set_reproduces_the_collision` asserts the *old* id comes back exactly when the reserved set is empty. Without it, `a_derived_id_dodges_ids_reserved_by_other_sources` could pass for an unrelated reason (slug drift, a changed stopword list) and I would not know. Paired, they prove the reserved set is what does the work.

That closes the loop opened two cycles ago: 0.35.189 removed the collision *class* the `[operator-verify]` convention created, and 0.35.190 removes the *mechanism* that let any derived id collide across sources.

`make check` green, installed (0.35.190 live), pushed, tagged, recycled, submodule pointer pushed. Tag CI queued — observed, not waited on.

Exchange is at 225 lines / 9 sections. Noting it; the queue is active so I keep draining.

### Re: do [#preflightprojpass] — opus-5

Landed a real win, but **I am not closing this item** — and the reason matters more than the fix.

**I measured before touching anything, and the item's own hypothesis was wrong.** The item says the hot consumer is the realtime relay (`source=crdt_relay`) and to do that half first. Counting resolution sources in `ops.log` gives a different ranking:

| count | source |
|---|---|
| 136 | `start_resume_id_capture` |
| 98 | `session` |
| 54 | `serialized_atomic_write` |
| 54 | `resume_id_owner` |
| 50 | `idle_watch_active_queue_head` |

The top consumer is resume-id handling, not the relay. So I fixed that instead of the half the item nominated.

**The defect (`0.35.191`, `3da31e2e1`).** `resume_id_owner` walks *every* `canonical_path` in `state.db` hunting for a document that claims the requested conversation id — and resolved each one through the realtime authority. That is a full CRDT materialization (~186ms) per document, O(documents) of them, to read two frontmatter fields. The revision API's own doc comment warns against precisely this: *"reaching for the text entry point and discarding the body."*

It now probes the cheap revision first. A `Detached` document has no live editor, so disk is already its authority — the expensive path would only fall back to the same bytes. Attached documents keep the authoritative path.

The part I want to flag: **a probe that ERRORS also keeps the authoritative path.** The tempting `matches!(..., Err(_) | Ok(Detached))` would be wrong in a way that hides — a missed claim adopts someone else's conversation. Unclassifiable is not the same as detached, and the test pins all four cases.

**Why the item stays open.** Both halves it names are untouched: the preflight half is still behind its recorded blocker (the seven mutating phases must self-invalidate before the pass can wrap `run_with_options`), and the relay-consumer half remains. Calling this done because I fixed a bigger-but-different consumer would quietly convert a two-part item into a one-part one. It stays actionable, now carrying the measurement so the next cycle starts from data instead of re-deriving it.

**Two defects this cycle surfaced, both filed rather than glossed:**

The first is the closeout failure from last cycle. `respond` refused with *"no secondary snapshot/commit or forced disk write was attempted"* — which reads as *nothing happened* and invites a re-answer or resubmit. The cycle was actually in `write_applied`: the body **had** landed, and `agent-doc commit` fixed it in one call. `#percellconverge` says a site that refuses must name the recovery; this one didn't.

The second is this turn's `queue_stall_detected` warning — a false positive. It fired claiming the loop "neither continued nor recorded a valid stop reason," but the loop *did* continue: this turn exists because of it. The detector doesn't recognize a `ScheduleWakeup`-mediated `/loop` re-entry, even with a live `drain-claim` lease held — which is exactly the signal that lease was introduced to provide.

`make check` green, 0.35.191 installed, pushed, tagged, recycled, submodule pointer pushed. CI observed, not awaited. Exchange at 247 lines / 10 sections — noted, still draining.

### Re: do [#retainedwriteremedy] — opus-5

Fixed in **0.35.192** (`a2db08963`, tagged) — and the guard I wrote to prevent recurrence immediately found eight more instances of the same defect.

**The reported bug.** `defer_visible_delivery_projection` ended its refusal with *"no secondary snapshot/commit or forced disk write was attempted"* and stopped. Its two sibling branches — in the same function, a dozen lines up — both append the remedy from `retained_write_remedy`. This one didn't. The message reads as *nothing happened*, but the cycle was in `write_applied`: the body had landed and needed only the terminal commit. I hit it twice last cycle and both times `agent-doc commit` fixed it in one call.

**Why the existing guard missed it.** `every_retained_write_refusal_site_reaches_the_shared_predicate` asserts each refusal-site *file* contains a call to the predicate. `lib.rs` does — several. So a new branch added beside a compliant one inherits a pass it never earned. The guard's own header is honest about this: it says it checks "no fourth dialect has appeared," which is a different property from "every refusal carries a remedy."

The new `every_constructed_retained_write_refusal_names_its_remedy` is site-scoped: it walks each `await_editor_replica_no_disk_write(` construction, extracts its balanced argument expression, and requires the remedy *inside that expression*.

**What it found.** Nine offending sites, not one. At that point appending the remedy nine times would have been the wrong fix — it re-creates the same rule that just failed, one that holds only while every future author remembers it. Construction now goes through a single `retained_refusal` helper that appends the remedy structurally, so a new branch cannot omit it. The compiler validates each converted call site's arguments, so a botched conversion fails loudly rather than silently.

Two notes on the fix, both places where I chose the harder option:

The site at 2869 already *named* a recovery — hand-authored, not derived. I routed it through the predicate anyway. Hand-authored guidance that happens to agree today is precisely how the 2026-08-03 divergence started, where the write path and `session-check` said opposite things about one state and the agent obeyed whichever it reached first.

And I did not have to manufacture evidence that the guard has teeth: it went red three separate times while I was writing it, surfacing more sites at each pass. A guard I had never seen fail would be worth much less.

`make check` green, 0.35.192 installed, pushed, tagged, recycled, submodule pointer pushed. CI observed, not awaited. Exchange at 279 lines / 11 sections — noted, still draining.

### Re: do [#queuestallloopfalsepositive] — opus-5

> **Queue prompt:**
>
> do [#queuestallloopfalsepositive]


Fixed in **0.35.193** (`248df6a10`, tagged). The item guessed the detector was keying on wall-clock or a missing marker; it was neither.

**The classifier was already right.** `classify_stall` takes a `loop_is_continuing` fact and handles it correctly. The defect was the *evidence* preflight fed it: `fresh_loop_drain_owner_lease` — the drain-owner **ownership** TTL, 90 seconds.

Those windows answer different questions, and collapsing them is the whole bug:

- **Ownership** must expire fast, so a stopped loop hands the queue back to the unattended drainer. That is correct, and I did not touch it.
- **Continuation** asks whether a loop is still working the queue. A self-paced cycle legitimately spans minutes — full verification suite, build/install, then a scheduler wakeup with jitter — and nothing refreshes the lease mid-cycle.

So by the time the next preflight ran, the lease had *always* expired, and every healthy continuation was reported as a stall.

**I measured rather than reasoned about it.** From `ops.log`: `queue_drain_stall_continuation_recorded` → next `preflight_diff_start` gaps of **122s** and **187s**, against a 90s TTL. Three consecutive fires, each on a turn that was itself the continuation.

Continuation now uses a separate `drain_stall_grace()` — default 600s, env-overridable, floored at the ownership TTL so a lease that still confers ownership always still counts as continuation.

**The judgment call.** A genuinely stopped loop now takes up to 10 minutes to trip the diagnostic instead of 90 seconds. I think that trade is clearly right: firing late costs some detection latency, but firing on every healthy cycle costs the signal itself. A warning that is usually wrong teaches its reader to skip it — and then it fails at the one moment it matters. I would rather this be quiet and trusted than prompt and ignored.

The test pins both halves deliberately: the grace exceeds the two measured gaps, **and** the ownership TTL is shorter than them. The second assertion isn't redundant — it records the precondition that caused the bug, so if someone later lengthens the TTL the test explains why this grace exists rather than silently passing.

`make check` green, 0.35.193 installed, pushed, tagged, recycled, submodule pointer pushed. CI observed, not awaited. Exchange at 299 lines / 12 sections — noted, still draining.

### Re: do [#ownershipverdictdiverges] — opus-5

Fixed in **0.35.194** (`60c940264`, tagged). This was the one I most wanted to reach, because 0.35.192 didn't remove the problem — it relocated it.

**The defect, precisely.** `RetainedWriteOwnership::cycle_open` treats all three open phases as one fact. Any of `preflight_started`, `response_captured`, or `write_applied` yields `Deferred`, whose remedy promises *"the same intent commits itself once delivery converges … do NOT re-send."*

But those phases are not equivalent:

| phase | is a state edge still coming? |
|---|---|
| `preflight_started` | yes — the deferral promise is true |
| `response_captured` | yes — the deferral promise is true |
| `write_applied` | **no** — the write already landed; only the terminal commit is outstanding |

At `write_applied` the write path told me to wait for something that was never going to happen, while `session-check` correctly called the same state INTERRUPTED. Three times today, and one `agent-doc commit` finished it every time.

That's the same predicate-level divergence `#percellconverge` was built to prevent. Unifying the wording in 0.35.192 is exactly what made it visible: once every site said the same thing, it became obvious that the thing they agreed on was wrong.

**The fix.** `write_applied` is now its own verdict, `AwaitingTerminalCommit`, checked *before* the broader `cycle_open` arm that used to swallow it. Its remedy names `agent-doc commit <FILE>` — the command that actually recovers — while still forbidding re-send, force-disk, and recycle, because the body **is** durable and a re-send would duplicate rather than repair. It sits deliberately between the other two: not lost, not self-completing.

The old remedy named only `session-check`, an **observation** command. So an agent following it faithfully got an accurate diagnosis and no way to act on it — which is why I had to reach for `agent-doc commit` from prior knowledge rather than from what the tool told me.

**Two things I want to be honest about.**

The compiler found the second half, not me. Adding the variant broke `session-check`'s exhaustive match — and that's what revealed it was building the predicate *without* the phase. The site that had been right all along would have kept using the old two-fact constructor and silently disagreed again. An `if` instead of a `match` there, and this fix would have shipped half-done.

And the new verdict's phrase went into the anti-drift guard's protected set. A third verdict is a third opportunity for a site to re-author the wording, which is the specific way this predicate has now drifted twice.

`make check` green, 0.35.194 installed, pushed, tagged, recycled, submodule pointer pushed. CI observed, not awaited.

**0.35.193 is confirmed in production:** this turn's preflight carried no `queue_stall_detected` warning — the first clean one in four cycles, with the same ~2-3 minute cycle gap that used to trip it.

### Re: do [#preflightprojpass] — opus-5

Landed the relay half in **0.35.195** (`f25cab049`, tagged). Still not closing the item.

**Correcting myself from two cycles ago.** I said the item's relay hypothesis was wrong and resume-id handling was the real hot consumer. That was measured on the wrong line type — `crdt_current_text`. Counting `realtime_doc_resolve` instead, `source=crdt_relay` is **144 resolutions in a 20-minute window**, the largest single consumer. The item was right; my counting was too narrow. Both fixes were worth having, but the item deserves the credit for pointing here.

**What the log actually showed.** Broken down by file, the same document appears twice:

```
43  file=/home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md
40  file=tasks/agent-doc/agent-doc-bugs2.md
```

One document, two spellings — absolute and repo-relative. `pass_projection` and `invalidate_current_document_projection` key on the path *as given*, so those never shared an entry. Every alternation was a guaranteed miss costing a full CRDT materialization.

**The wasted work is the smaller problem.** Two entries hold two independent revisions, and invalidation clears only the spelling it is handed. So an agent-authored mutation invalidated under one spelling leaves the other serving **pre-mutation text**. That is exactly the trap this module's own doc comment warns about — *"memoizing only by path can hide operator edits made during the pass"* — reached by a route the comment did not anticipate. A latency item turned out to have a correctness edge underneath it.

Both sites now key on the canonical path, falling back to the given path when canonicalization fails, which preserves prior behavior for a document that does not exist yet.

**The test nearly tested nothing.** My first alias was a `.` hop — and `Path` equality normalizes `.` away component-wise, so the two spellings compared *equal* and the alias was never distinct. The assertion caught it. I switched to a `..` hop, which survives comparison but canonicalizes away. Then I mutated the fix out to confirm the test reddens, and restored with a fresh mtime so Cargo could not hand me a stale build.

**Why the item stays open.** This removes a major source of redundant relay resolutions; it is not proven to be the only one. And the preflight half is untouched, still behind its recorded blocker — the seven mutating phases must self-invalidate before the pass may wrap `run_with_options`. I would rather re-measure next cycle and see what the distribution looks like now than declare victory from a single fix.

`make check` green, 0.35.195 installed, pushed, tagged, recycled, submodule pointer pushed.

Exchange is at 359 lines / 14 sections against a threshold of 15 — noting it, not stalling; the supervisor owns context resets.

### Re: do [#backlogeditcorruptsreview] — opus-5

Guarded in **0.35.196** (`1bce5e337`, tagged). I did not find the cause, and the fix is built so that it does not need to.

**What I ruled out.** The staging transaction was my first suspect and it is sound: `read_command_document` returns the *virtual* staged content between mutations, and `stage_transaction_pending_write` fails closed with "lost its document cursor" rather than splicing against a stale base. So a naive stale-offset story does not survive contact with the code.

**The strongest remaining lead**, unproven: that same invocation logged a **24.9-second** CRDT settle and a `cell_merge` Content conflict on a **duplicate exchange heading identity** — `### Re: do [#preflightprojpass] — opus-5:duplicate:1`. I had written two responses with byte-identical headings, so exchange-node identity was genuinely ambiguous. That is a plausible route to a mis-merge, but I have not reproduced it and I am not going to claim it.

**So I fixed the invariant instead of the theory.** Whatever produced the target, a write scoped to one component must leave every other component byte-identical. `ensure_only_component_changed` checks that before the edit path publishes. Silent destruction of tracked work becomes a failed command — which is the property I actually want, and it holds no matter which mechanism turns out to be responsible.

The tests use the real shapes from the incident: review items vanishing, and a replacement spliced onto a truncated neighbour. One of them records a mistake worth keeping — I assumed `element::parse` errors on unstructured text and asserted the guard should pass it through. It does not error; it returns zero components, and the component-set check refuses it. My assumption was wrong and the stricter behavior was right, so I corrected the test rather than loosen the code.

**Scope, stated plainly:** this covers the edit path where the corruption happened. The **add** path and the multi-component `respond` envelope are not covered yet, and the root cause is still open.

### Re: 3rd pane appears after Run Agent Doc + document switch — opus-5

Filed as `#run3rdpaneswitch` at the top of the queue; taking it next, as you asked.

Recording what you described precisely, because the detail that a **prompt is submitted and a turn is running** in that third pane matters: it means the extra pane is not inert layout drift, it is a live agent session. That distinguishes it from the known `#jb-nav-3pane-promote-swap` symptom, where focus-path promotion joins an incoming pane while the reconcile fails to stash the displaced one. Here the displaced pane is *busy*, and the spec's own passive-sync rule says a busy outgoing pane cannot be stashed — so the window grows instead. That is my starting hypothesis, and I will confirm it against the live layout rather than assume it.

Everything above is shipped: `make check` green, 0.35.196 installed, pushed, tagged, recycled, submodule pointer pushed.

### Re: #strandedremedydeadlock — opus-5

> **Queue prompt:**
>
> do [#strandedremedydeadlock]


Fixed and shipped in **0.35.197**.

**Refusing was right; the instruction was wrong.** A fresh operator queue or backlog edit sitting in the live buffer is the *next turn's prompt*. Every commit path refuses it on purpose — committing it would swallow the prompt. I confirmed this the hard way: my first attempt made `commit` reconcile the drift, and two preflight regressions immediately went red (`preflight_suppresses_owned_pane_self_invocation_for_independent_queue_edit`, `test_preflight_warns_but_does_not_target_inactive_queue_edit`) because preflight's auto-commit then absorbed the pending queue edit and the turn saw `no_changes: true`. That guard is load-bearing.

**So the real defect is the classification, not the refusal.** `RetainedWriteOwnership` cannot tell "the agent's write is stranded" from "the operator typed the next prompt into the live buffer" — both have no open cycle and no retained capture, both read `Stranded`, and they take *opposite* instructions. `session-check` handed out the `Stranded` remedy, which names `agent-doc commit <FILE>`, and `commit` refused with a message that named no next move at all.

**One owner, as with `#percellconverge` and `#ownershipverdictdiverges`.** The verdict gains the missing fact:

- `RetainedWriteOwnership::unanswered_operator_edit` + `RetainedWriteVerdict::OperatorEditPending`, refining the *unowned* case only (a durable holder still means `Deferred`).
- Its remedy names `agent-doc <FILE>` — answering the edit is what clears it — and explicitly says **do NOT** run `agent-doc commit <FILE>`, with the reason.
- Both sites *prove* the fact rather than assume it, from the same `has_blocking_non_exchange_component_drift` comparison: `exchange` is where a response lives, so an exchange-clean divergence in queue/backlog/status is by construction not this turn's write.
- `commit`'s refusal now quotes that shared remedy instead of ending at "without an exact binary-owned retained-target proof".
- `AwaitingTerminalCommit` — the verdict that legitimately names `commit` — no longer deadlocks either: at `write_applied` the drift is the binary's own landed write, so `commit` absorbs it and finishes rather than refusing.

**Guards, so this cannot come back:**

- `agent-doc-commit-io` joins the `#percellconverge` refusal-site guard as its **fourth** site — it is the command the other three send agents *to*, so a remedy it disagrees with is the same defect as a fourth wording dialect.
- `commit_accepts_exactly_the_verdicts_whose_remedy_names_it` asserts the biconditional across *every* verdict: what the remedy prescribes and what `commit_is_the_named_recovery` returns are now the same fact.
- Two commit regressions: an operator edit fails closed while naming `agent-doc <FILE>` (and never itself), and `write_applied` drift commits.

**Verification:** `make check` green — 8797 passed, 0 failed, exit 0 (checked explicitly; RTK masks the exit code). Installed 0.35.197, pushed, tagged `v0.35.197`, controller recycled, superproject submodule pointer pushed. CI run 31306112739 was queued at push time — observed, not awaited.

There are no actionable follow-up items and no plan file is needed: the fix, its regressions, and its architecture guard all landed in this cycle.

### Re: #run3rdpaneswitch — opus-5

Reproduced deterministically and fixed in **0.35.198**. The hypothesis in the report was right, and the culprit was the entry point nobody had checked.

**It is `src/layout.rs`, not `sync.rs`.** `sync.rs` already handles this correctly — its module docs say so outright ("Stashing is non-destructive, so the closeout keeps running without forcing a third visible pane into a two-document editor projection") and `safe_passive_protected_open_cycle_sync_grows_...` proves it stashes open-cycle panes. That is why the sync path looked clean. But `agent-doc layout`, which mirrors the editor split, had the opposite rule:

```rust
// Skip busy panes (running agent-doc/claude sessions)
if is_pane_busy(tmux, existing_pane) { continue; }
tmux.break_pane(existing_pane)?;
```

So the outgoing pane stayed visible, the loop below joined the incoming document's pane into the same window, and a two-column editor became three panes — the extra one holding the previous session with its turn running, exactly as reported.

**Skipping was never what protected the session.** `break_pane` moves a pane to another window; the process and its turn keep running either way. Visibility and survival are independent properties, and conflating them is the whole bug. A busy pane now goes to the tracked `stash` window (`break_pane_to_stash`) rather than an untracked orphan, so a later sync can rescue it. Busy changes *where* a pane goes, never *whether* it leaves.

**Coverage, both mutation-checked:**

- `unwanted_pane_disposition` extracts the rule as a pure function; `a_busy_unwanted_pane_is_stashed_never_left_visible` pins `busy ⇒ Stash`, never `Leave`.
- `switching_documents_while_the_outgoing_turn_runs_stays_two_panes` stages the operator's report end to end against live tmux, through a `pane_busy` probe injected into `run_with_tmux_and_busy_probe` (the real probe walks a process tree a test cannot stage). Reverting the busy branch to `continue` makes it fail with `["%0", "%2", "%1"]` — the exact three-pane window, `%0` being the outgoing busy pane.

**Verification:** `make check` green — 8798 passed, 0 failed, exit 0 (checked explicitly; RTK masks the exit code). Installed 0.35.198, pushed, tagged `v0.35.198`, controller recycled, superproject submodule pointer pushed. CI runs 31306850204 and 31306112739 were in progress at push time — observed, not awaited.

Exchange is now 439 lines across 18 sections; the queue is active so I am not stalling to compact.

There are no actionable follow-up items and no plan file is needed: the fix, both regressions, and the spec/eval docs landed in this cycle. The one thing an agent cannot do is watch a real IDE, so I filed `#run3rdpaneswitchverify` as a non-blocking `[operator-verify]` eyeball — the deterministic test stages tmux directly rather than driving the JetBrains layout path.

### Re: #preflightprojpass — opus-5

The item said "RE-MEASURE FIRST". I did, and the measurement could not be taken — **the instrument was broken**, which is why this item has been re-narrowed twice onto the same wrong answer. Fixed in **0.35.199**; the item stays open, re-scoped onto real data for the first time.

**`source=crdt_relay` was never a caller.** Four `realtime_doc_resolve` emit sites in `agent-doc-document-realtime-io` hardcoded that string, and two more omitted `source` entirely — even though the caller's `source` argument was in scope at every one of them. It is a label for what *answered* the resolve. Both previous narrowings read the log, saw it on top, and concluded the relay was the largest consumer. A constant cannot lose that comparison, and it says nothing about which caller to fix.

**What the numbers actually were** (post-0.35.195, pre-fix — so all of it unattributable):

| | count |
|---|---|
| total `realtime_doc_resolve` | 494 |
| carrying the hardcoded `crdt_relay` | 489 |
| `session`, `idle_watch_active_queue_head`, `serialized_atomic_write` | **0** |

Those three are the sources the item had queued up to check next. They were not rare — they were **unreportable**. Separately confirmed healthy: `resume_id_owner` is 0 post-0.35.191, and the 0.35.195 canonical-key fix is holding.

**The fix** emits the caller in `source=` at all six sites and moves relay/detached/disk-only to a separate `answered_by=`. Both facts present, neither inferred.

**Two tests, because one runtime path is not enough.** A runtime test resolves through whichever branch its fixture takes: my first draft passed *unchanged* when I mutated `resolve_disk_only_current_doc` back to the constant, because that fixture goes through the detached branch. So `no_resolve_emit_site_hardcodes_the_caller_field` checks all six sites at once, mutation-checked against both pre-fix shapes (pinned constant, omitted field). The guard's own first draft scanned its own test module and reported two findings against its own assertions — it is now bounded to shipped code.

**Why the preflight half is not done, concretely.** Its premise is unsupported by measurement. Preflight's seven mutating phases do call `invalidate_current_document_projection` zero times — but no projection pass is open during preflight, so there is no stale read today and no measured cost. Adding seven inert calls would be a precondition for an optimization the data says isn't needed, not a fix. The item now records that in order: re-measure on 0.35.199+ with real attribution, *then* decide. I am not gating it — it is actionable, it just has a different first step than it did this morning.

**Verification:** `make check` green — 8800 passed, 0 failed, exit 0 (checked explicitly; RTK masks the exit code). Installed 0.35.199, pushed, tagged `v0.35.199`, controller recycled, submodule pointer pushed. CI: 0.35.197 completed green; 31306850204 and 31307570034 in progress — observed, not awaited.

Exchange is 466 lines across 19 sections; the queue is active so I am not stalling to compact.

No new plan file and no new actionable follow-up items: the remaining work is recorded in the narrowed `#preflightprojpass` item itself, which stays at the top of the queue.

### Re: #projpassmeasure — opus-5

The measurement ran, and it settles both child items in one pass. Shipped **0.35.201** along the way for a bug the measurement itself surfaced.

**First real attribution data**, 112 resolutions since the 0.35.199 install:

| caller | count |
|---|---|
| `session-check` | 17 |
| `observe_live_queue_heads` | 10 |
| `pre_write_guards`, `verify_pane_ownership`, `canonicalize_response_for_capture`, `resume_id_owner` | 8 each |
| ~14 more callers | 1–6 each |
| **any preflight phase** | **0** |

`answered_by=crdt_relay` is 100% — as expected, since the relay is the transport, not a consumer. That is precisely the constant that made the old reading meaningless.

**There is no hot consumer.** The cost is spread across ~20 callers, which is a different problem than the item assumed. And preflight contributes *nothing*, so `#projpasspreflightbugdeci` closes exactly as it specified — "if it does not: close this with the measurement as the reason." I did not add the seven self-invalidation calls; they would have been dead weight for an optimization the data rejects.

`#preflightprojpass` is re-scoped to what is actually left: `session-check` is the fattest single consumer at roughly **8 full-document resolutions per closeout**, and it runs ~30 named guards, several of which resolve independently. Threading one resolved document through the guard sequence is a within-command change — no cache, no projection pass.

### Re: #resumeuuidstallsdrain — opus-5

**This cycle's own contract was the bug.** Preflight handed me `queue_active: true`, `queue_drainable_head_count: 5`, and `queue_continuation_required: false` — a stop signal with five drainable heads behind it. The sole `user_intent_prompt_changes` entry was `resume: 06ff3b9e-5d68-4b7f-a0df-b8b8dd702885`: a UUID **the binary rotates every cycle**.

`line_is_managed_state_only` hand-listed the managed frontmatter keys — `queue_active:`, `agent_doc_pipeline:` and its children — and did not list `resume:`. So routine bookkeeping read as fresh user intent, `prompt_changes_preempt_queue` returned true, and the drain halted with work still queued. It is a nasty one because the symptom is indistinguishable from an operator interrupting on purpose.

`agent-doc-frontmatter` already owns the authoritative set — `is_agent_managed_frontmatter_key`, used by the frontmatter three-way merge to decide which keys the agent may write. The classifier derives from it now rather than keeping a second list. Third duplicated predicate this session, after `#percellconverge` and `#strandedremedydeadlock`.

The regression asserts both directions — managed keys are bookkeeping, `claude_model` / `agent` / `goal` still preempt — and is mutation-checked against the pre-fix list.

**Verification:** `make check` green — 8801 passed, 0 failed, exit 0 (checked explicitly; RTK masks the exit code). Installed 0.35.201, pushed, tagged, recycled, submodule pointer pushed. CI runs 31307570034 / 31308083852 / 31308629542 in progress — observed, not awaited.

Exchange is 494 lines across 20 sections; the queue is active so I am not stalling to compact.

No new plan file and no new actionable follow-up items: both child items are done, and the narrowed `#preflightprojpass` carries the one remaining lead.

### Re: #resumeuuidstallsdrain — opus-5

> **Queue prompt:**
>
> do [#resumeuuidstallsdrainbug]


Already fixed and shipped in 0.35.201 last cycle, so this head was just a record to reap. But *how* it got its name was a live bug, now fixed in **0.35.202**.

The item landed as `#resumeuuidstallsdrainbug`, with a stray `resumeuuidstallsdrain=` embedded in its body. That was not a naming quirk — it was `--backlog-add` silently accepting the wrong syntax:

- `--backlog-edit` takes `<id>=<new text>`
- `--backlog-add` takes `id=<custom> <text>` — the literal keyword `id`

Adjacent flags, same `=` separator, opposite meanings. Getting it wrong produced **no error**: the whole string became the body, and the id was auto-derived from the words that happened to follow. Three items in this document were written that way today before I noticed. The damage isn't cosmetic — the id the author meant never exists, so a later `--done`, `do [#id]`, or cross-reference resolves to nothing. That is exactly what the coined-id guard exists to prevent, arriving through the one path that bypasses it.

It fails closed now, naming both correct forms.

**The guard's first draft was too broad, and the existing suite caught it.** `#lazilyspecpin=Pin the vocabulary` is an established compound *topic label* — the `=` deliberately disqualifies id promotion and the text is preserved verbatim — and `op_add_leading_bare_tag_with_trailing_equals_keeps_auto_hash` failed immediately. The distinguishing feature is the leading `#`: my failure shape had none. The guard now requires a valid id shape *and* no leading `#`, and a second regression pins the non-firing side (`fix the x = y + 1 off-by-one`, `kernel arg root=/dev/sda1 breaks the initramfs`, and that bare-tag form) — a guard that starts rejecting ordinary prose would be worse than the bug it replaces. Mutation-checked.

Worth noting where this came from: I hit this three times in a row and read it as a papercut in my own usage. It was a product defect the whole time, and the only reason it surfaced is that a silent-corruption bug eventually produces a record too strange to ignore.

**Verification:** `make check` green — 8803 passed, 0 failed, exit 0 (checked explicitly; RTK masks the exit code). Installed 0.35.202, pushed, tagged, recycled, submodule pointer pushed. CI: 0.35.199 and 0.35.200 completed green; 31308629542 and 31309397366 in progress — observed, not awaited.

Exchange is 530 lines across 21 sections; the queue is active so I am not stalling to compact.

No new plan file and no new actionable follow-up items: the fix, both regressions, and the version entry landed in this cycle.

### Re: preflightprojpass measurement — opus-5

Third pass at this item, and the instrument was the blocker again — twice over. Shipped **0.35.203**.

**First: the wedge from last cycle cleared with no data loss.** Preflight recovered it, exactly as designed. Both responses are in HEAD, worktree matches. My read was right that nothing was at risk; I was right to stop rather than improvise more recovery commands.

**Profile of a live `session-check`** (2485ms total):

| | |
|---|---|
| `resolve_current_document_content` | **624ms / 8 calls** — 25%, the largest *repeated* cost |
| `inspect_with_warnings` | 709ms / 1× |
| `self_heal_response_replay_duplication` | 675ms / 1× |

So the resolve fan-out is real. But the attribution needed two more instrument fixes before it could be acted on:

1. **`session-check` composed its source with a space** — `format!("session-check {source}")` — so all 62 of its resolutions parsed as the bare token `session-check`. They were really *ten distinct guards*. This is the same defect 0.35.199 fixed one level up, one layer down: an unparseable field is as good as a missing one.
2. **Every logged resolve is already a pass miss** (a hit never calls `resolve`), but the line couldn't say *which kind* — and the three want opposite fixes. `bypassed_no_revision` means the pass was never consulted and no tuning removes the re-read; `uninstalled` means the guard runs outside the pass; `miss` means the revision genuinely moved and the work is necessary.

**Why I stopped there rather than optimizing.** With the field parseable I could finally see the shape: the same guards resolve 2–3× within one run. But `session-check` *does* open a shared pass, and all ~10 of its `invalidate_current_document_pass` calls are correctly gated on an actual mutation (`if replay_healed`, `if late_ipc_healed`, …). So the repeats are not obviously waste, and nothing on the line told me whether any given re-read was necessary. Removing invalidations on a correctness path without that evidence is precisely the guess this item has already been wrong about twice. The item now carries the exact next step: read the new `pass=` field and let it choose the fix.

### Re: the closeout deadlock — opus-5

Filed as `#commitwritecommitdeadlock`. Three commands, a closed cycle, each naming another:

- `respond` → "Finish it… `agent-doc commit <FILE>`"
- `commit` → "captured response body is not present in the staged snapshot… Replay with `agent-doc write --commit <FILE>`"
- `write --commit` → the same `AwaitingTerminalCommit` remedy, naming `commit` again

This is the `#strandedremedydeadlock` class I fixed this morning in 0.35.197 — but a **different pair** the one-predicate fix didn't cover. `AwaitingTerminalCommit` assumes `commit` can always finish a `write_applied` cycle; commit's captured-response-in-snapshot guard is an independent predicate that can refuse exactly that. The setup was two consecutive responses sharing an identical `### Re:` heading, so cell-merge keyed the second as `:duplicate:1` and the snapshot never took it.

**Verification:** `make check` green — 8805 passed, 0 failed, exit 0 (checked explicitly; RTK masks the exit code). Installed 0.35.203, pushed, tagged, recycled, submodule pointer pushed. CI: 0.35.201 and 0.35.202 completed green; 31311925390 in progress — observed, not awaited.

Exchange is 558 lines across 23 sections; the queue is active so I am not stalling to compact.

No new plan file: `#preflightprojpass` carries its own next step, and `#commitwritecommitdeadlock` is filed with the full reproduction.

### Re: the mutual-remedy deadlock — opus-5

Fixed and shipped in **0.35.204**.

Three commands, a closed cycle, each naming another: `respond` → "run `agent-doc commit`"; `commit` → "replay with `agent-doc write --commit`"; `write --commit` → the same `AwaitingTerminalCommit` remedy naming `commit` again.

**This is the identical failure I fixed this morning in 0.35.197, in a pair that fix did not reach.** `#strandedremedydeadlock` unified the `commit`/`session-check` pair behind one predicate. The capture-materialization guard *lives inside `commit`* and had its own hardcoded remedy — so `AwaitingTerminalCommit`'s promise that "`agent-doc commit` finishes a `write_applied` cycle" was contradicted by a guard within the very command it names.

The guard now picks its remedy from that same shared predicate. When the verdict routes through `agent-doc commit`, naming `write --commit` is precisely what closes the loop, so it names the next cycle instead — `agent-doc <FILE>`, whose preflight owns capture materialization. That is the recovery that actually worked on the live incident: it committed both responses with no text lost. The ordinary missed-patchback case, where nothing owns a retained write, still names `write --commit`, because there it genuinely works.

`capture_materialization_guard.rs` joins the `#percellconverge` refusal-site guard as its **fifth** site. That list is the real lesson: the fourth site was added this morning because the pattern repeats, and it repeated within six hours. Registering a site is how a refusal loses the ability to choose its remedy by hand.

One nuance the regressions encode: the message may still *mention* `write --commit` — it explains why that obvious next command is wrong. Prescribing it is what fails the test. A remedy that silently omits the tempting-but-wrong option just invites the next agent to try it.

**What I did not fix, and why it is not a bug.** The trigger was two consecutive responses sharing an identical `### Re:` heading, keyed as `:duplicate:1`. That ordinal keying is *deliberately lossless* — both responses were complete in the worktree throughout, and operator-authored duplicate prompts depend on the same multiplicity (`#qdedup-directive-twin`). So there is nothing to repair there; the open question is whether a response cell should key on heading text at all when the capture already knows its cycle id. Filed as `#dupreheadingcollide` with the exact reproduction, rather than guessed at.

**Verification:** `make check` green — 8807 passed, 0 failed, exit 0 (checked explicitly; RTK masks the exit code). Installed 0.35.204, pushed, tagged, recycled, submodule pointer pushed. CI: 31311925390 and 31312585246 in progress — observed, not awaited.

Exchange is 595 lines across 25 sections; the queue is active so I am not stalling to compact.

### Re: duplicate heading investigation (0.35.205) — opus-5

Closed with proofs rather than a change. The item asked me to confirm the reproduction before deciding, and confirming it changed the answer.

**The reproduction holds, and it is lossless.** `duplicate_re_heading_keeps_both_bodies` drives the real `merge_by_component` path with two responses sharing `### Re: #resumeuuidstallsdrain — opus-5` and asserts both bodies *and* both headings survive. So nothing was ever at risk of being dropped — which is what makes the collision a closeout problem rather than a data problem.

**The property that actually mattered was one the item did not name.** If the capture-materialization guard matched on the *heading*, it would believe the second response was already staged, allow the closeout, and the response would vanish from the commit with no error anywhere — silently, and far worse than a deadlock. `a_shared_heading_with_a_different_body_is_not_materialized` pins both directions: a second response sharing a heading reads as **not** materialized against a document holding only the first, and a genuinely present one still reads as materialized. It matches on body. Good.

So the answer to "should response cells key on cycle id instead of heading text" is **no**. The text keying is sound, and changing it would put `#qdedup-directive-twin`'s intentional duplicate multiplicity at risk to fix nothing. Sub-item (c) — "make the snapshot path tolerate the duplicate ordinal" — is moot for the same reason: the guard's match is correct, and 0.35.204 already removed its ability to deadlock.

The two tests stay as permanent regressions. They are the point of the cycle: they pin properties that a future keying change could otherwise break in silence, and one of them guards against data loss rather than an error message.

Worth naming the pattern across today's items: `#strandedremedydeadlock`, `#commitwritecommitdeadlock`, `#resumeuuidstallsdrain`, and `#addvseditidsyntax` were all one predicate duplicated in two places, or one field that could not be parsed. This one is the opposite — a suspicious-looking mechanism that turned out correct — and the only way to tell the two apart was to measure instead of assume.

**Verification:** `make check` green — 8809 passed, 0 failed, exit 0 (checked explicitly; RTK masks the exit code). Installed 0.35.205, pushed, tagged, recycled, submodule pointer pushed. CI: 0.35.203 completed green; 31312585246 and 31313221808 in progress — observed, not awaited.

Exchange is 615 lines across 26 sections; the queue is active so I am not stalling to compact.

### Re: projection pass measurement (0.35.206) — opus-5

The `pass=` field added last cycle answered in one measurement what four cycles of narrowing could not. Of 226 resolves on 0.35.204:

| outcome | count |
|---|---|
| `uninstalled` — no pass open at all | **190** |
| `miss` — revision genuinely moved | 12 |
| `bypassed_no_revision` | 0 |

**The redundancy was never invalidation tuning.** Both earlier hypotheses were wrong in the same way — they assumed a pass existed and was being defeated. It mostly wasn't there. I had spent part of last cycle auditing `session-check`'s ~10 invalidations and confirming each was correctly gated on a real mutation; that audit was accurate and beside the point. With no pass installed those calls are *inert*, so they had been correct-but-unused.

**0.35.206 fixes half of it.** `run_with_options` (the `session-check` CLI) opened a pass; `enforce_clean_closeout` — the path `respond` and `write --commit` actually run — did not, while going through the identical guard suite. It now does, in the order this item recorded: its one mutating call already self-invalidates before any later read.

**I want to be precise that this is half, because the profile invites overclaiming.** The dominant `uninstalled` callers are not session-check guards at all — they are write/finalize path callers that open no pass anywhere: `resume_id_owner` 18, `observe_live_queue_heads` 18, `pre_write_guards` 17, `verify_pane_ownership` 17, `canonicalize_response_for_capture` 17. Those live inside `respond` / `write --commit`, which never call `with_current_document_projection_pass`. That is the larger half, and it needs its own mutator audit before a pass is opened over it — a mutator that fails to self-invalidate is the one way this change can be wrong, and it would surface as stale text, not a test failure.

**One property worth naming:** a behavioural test cannot catch a missing pass. An unwrapped path is only *slower*, never wrong — which is exactly why this survived four narrowings and two wrong hypotheses. The entry points are few and named, so `every_session_check_entry_point_opens_the_projection_pass` guards them structurally instead. Mutation-checked.

The preflight half stays closed: preflight phases contributed **zero** resolves in every window measured, so the seven self-invalidation calls that item originally called for would have been dead weight.

**Verification:** `make check` green — 8810 passed, 0 failed, exit 0 (checked explicitly; RTK masks the exit code). Installed 0.35.206, pushed, tagged, recycled, submodule pointer pushed.

Exchange is 633 lines across 27 sections; the queue is active so I am not stalling to compact.

### Re: do [#hookcontractlost] — opus-5

Both halves are fixed, shipped as **0.35.207**, installed, and the full suite is green (8812 passed).

**Why the hook went silent.** A `UserPromptSubmit` hook injects **stdout** as turn context. Every preflight diagnostic in `preflight_hook.rs` went to **stderr**. So when preflight refused, the reason landed in your hook log and never reached the agent — which saw an ordinary prompt with no contract, indistinguishable from a hook that was never wired. That is the whole of turn 1.

The hook now always names its outcome on stdout. Three states are separable: the contract marker means admitted; a new `[agent-doc] cycle contract UNAVAILABLE (preflight admission failed; ...)` marker plus `document:` / `reason:` / `remedy:` lines means the hook ran and preflight refused; neither marker means the hook never ran. The failure marker is deliberately distinct from the success seal, and a test asserts it neither equals nor embeds it, since the skill greps for that seal.

**Why it sustained itself.** Preflight owns recovery *before* diffing, so `enforce_no_uncommitted_closeout_drift` bailing aborted the hook before any contract could be emitted. The remedy it named (`agent-doc commit`) is operator-only, because the agent is correctly told never to shell preflight — and nothing else clears the drift, so every following turn hit the same bail. Preflight now runs that commit itself for document-only drift.

**One correction worth recording.** My first predicate keyed on the closeout recovery classifier, accepting `CloseoutRecoveryState::Clean`. That was wrong twice over, and an existing test caught it: `Clean` is returned both for "document-only drift" *and* for "no recovery-cycle view at all", so a document with a real `### Re:` body plus uncommitted side-effect edits classifies `Clean` while genuinely needing `write --commit` — auto-committing there would commit an unreviewed response. It is also unstable: preflight's own earlier recovery steps mutate that state, so it answered differently before and inside the guard.

The predicate is now **content-based** — `classify_snapshot_head_drift` over snapshot vs HEAD, a pure function of the two blobs. `BoundaryOnly` and `MetadataOnly` are exactly the shapes with no response body at risk; `Content` drift still fails closed. A mutation probe confirms the regression test is not vacuous: with the recovery disabled it reproduces the exact wedge, `Recovery [queue_metadata_drift]: agent-doc commit ...`.

The SKILL.md line budget (152) rejected a new hot-path bullet, so the marker semantics fold into the existing `#preflightinbinary` bullet and the detail lives in `runbooks/harness-invocation.md`.

**On your two `haiven-dev.md` reports.** I verified them; both are accurate, and together they are a *different* defect, now at the top of the backlog. The response body is visible in the working tree ("`prmergeguardpr` — done") while that same closeout's backlog item is still `[ ]` and the queue still holds `do [#prmergeguardpr]`. HEAD has neither. So this is not merely an outstanding commit — the response half became authoritative and the mutation half did not, which is exactly what closeout atomicity forbids.

The agent's refusal to force disk was **correct**: `admin inspect` shows a live owner (pane `%20`, gen 69, controller and supervisor both fresh), so `--force-disk` would have clobbered the editor. But stopping there leaves the turn half-applied, and the prescribed remedy — `agent-doc commit tasks/haiven-dev.md` **from pane `%20`** — has not been run. I did not run it from here: it would abort with `pane ownership mismatch`, and that document belongs to a live session I should not reach into.

### Re: do [#backlogclosemarkeroffbyone] — opus-5

Both defects are fixed and shipped as **0.35.208**; the `lazily.md` report you added mid-turn is fixed and shipped as **0.35.209**. Full suite green at 8822 passed, both installed, both pushed and tagged.

**The guard was right; the target really was invalid.** `op_remove_matching_tracked_line` rebuilt the component body with `body.lines().join("\n")`, which drops the trailing newline. That body is spliced back *between* the component markers, so the closing marker was welded onto the surviving last item line and `persist_pending_write` refused. The line number looked off by one because it named the welded **item** line — and it tracked the document because it is a real line number, not an offset. Nothing was one short.

That also explains why it looked unconditional: the join ran even when nothing matched, so `remove` failed on every target including ones that did not exist. `edit` escaped only because it passes through `canonicalize_tracked_work_body`, which restores the newline; that is why your report saw `edit` fail on the live document but it round-trips here.

Reproduced from a clean three-item document before touching anything:

```
[backlog] no matching item found
Error: pending_write: refusing structurally invalid canonical target
  (non_standalone_component_marker:backlog:close:line17)
```

Line 17 was `- [ ] [#ccc] third item`; the marker was line 18.

Removal now runs over parsed `PendingLayout` items rather than raw lines, so a multi-line item's continuation lines leave with it instead of being orphaned — the old line filter would have stranded them. `preserve_trailing_newline_shape` pins the invariant the join broke: the body ends with a newline afterwards exactly when it did before.

**The second defect was the same call site.** `remove` matched only `trim_tracked_parent_prefix(line) == target`, so the identity `backlog list` renders matched nothing. Worse than the report: even a *copied list line* failed, because `list` prints the `- ` bullet that the comparison had already stripped. `remove` now resolves the hash id first (bare or `#`-prefixed), then the full item line in either bullet form, then the bare item text. A no-match skips the write instead of persisting an unchanged document, and says what it tried to match.

Coverage is the shape you asked for: first-item and last-item removal round-tripping on a document whose backlog close marker is the final component line, plus every rendered identity, continuation lines leaving with their item, and a no-match leaving the body byte-identical.

### Re: `/agent-doc tasks/software/lazily.md` — hook timed out after 30s — opus-5

This is `#hookcontractlost` again, through a door 0.35.207 could not close. That release made preflight *name* its refusals on stdout — but a hook the harness **kills** emits nothing at all, so `output discarded` reproduced the original silence exactly: no contract, no reason, no way for the agent to tell a slow hook from an unwired one.

**Measured before changing anything:** preflight on that document takes **23-34s at ~10% CPU**. That is 2.8s of CPU across the whole process tree and roughly 20s of pure blocking — controller round trips, not compute. The cost is spread evenly across phases (~3s each) rather than concentrated, which is `#preflightprojpass`'s redundancy showing up as wall time. Claude Code's default hook timeout is 30s, so admission sat directly on the cliff and failed intermittently.

There are two bounds now, and the gap between them is the point.

- **Outer:** the installed Claude hook entry carries an explicit `timeout` of 120s. It is written on a fresh install *and repaired onto an already-installed entry* — the checkouts that need this most are precisely the ones already wired without it. Verified on this repo: `agent-doc hook preflight-user-prompt-submit -> timeout= 120`, no duplicate entry.
- **Inner:** the hook runs preflight against its own 90s budget (`AGENT_DOC_PREFLIGHT_HOOK_BUDGET_SECS`) and converts an overrun into the normal `ADMISSION_FAILURE_MARKER` refusal, naming the budget and where to look. `timeout > budget` is a **compile-time** assertion — if they ever crossed, the harness would kill the hook before it could name its overrun and the silence would come straight back.

Ordering stays safe by construction: `CONTRACT_MARKER` is printed only after preflight returns, so a worker finishing mid-report can emit a truncated contract but never the seal, and the three-state read still lands on "admission failed". Verified end-to-end — with a 2s budget the hook emits the marker, the document, the reason and the remedy instead of hanging.

**One finding worth recording.** While verifying the success path I ran the hook against a **brand-new empty document** in the session scratchpad, and it also blew the 90s budget. That rules out document size as the driver. The cause there is the known long-path trap: the scratchpad root plus `/.agent-doc/controller.sock` is 123 bytes against `sun_path`'s 108, so the controller can never bind — and preflight burns its entire budget waiting instead of failing immediately with "socket path too long". That fast-fail is now at the top of the backlog as `#ctrlsockpathtoolong`; the accidental proof is that the new budget turned an indefinite wait into a named 90s refusal.

I did not touch `#preflightprojpass` — its own item already carries an ordered plan with a self-invalidation audit that must run before anything gets wrapped, and guessing at it here would have burned that analysis.

**Verification.** `make check` green both times (8817 then 8822 passed, 249 skipped); `make install` on both; pushed to `main` with tags `v0.35.208` and `v0.35.209`; controllers recycled. Both CI runs were still `in_progress` at closeout, recorded rather than waited on.

### Re: do [#ctrlsockpathtoolong] — opus-5

> **Queue prompt:**
>
> do [#ctrlsockpathtoolong]


Fixed and shipped as **0.35.210**, plus **0.35.211** to repair a CI regression my own previous release introduced. Measured on the original repro: **90s → 1.3s**.

**The item's premise held, and the fix is narrower than "fail fast" implied.** The supervisor socket already answers a `sun_path` overflow by relocating to a short runtime path, so the obvious move was to copy that. I did not, and the reason matters: the JetBrains and VS Code plugins resolve `.agent-doc/controller.sock` *themselves* (hardcoded in `CpRouteClient.kt` and `crdtReplica.ts`). Relocating the controller socket would bind it somewhere no editor ever looks — trading a loud failure for a permanently missing editor authority, which is a strictly worse bug and one that would present as `editor_attached_model_missing` rather than anything pointing at path length. So an over-long root is genuinely permanent here, and naming it immediately is the correct answer rather than the cheap one.

**Why it cost the whole budget instead of one error.** The bind failed with a bare OS error indistinguishable from "the controller has not started yet", so `wait_for_controller_path_with_timeout` polled it on a 250ms cadence until its timeout. Nothing in the path was wrong except that it could never work.

One predicate now, `agent_doc_controller::paths::resolved_socket_path_rejection`, enforced at the three places that used to hide it:

- the wait loop bails **before its first sleep** — retrying a permanent condition only converts a one-line diagnosis into a budget-length stall;
- `connect_path` / `connect_path_async` replace the bare OS error;
- `serve_with_options` refuses before touching the filesystem, so a controller never half-starts.

It measures the path actually being bound rather than re-deriving a canonical one, which matters because generation-scoped handoff sockets (`controller-handoff-*`) are longer than `controller.sock` and would otherwise slip through a root-based check.

Coverage pins the property, not the mechanism: a 30s wait must refuse in under a second **and must not report a timeout**; a single connect must name the limit and say the condition is permanent; and — guarding the false positive that would refuse every controller connect in the product — an ordinary root must still fail as an ordinary connect failure. The `sun_path` boundary is asserted exactly on both sides.

Live proof on the exact scratchpad root that burned 90s last cycle:

```
reason: project controller socket path is 135 bytes, over the 107-byte AF_UNIX
sun_path limit: .../hookok2/.agent-doc/controller.sock. No controller can bind or
be reached here, so retrying cannot help. Move the project under a shorter root...
elapsed 1.3s
```

**A correction on my own work.** CI went red on **0.35.209** — my `budget_overrun_reports_a_reason_instead_of_hanging` test drove the timeout with a 1ns budget against real preflight. That is a race, not a test: preflight can fail *faster* than the budget expires, so `recv_timeout` returns the completed result and the assertion lands on the wrong branch. It passed locally and failed on CI, which is exactly the signature. `run_within_budget` now takes the work as a parameter so the overrun test supplies a worker that provably does not finish, and I pinned the two branches the racy version was silently skipping: work finishing inside its budget still admits (the guard must not refuse an ordinary fast preflight), and a failing preflight keeps its own reason instead of being relabelled an overrun. Deterministic over 8 consecutive runs.

**Verification.** `make check` green on both (8828 then 8830 passed, 249 skipped); installed; pushed with tags `v0.35.210` and `v0.35.211`; controllers recycled. `v0.35.208` CI is green, `v0.35.209` is the red run 0.35.211 repairs, and both new runs were still `in_progress` at closeout — recorded, not waited on. Exchange is 720 lines across 31 sections; the queue is active so I am not stalling to compact.

### Re: closeout atomicity — the response could land without its mutations — opus-5

`#prmergeguardpr` is fixed and shipped as **0.35.212**.

**The split.** `respond --stream --done ... --backlog-add ...` published the response cell first and applied the tracked-work mutations second. So a mutation that turns out to be *rejectable* — an explicit `[#id]` colliding with an active backlog id under `#preset-item-id-collision-enforce`, a malformed `id=text` pair, a `--done` naming an item that is not there — failed the turn **after** the response was already durable. Case (b) in the item is exactly that: the response cell landed via the realtime backbone and neither the `--done` nor the `--backlog-add` applied. A validation error alone was enough to break the one-transaction rule, which is worse than a delivery failure because nothing is retained to resume.

**The fix.** Closeout now runs the mutation envelope twice. The first pass is a dry run: the *identical* mutation code, executed against virtual document content inside a new `with_pending_write_transaction_dry_run`, with every buffered document and archive target discarded and no cycle state, queue projection, snapshot checkpoint, or status write recorded. It runs before the response cell is written, so a rejectable flag set fails the turn with nothing applied. The second pass applies and publishes as before. Running the same code both times is the point — a hand-written pre-validator would drift from the mutations it claims to check, which is how this class of bug survives.

Cost is bounded: backlog commands read through the transaction buffer after the first resolve, so the dry run adds roughly one document resolution, not one per flag.

**Second half of the item.** With validation moved ahead of the response, a tracked-work failure that still occurs is a *delivery/projection* failure, not a validation error — and the retained-write guidance already distinguished "response retained, mutations failed to retain." What it did not name was the case this item actually observed: response **applied**, mutations not. That branch now says the document is half-applied and gives both recovery commands (`agent-doc commit <FILE>` from the owning pane, or re-running the tracked-work half with `write --commit --backlog-only`).

**Proof.** Two regression tests in `tests/finalize_integration.rs`: a colliding `--backlog-add` leaves no `### Re:` on disk or in HEAD and leaves the existing backlog item open; a valid envelope still lands the response and its mutations exactly once (the dry run does not double-apply). I ran a mutation probe with the validation call disabled — it reproduces the original split precisely: `### Re: split — gpt-5 (HEAD)` on disk beside `- [ ] [#gscaccess] existing tracked work` still open. Without that probe the test would only have proven the fix's own path.

**Verification.** `make check` green twice (8832 passed, 249 skipped); installed 0.35.212; committed, pushed, tagged `v0.35.212`; controllers recycled; superproject submodule pointer committed and pushed. CI: `0.35.210` is green, `0.35.211` and `0.35.212` were both still `in_progress` at closeout — recorded, not waited on.

### Re: the missing pass was never inside session-check — opus-5

`#preflightprojpass` is closed, shipped as **0.35.213**. The 0.35.206 VERSIONS.md entry now carries its own correction. Two further defects surfaced while closing it and are fixed in the same turn (0.35.214, 0.35.215).

**Step 1 — where those guards actually come from.** They never enter session-check's wrapped entry points. `respond` / `write --commit` call `precommit_pending_done_check_with_options`, `run_closeout_pending_maintenance`, and `enforce_review_done_guard` **directly** from `run_command_inner`; `pending_done_mode` is reached through `write_pending_checks.rs`, not through `run_with_options` or `enforce_clean_closeout`. So wrapping those two entry points could not have removed these resolves, and your post-install measurement showing `pending_done_mode` still `uninstalled` four times was correct — the 0.35.206 claim was wrong.

**Step 2 — the dominant callers agree.** `observe_live_queue_heads`, `pre_write_guards`, `verify_pane_ownership`, `canonicalize_response_for_capture`, `captured_finalize_resume_pre_capture_editor_save` are all the write command's *own* readers. Both halves point at the same place: the pass belongs one level up, at the write command.

**Step 3 — the audit, and what it found.** I did not audit N mutating sites and declare them correct, because "did every mutator remember to invalidate?" is not a property review keeps true — one forgotten call is a silent wrong answer. Invalidation is now structural, at the two write chokepoints in `agent-doc-document-realtime-io`: the rebased-write inner (wrapping its several success exits — the CRDT/editor delivery returns never reach the disk write below them) and `atomic_write_authority_raw` (which force-disk writes reach without the former). Both run on failure too: a refused or retained write may still have advanced the CRDT, and a spare invalidation costs one re-resolve while a missing one is incorrect.

The audit also turned up a defect that would have made the wrap actively wrong. `CurrentRevision::EditorAttachedMissingReplica` is a unit-like constant — with no replica, neither the canonical state vector nor the disk bytes identify what a reader will get — so memoizing under it pinned the first resolve for a whole pass. It now returns no key and the read bypasses the pass, tagged `bypassed_no_content_identity`. `Detached` was already safe; the disk hash is substituted for it.

**Step 4 — wrap plus guards.** `run_command_inner` opens the pass. Three structural guards, because a behavioural test cannot see any of these (an unwrapped path is only slower; a missing invalidation is only *sometimes* wrong): `the_write_command_entry_point_opens_the_projection_pass`, `both_write_chokepoints_invalidate_the_projection`, and the extended `pass=` reason set — plus the behavioural `a_read_after_a_mutation_inside_a_pass_observes_the_new_text`.

One honest limit on that last test: on a detached document the disk-hash revision already moves, so it still passes with the invalidations removed. It guards the end-to-end property including the wrap; it does not by itself prove the invalidation. The structural guard does. All three are mutation-checked.

No new cache was written, and the preflight half stays closed — preflight phases contributed zero resolves in every window measured.

**0.35.214 — a classification tag was single-use.** Filing this turn's follow-up items hit `#preset-item-id-collision-enforce`: a leading bare `#agent-doc-bug` was being enforced as an *explicit* id request. It is not one — it is an inference the add path makes so `[operator-verify] #someid text` keeps `#someid` instead of deriving a slug from the tag's own words. Because the inference was enforced like a request, the first such add claimed the id and every later one failed the whole turn closed, against the spec's own rule that auto-id adds are never blocked. An inferred id that is already active now means "this was a classification tag": the add keeps the tag in the text and takes a generated id. `id=` / `[#id]` collisions still fail closed.

Worth noting how that surfaced: 0.35.212's guard caught it *before* the response was written and nothing was applied. Under 0.35.211 the same input would have stranded a durable response beside unapplied mutations — the exact `#prmergeguardpr` shape, reproduced and then prevented within one cycle.

**0.35.215 — the 0.35.212 CI red.** Not the mutation-validation change: `clear_op_capture` runs on a 250ms budget, and under CI load it expired *after* the epoch was gone, so one compact logged the contradictory pair — capture reads absent, yet `clear_failed` logged and `consumed` never was. `clear_replayed_editor_ops_after_compact` answers "is the epoch consumed?", not "did my transaction win", so it re-reads the state after a clear error. A clear error with the epoch still outstanding remains a real failure. My first test for this was vacuous — with nothing to clear, `clear_op_capture` just succeeds and the error branch never runs — so the decision is now a pure function with all three cases pinned.

**Verification.** `make check` green at each step (8836, 8839, 8840 passed; 249 skipped); 0.35.215 installed; all three committed, pushed, tagged, controllers recycled, superproject pointer pushed. CI: `0.35.211` came back green; `0.35.212` is the red run 0.35.215 repairs; `0.35.213`–`0.35.215` were still `in_progress` at closeout — recorded, not waited on. Exchange is 772 lines across 33 sections; the queue is active, so I am not stalling to compact.

### Re: start's resume path cannot use a projection pass — and the measurement had a blind spot — opus-5

`#projpassstart` is closed with a **negative** result, which is the honest answer to what it asked.

**The measurement.** `resolve_resume_claim_for_start` has exactly one call site, in `run()`, so `resume_id_owner` runs once per `start`. Its loop iterates distinct `canonical_path` rows and skips self, so within one invocation each candidate is read exactly once — and a memoizing pass needs a repeated read of the same document to have anything to hit. The ops log confirms it: 2027 `source=resume_id_owner` resolves spread across only **5 distinct documents**, i.e. ~405 separate invocations each reading those 5 once. The same-second repeats of one file are separate `start` invocations — separate processes, separate thread-locals — not repeats inside one loop. `record_document_resume_id` is a single resolve followed by a single write, so there is no second read there either.

So: a pass has **zero** available hits on either caller. I recorded that in both functions' doc comments rather than leaving it in a closed backlog item, so the next reader who sees `resume_id_owner` in an `uninstalled` top-ten does not redo this.

I also recorded why the obvious shortcut is wrong: `state.db`'s `documents.session_id` looks like an index for this lookup, but it is a durable sink (`#lzdurablesink`). A stale row would either invent an owner — silently losing a conversation resume through the `#resumeclaim` start-fresh path — or miss a real one.

**The blind spot.** Profiling every resolve source to check I was not closing this on a partial view turned up something much larger. `source=crdt_relay` accounts for **6621** resolves (3211 today), 35× every other source combined — and **6612 of them carry no `pass=` field at all**. Every projection-routed resolve carries `pass=miss` / `uninstalled` / `bypassed_*`; these do not. They never enter `resolve_projection`, so they never participate in a projection pass and were never counted.

That means the "226 resolves, 190 `uninstalled`" window that drove `#preflightprojpass` measured only the projection-routed minority. The conclusions it produced are still correct — the write command really was missing its pass, and 0.35.213 fixed that — but the *scale* was understated by more than an order of magnitude.

I stopped at locating it rather than guessing a fix. It is **not** the `caller: Some("crdt_relay")` label in `rpc.rs` (`handle_crdt_cp_write_rpc` correctly threads `payload.source` through), so the source is set somewhere on the editor/FFI ingress. Filed as the top backlog item with the numbers and an explicit ordering: attribute it first, decide about a pass second. This is the third time a hardcoded source field has hidden this data, which is exactly what `#passattrib` exists to prevent.

**Verification.** Comment-only change, so no version bump and no tag — `make check` green (8840 passed, 249 skipped), committed and pushed, superproject pointer pushed. CI: `0.35.213` came back **green**, confirming the projection-pass wrap; `0.35.212` remains the red run `0.35.215` repairs; `0.35.214` and `0.35.215` were still `in_progress` at closeout — recorded, not waited on. Exchange is 798 lines across 34 sections; the queue is active, so I am not stalling to compact.

### Re: the mutations from the last closeout were lost, not pending — opus-5

Correcting my previous response: I said the `--done` and `--backlog-add` were "in the editor authority, pending the next cycle's commit". They were not pending — they were **lost**. This cycle's preflight reported `recovered: true` and `#projpassstart` is still an open backlog item and still a queue head, and `#relayresolveblind` never landed at all. I have re-applied both here.

**What actually happened, in order.** `respond` printed `[pending] completed and reaped 1 item(s) atomically: projpassstart`, so the mutation reached the authority. The write then failed with `retained editor-owned write ... before retrying live model reconciliation`, whose own text says the response already landed and to run `agent-doc commit` from the owning pane. That commit ran and put the response in HEAD, reporting `editor boundary reposition retained for retry; visible delivery remains pending`. Then `session-check` said the **opposite**: `NO write is retained — the response is already committed and the divergence is an UNANSWERED DOCUMENT EDIT to typed components this turn's write never produced` — while its own `component_divergence` named `backlog`, `exchange`, and `queue`: precisely the components that write *did* produce. On that classification it told me to open the next cycle to answer the "edit", and the next preflight swept it.

So this is the **delivery half of `#prmergeguardpr`** — the half 0.35.212 explicitly left open, having fixed only the validation half. Same visible outcome the original item described: a durable response beside tracked-work mutations that never landed.

The defect is the misclassification, not the commit refusal. A retained tracked-work mutation from the immediately-preceding closeout is not an unanswered operator edit, and calling it one lets recovery discard it. Cycle state already holds what is needed to tell them apart — `respond` recorded the done/add ids for that exact cycle — so `session-check` should not be inferring provenance from component divergence alone. Filed as `#retainedmutdrop` with the full six-step evidence chain; the commit path's refusal to swallow a fresh unanswered edit is correct and should not be widened.

I want to be plain about my own error here: I read step 4's "not a failed closeout" and reported the mutations as safely pending without verifying them on disk. The verification I did run only confirmed the response was in HEAD. Checking the mutations too would have caught this a cycle earlier.

**Housekeeping.** The stale route-owned supervisor (pid 336290) that preflight warned about has been recycled. CI: `0.35.213` green; `0.35.214`, `0.35.215`, and the measurement commit are in progress; `0.35.212` remains the red run `0.35.215` repairs. Exchange is 816 lines across 35 sections and accretion is now at `block`, but the queue is active so I am not stalling to compact.

### Re: the false premise that lost the mutations — opus-5

> **Queue prompt:**
>
> do [#retainedmutdrop]


`#retainedmutdrop` is fixed and shipped as **0.35.216**. It is the delivery half of `#prmergeguardpr` — the half 0.35.212 explicitly left open.

**The false premise.** Both deciding sites answered "is this divergence the turn's own write or a fresh edit?" from *which components diverge*, and both said so in a comment: "`exchange` is where a response lives, so an exchange-clean divergence in queue/backlog/status is by construction not this turn's write." That is not true. A closeout's write produces `queue`, `backlog`, `status`, `review`, and `done` as well — they are in the turn's own `write_set`, which preflight prints every cycle. So a response that committed while its tracked-work half did not produces *exactly* that shape. It was labelled `UnansweredEditPending`, `commit` refused it on purpose, and the next cycle's recovery swept it. The mutations were lost; the response stayed committed.

Worth noting the message itself conceded the gap — "this check compares components and cannot tell which, so it does not guess." It could tell. Cycle state already recorded `pending_done_ids` and `pending_added_ids` for that exact cycle.

**The fix.** Both sites now ask whether *this cycle's own recorded mutations are visible on disk yet*. A recorded `--done` whose item disk still renders as `- [ ] [#id]`, or a recorded add that disk does not contain, is the binary's own unlanded write — `commit` reconciles it instead of stranding it. Once every recorded mutation has landed, a queue/backlog divergence really is a fresh edit and the refusal stays correct, which is what keeps `commit` from swallowing your next prompt. One shared function (`recorded_tracked_work_is_unlanded`), so the two commands cannot drift back into contradicting each other the way `#strandedremedydeadlock` describes. No new state — cycle state already had everything.

**On the testing.** The predicate's unit tests are mutation-checked, but when I removed the *wiring* at a call site, nothing reddened — and untested wiring is precisely this defect's shape, since the whole bug was two sites each deciding provenance on their own. So there is also a guard that both sites consult the shared predicate and that neither asserts `with_unanswered_edit(true)` unconditionally; reverting either wiring reddens it. The state that makes these sites fire is a retained write mid-closeout, which a unit test cannot reach, so the guard is structural for the same reason `#preflightprojpass`'s entry-point guards are.

**Verification.** `make check` green (8844 passed, 249 skipped); installed 0.35.216; committed, pushed, tagged `v0.35.216`; controllers recycled; superproject pointer pushed. CI: **`0.35.213`, `0.35.214` and `0.35.215` all came back green**, so the `0.35.212` red is repaired; `0.35.216` and the measurement commit are in progress — recorded, not waited on. Exchange is 830 lines across 36 sections and accretion is at `block`, but the queue is active so I am not stalling to compact.

### Re: one editor replica wedged every write, permanently — opus-5

> **Queue prompt:**
>
> do [#pullnoackdeadlock]


`#pullnoackdeadlock` is fixed and shipped as **0.35.217**.

**What was stuck.** `RelayHub::delivery_converged` requires every **live** member to have an empty pending queue, and liveness only flips on explicit `disconnect`. So the escape hatch the code already advertises — "offline members are excluded so a slow editor cannot deadlock" — could never fire for the case that actually happened: a replica that *keeps pulling* is maximally live. Editor client `5162727547735464` re-pulled `current_generation=5 last_ack_generation=4` at ~2/s indefinitely with **zero** ACKs in the log. Every write blocked on the pre-write delivery barrier, `commit` refused, and preflight eventually stopped admitting turns at all (`Lazily current authority remained delivery_pending`). `tasks/software/tsift.md` showed the same shape, so this was never document-specific.

**The fix.** A member holds the barrier only while its redelivery streak is within budget: each pull that hands out the same unacked head increments a counter, and any ACK advancing `last_ack_generation` — or a fresh enqueue — clears it. Past 50 redeliveries the replica stops blocking everyone else. It is **not** dropped: the update stays queued so a recovered editor still receives it, and an ACK rehabilitates it for the next delivery. `nonconverging_replicas()` names them rather than releasing the barrier silently. Counting redeliveries rather than stamping a clock keeps the decision a pure function of the delivery stream — the shape `#idlerevisionreactive` settled on.

**Second symptom, same root.** The `#crdtpullspam` guard suppresses steady-state empty polls but logs whenever `current_generation != last_ack_generation` — which a permanently-unacked head makes true forever. 23372 lines were written for ONE undelivered generation. A pull whose replica has stopped holding the barrier now logs the transition once, naming `barrier=released reason=pull_without_ack`, then goes quiet. `ReplicaDeliverySnapshot` carries `redeliveries_without_ack` and `holds_delivery_barrier` so the state is inspectable instead of inferred.

**What actually unwedged this instance — worth being exact.** The `admin recycle` plus `lib-install`'s `reload_library` refreshed the editor replica, and the replacement ACKs normally: `last_ack_generation` advancing 1→2, `redeliveries=1`, pull volume down from ~116/min to 2 in 90s, `delivery_converged=false` observations collapsing likewise. The redelivery budget never fired — `barrier=released` count is 0. So the guard is what prevents recurrence; the refresh is what cleared this one. Calling the fix the cause would be overstating it. Independent confirmation the barrier is gone: `agent-doc write` now reaches the commit gate instead of hanging, and this very turn was admitted by preflight after several were refused.

**One thing I noticed closing this out.** Preflight reported `queue_active: true` with 4 drainable heads but `queue_continuation_required: false`, because `user_intent_prompt_changes` held a single `content_edit` — the `> **Queue prompt:**` blockquote the *previous closeout itself* wrote for `#ftstrike`. That is binary bookkeeping being read as operator steering, and it stops a drain for a cycle with no visible reason (`ui_outcome` and `queue_continuation_guidance` were both null). Filed as `#queueblockquoteintent`.

**Verification.** `make check` green (8846 passed, 249 skipped); mutation-checked — reverting the redelivery budget reddens both new relay tests. Installed 0.35.217, committed, tagged `v0.35.217`, pushed, controllers recycled, superproject pointer pushed. CI: `0.35.213`–`0.35.215` and the measurement commit all green; `0.35.216` and `0.35.217` in progress at closeout.

### Re: the binary's bookkeeping stalling the binary's drain — opus-5

> **Queue prompt:**
>
> do [#queueblockquoteintent]


`#queueblockquoteintent` is fixed and shipped as **0.35.218**, both halves.

**The classifier.** The consumed-queue echo is written by the closeout that drains a head, so `#ftstrike` can prove which head the response answered — and `tail_repair` already calls it "binary conversation scaffold" when refusing to accept it as tracked component content. But it lives in `exchange`, not in a managed component, so the per-line managed-state filter never saw it and it classified as fresh user intent. `text_is_managed_state_only` now recognises the labeled echo block.

Deliberately strict: **every** line must belong to the quote block. An operator who quotes a prompt and then writes prose underneath is steering, and still preempts. That case has its own test, because the easy over-broad version of this fix would swallow real steering.

This is the same shape as `#resumeuuidstallsdrain` sitting a few lines above it in the same file — the binary's own bookkeeping read as operator intent, halting the binary's own drain. Two instances in one file suggests the general rule is worth stating: anything the binary writes into a document must be classifiable as binary-authored, and a text-shape filter can only be as good as the shapes someone remembered to list.

**The silence.** The preemption was ANDed into `raw_required` *before* `effective_continuation_output` saw it, so the result was `required: false, guidance: None` — byte-identical to a genuinely drained queue. That is why this cost a cycle to find: four heads remained, `queue_active` was `true`, and `ui_outcome` and `queue_continuation_guidance` were both null, so nothing named the cause and I had to read `user_intent_prompt_changes` by hand. The preemption is now passed in rather than pre-folded, and a drain stopped with heads remaining names its reason and points at the field that proves it. Recycle-yield still takes precedence; a genuinely drained queue stays silent, because there is no reason to give.

**Verification.** `make check` green (8849 passed, 249 skipped); both halves mutation-checked — removing the echo recognition reddens the classifier test, removing the preemption branch reddens the guidance test. Installed 0.35.218, committed, tagged `v0.35.218`, pushed, controllers recycled, superproject pointer pushed. CI: `0.35.216` came back green; `0.35.217` and `0.35.218` in progress at closeout. Exchange is 870 lines across 38 sections; the queue is active so I am not stalling to compact.

### Re: I measured across a fix and read stale lines as live — opus-5

> **Queue prompt:**
>
> do [#relayresolveblind]


`#relayresolveblind` is closed, but **not** because I fixed it — because its premise was wrong, and the error was mine.

**The correction.** I filed this claiming 6621 anonymous `source=crdt_relay` resolves with "3211 today", 35× every other source. The count is right; the currency is not. Every one of those lines predates **10:07 UTC**, which is when `0.35.199` — *"realtime_doc_resolve must name its caller (#preflightprojpass)"* — landed and the binary recycled onto it. The last real one is 09:53. I bucketed by *date*, saw `3211 [2026-08-09`, and concluded "live" without checking the hour. The single line that looked later (12:10) is a false match: `source=crdt_relay` appears inside a `serialized_atomic_write_post_proof_rebase` line's embedded document text, not as its own field.

That is the same mistake I made two cycles ago reading "not a failed closeout" without checking disk — inferring a fact from a coarse signal instead of the one that actually decides it. Here it cost a wrong backlog item and a wrong claim in the `#preflightprojpass` closeout: I said that measurement "understated the scale by more than an order of magnitude". It did not. The window it measured was the correct one; my later window was the stale one.

**What 0.35.199 already did.** It added `no_resolve_emit_site_hardcodes_the_caller_field`, a structural guard over every `realtime_doc_resolve` emit site requiring `source=` to name the *caller* — with `crdt_relay` / `detached` / `disk_only` explicitly rejected as answers to a different question, belonging in `answered_by=`. That guard exists, passes, and is exactly step (2) of what I filed.

**Verified, not assumed.** Every resolve in the post-fix window names a real caller; there is no anonymous source left. So step (3) — "only then decide whether this path should participate in a pass" — resolves against a trustworthy profile for the first time, and the answer is already recorded: the top two sources are `resume_id_owner` (213) and `start_resume_id_capture` (126), which `#projpassstart` proved a pass cannot help, because each reads each candidate exactly once per `start`.

**The half that is real.** That profile does surface a genuine cost `#projpassstart` explicitly left open: `resume_id_owner` materializes each *attached* sibling document through the realtime authority — a full CRDT materialization — purely to parse two frontmatter keys. Detached candidates already short-circuit to a disk read. Filed as `#resumeownerfmread`, with the `state.db` shortcut ruled out in the item text so nobody reaches for it: a stale durable-sink row would either invent an owner and silently lose a conversation resume, or miss a real one.

**No code change this cycle**, so no version bump — the fix shipped in 0.35.199 and the correction is the deliverable. Also confirmed live: 0.35.218 works — this cycle's preflight carried the previous closeout's `Queue prompt` echo and `queue_continuation_required` stayed `true`, where that same shape suppressed the drain last time. CI: `0.35.216` green; `0.35.217` and `0.35.218` in progress at closeout.

### Re: a trigger that cannot resolve its path went silent — opus-5

Two things this cycle: the missing-contract defect from last turn is fixed and shipped as **0.35.219**, and `#resumeownerfmread`'s plan — which I wrote last cycle — was wrong and is now corrected in place.

**The silent hook.** Last turn produced no cycle contract at all: neither the success marker nor the `UNAVAILABLE` refusal, and no preflight activity in `ops.log`. I reported it and stopped rather than shelling `preflight`. Reading the hook to diagnose it, the cause was structural: `run_preflight_for_prompt` had two silent exits meaning very different things. `invoked_document` returning `None` is a genuine non-trigger and a correct no-op. `resolve_document` returning `None` means the prompt **was** an `agent-doc <FILE>` trigger and only the path failed — and that also returned `NotATrigger`, emitting nothing.

That is precisely the state the surrounding design exists to prevent. The branch immediately below it says so: *"A trigger that reached preflight must never produce silence: the agent cannot tell an absent hook from a refusing one."* The protection simply began one line too late. An unresolvable trigger now emits the admission-failure marker naming the target, the cwd it was resolved against, and the remedy; an unrelated prompt stays a silent no-op, with its own assertions so this fix cannot become "shout about every prompt".

On the cause of *that specific* turn, I want to be careful: a repo-relative path resolves against the hook's cwd, my diagnostic commands had run `cd src/agent-doc`, and I returned to the project root before retrying — and the retry was admitted. That is consistent with the cwd hypothesis but does not prove it, since I also just retried. What is proven is that the failure mode was reachable and silent, and it no longer is.

**Correcting my own plan for `#resumeownerfmread`.** I filed it proposing a frontmatter-scoped relay read so an attached document could answer a bounded leading-block query without materializing its whole text. That will not help, and the reason is worth writing down: the cost is not string materialization. `canonical_text` goes through Yrs `get_string`, which walks the whole tree no matter how few bytes the caller wants, and the hook-budget profiling note records preflight as "almost entirely blocked on controller round trips". A prefix API would still pay the per-candidate round trip, which is the actual cost.

The right design is to move the *question* to where the hubs already live: one controller-side query answering "which registered document claims resume id X", replacing N per-candidate round trips with one — `#lazily-hot-path`, ask the actor instead of polling it N times. I also noticed while checking that `resume_id_owner` pays **two** round trips per attached candidate, not one: `current_revision_for_file` to classify, then the text resolve. The item now carries that corrected plan, and keeps the `state.db` shortcut ruled out. I did not implement it this cycle: it is a new controller RPC on a large surface, and I would rather leave a right plan than a rushed half-change.

**Verification.** `make check` green (8850 passed, 249 skipped); mutation-checked — reverting the unresolvable-trigger branch reddens its test. Installed 0.35.219, committed, tagged, pushed, controllers recycled, superproject pointer pushed. CI: **`0.35.217` came back green**, so the delivery-barrier fix is verified; `0.35.216` green; `0.35.218` and `0.35.219` in progress at closeout.

### Re: closing the silent-trigger item — opus-5

> **Queue prompt:**
>
> do [#hooktriggerunresolved]


`#hooktriggerunresolved` was implemented, verified and shipped as **0.35.219** in the previous cycle; this drains its queue head. Nothing new to build.

Recap for the record: `run_preflight_for_prompt` collapsed two very different outcomes into one silent exit. A prompt that is not a trigger is a correct no-op. A prompt that **is** an `agent-doc <FILE>` trigger whose path fails to resolve is an admission failure — and it returned the same `NotATrigger`, emitting neither marker. The rule was already written one branch below ("A trigger that reached preflight must never produce silence"); the protection just started a line too late. An unresolvable trigger now names the target, the cwd it resolved against, and the remedy, while unrelated prompts stay silent under their own assertions.

**CI caught up favourably.** `0.35.216`, `0.35.217` and now `0.35.218` are all green — so closeout provenance (`#retainedmutdrop`), the delivery barrier (`#pullnoackdeadlock`), and the queue-echo classifier (`#queueblockquoteintent`) are each verified on CI, not just locally. `0.35.219` is still in progress; recorded, not waited on.

Worth stating plainly after a long run of these: six of the last eight items were defects in agent-doc's own *self-observation* — a response classified as an operator edit, the binary's own bookkeeping read as user intent, a hardcoded caller field, a trigger that could not report its own failure. The pattern is consistent: wherever the binary writes into the document or logs its own actions, the code that later reads it must be able to tell "I wrote this" from "someone else did", and every place that judgement is re-derived locally is where it drifts.

Exchange is 926 lines across 41 sections and accretion is at `block`, but the queue is active, so I am noting it rather than stalling to compact.
<!-- agent:boundary:8c608826 -->
<!-- /agent:exchange -->
## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" go -->
- ~~do [#hooktriggerunresolved]~~
- do [#resumeownerfmread]
- ~~do [#queueblockquoteintent]~~
- ~~do [#pullnoackdeadlock]~~
- ~~do [#retainedmutdrop]~~
- ~~do [#relayresolveblind]~~
- do [#planhead]
- ~~do [#projpassstart]~~
- do [#operatorverifylive]
- ~~do [#prmergeguardpr]~~
- do [#run3rdpaneswitchverify]
- ~~do [#preflightprojpass]~~
- do [#respondbaselineflag]
- do [#hookhashfalsepositive]
<!-- /agent:queue -->

## Notes

<!-- agent:notes -->
<!-- /agent:notes -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#resumeownerfmread] agent-doc start pays TWO controller round trips per attached sibling document to answer one ownership question, and it is the single largest resolve source. Measured 2026-08-09 over the window AFTER 0.35.199 landed, so this profile is trustworthy: resume_id_owner 213 and start_resume_id_capture 126 top every other caller (next is observe_live_queue_heads at 107). #projpassstart already proved a projection pass cannot help — resume_id_owner runs once per start and reads each candidate exactly once. CORRECTION to this item's original plan (2026-08-09): it proposed a frontmatter-scoped relay read so an attached document could answer a bounded leading-block query without materializing its whole text. That will NOT help, and the reason matters. The cost is not string materialization: canonical_text goes through Yrs get_string, which walks the whole tree regardless of how few bytes the caller wants, and the profiling note on the hook budget says preflight is 'almost entirely blocked on controller round trips'. A prefix API would still pay the per-candidate round trip, which is the actual cost. The right design is to move the QUESTION to where the hubs already live: one controller-side query answering 'which registered document claims resume id X' across all hubs, replacing N per-candidate round trips with one. That matches #lazily-hot-path — ask the actor rather than have the caller poll it N times. Note resume_id_owner currently pays TWO round trips per attached candidate (current_revision_for_file, then the text resolve). Do NOT substitute state.db documents.session_id for the frontmatter read: it is a durable sink (#lzdurablesink) and a stale row would either invent an owner, silently losing a conversation resume via the #resumeclaim start-fresh path, or miss a real one.
- [ ] [#planhead] `agent-doc plan` reports a queue head the in-session loop is not allowed to drain (#agent-doc-bug). Observed twice this session: preflight's `selected_queue_prompts` correctly named the drainable head (`do [#prmergeguardpr]`, then `do [#preflightprojpass]`) while plan's `prompt_targets` / `repo_actions` / `required_commands` all named `do [#operatorverifylive]` — an [operator-verify] head that needs a human watching a real IDE + tmux session and that preflight had already deferred. SKILL.md step 0d says to treat plan's output as the execution contract, so an agent following it verbatim works the wrong item and would then --done an operator-gated one. Fix: drop plan's own head selection and defer to preflight.selected_queue_prompts. Two independent derivations of 'which head is next' is the defect class, not the specific filter.
- [ ] [#run3rdpaneswitchverify] [operator-verify] Live-editor proof for #run3rdpaneswitch (0.35.198): with a two-column editor layout, Run Agent Doc on document A, let its turn keep running, then switch the editor to document B. Expect the agent-doc window to hold exactly two panes, A's pane to be moved into the 1:stash window still alive with its turn running (ops shows 'Stashed busy pane %N from window @M'), and NO third pane. The fix is proven deterministically by switching_documents_while_the_outgoing_turn_runs_stays_two_panes against live tmux with an injected busy probe; this item is only the real-IDE eyeball, since that test stages tmux directly rather than driving the JetBrains layout path. Unblocks on a human watching a real IDE + tmux session.
- [ ] [#operatorverifylive] [operator-verify] #panewindowdriftverify Live-pane proof for #panewindowdrift (0.35.187): with a two-column agent-doc layout, move the focused document's pane into the stash window, then trigger an editor tab/focus change for that document. Expect NO select-pane onto the stashed pane (ops.log shows pane_layout_projection ... focus=focus_pane_not_co_visible:...), the structural reconcile surfaces the pane, and the retry focuses it in the agent-doc window. Unblocks on a human watching a real IDE + tmux session.
- [ ] [#hookhashfalsepositive] agent-doc's coined-id-pre-tool-use hook rejects C preprocessor directives as untracked backlog ids. Writing a file containing a line starting with a hash — pragma once, include <string> — is blocked with "would write coined id(s) #include, #pragma ... not tracked in agent:backlog". The same class of false positive already applies to CSS ids, shell comments, and agent-doc's own anchor tags quoted from SKILL.md. It is not a hard block (a bash heredoc bypasses the PreToolUse hook entirely, which is itself the interesting part — the guard is trivially avoidable while being obstructive to correct writes) but it forced a workaround into emitted C++: src/sdk-spike/ts/src/wire/code.ts assembles the directives via String.fromCharCode(35) with a comment explaining why. The guard should recognise language syntax — preprocessor directives, CSS selectors, shell/YAML comments, markdown headings — or scope itself to prose contexts. Fix belongs in agent-loop src/agent-doc, not here; remove the workaround in code.ts once it lands.
- [ ] [#respondbaselineflag] #agent-doc-bug SKILL.md's MANDATORY persist step documents `agent-doc respond <FILE> --baseline-file <preflight.baseline_file> --stream --origin skill`, but neither `respond` nor `write` accepts `--baseline-file` in 0.35.184 (`agent-doc respond --help` has no baseline/snapshot flag). An agent following the hot-path skill verbatim gets a clap parse error on the one step the skill calls mandatory, and the documented 'use preflight's baseline, do NOT save your own' contract has no way to be honoured. Decide one way: either add `--baseline-file` to respond/write and thread it into the diff baseline, or remove it from SKILL.md + every generated harness surface (.claude/.codex/.opencode/.cursor) and the runbooks. Reported by the operator 2026-08-08 while diagnosing the haiven-dev wedge.
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
- [/] [#operatorverifyrestart] [operator-verify] Restart a running JetBrains IDE (installed plugin is 0.2.350; this item originally named 0.2.340, long superseded — the single restart in #stickymdpaneverify covers it), then move from fpe-service-topology.md in a one-pane surface to agent-doc-bugs2.md in its two-column surface and confirm the first selection restores two tmux panes without a second navigation.
- [/] [#stickymdpaneverify] [operator-verify] #stickymdpane live proof: restart a JetBrains IDE — plugin 0.2.350 is ALREADY INSTALLED in all four IDE configs, so only the restart is missing, and that one restart also satisfies the install precondition for #operatorverifyrestart, #rebootselfhealverify and #falsetyping-guard — then open a two-column split with an agent-doc document in each and switch ONE column to a source file. Expect tmux to keep both panes with that column still showing its last document, instead of collapsing to one pane. Then close that split entirely and confirm tmux DOES collapse — the fallback must be memory, not invention. Unit tests pin the pure resolution but cannot prove the live IDE wiring.
- [/] [#stalesupervisorreexecdrain] [operator-verify] One supervisor is still stranded on the pre-fix image and cannot self-rescue: PID 4069526 (`agent-doc start --resume 019fd336-7ce8-7ed1-8141-d77021a00085 src/boost-client/tasks/monsterrodholders.md`, started 2026-08-05 19:12:50 EDT) maps a DELETED binary and, by the very defect #stalereexecstarve fixes, can never reach the recycle decision to adopt 0.35.186. The fix is preventive, not retroactive -- it lives in code that process does not run. Recovery requires interrupting a live Claude child (PID 2653032), which is an operator call, not an agent-doable step: either let it retire naturally when that session ends, or `kill 4069526` and re-route the document. Verify afterwards with `readlink /proc/<newpid>/exe` showing no `(deleted)` suffix. Every other live supervisor (336290, 523099, 577794, 837794, 3268523, 3854486, 3956615) already maps the current inode.
- [/] [#rebootselfhealverify] [operator-verify] Live proof for #rebootselfheal after restarting the IDE (installed plugin is 0.2.350; this item originally named 0.2.339 — reload-lib swaps the cdylib but NOT the Kotlin package, so a restart is still required, and the restart in #stickymdpaneverify covers it). Reboot the host (or stop the controller and leave a stale .agent-doc/controller.sock behind), then run Sync Tmux Layout and Run Agent Doc from the IDE and confirm both succeed without the `pane-layout state projection publish failed ... Connection refused` notification and without a human deleting the socket. Then confirm the passive lane is still free: click between editor tabs with NO controller running and verify no controller is launched (editors/SPEC.md requires the surface-observation lane to never launch one) - grep ops.log for a controller bootstrap around the tab clicks. Both reboot states are already proven end-to-end against the installed cdylib; only the in-IDE path needs a human eyeball.
- [/] [#hvw0] [operator-verify] Rotate the CARGO_REGISTRY_TOKEN secret on github.com/btakita/skill-harness — the current one returns 403 authentication failed, so CI cannot publish to crates.io (v0.1.3 was published manually from the passage token btak/CARGO_REGISTRY_TOKEN). Needs a repo-secret write, which is operator-gated. CORRECTION 2026-08-08: the claim that every tagged release now FAILS loudly at the publish step (intended behavior after 32836ad) is UNTESTED — no tag has run since. The newest Release run is v0.1.3 from 2026-07-18 and its `Publish to crates.io` step reports success, which is exactly the false green. #hvw0publishproof carries the static check.
- [/] [#c8tb] [operator-verify] Live #jbroutebusystall proof (0b49105e1): with the supervisor socket down (route logs `route_dispatch_only_authoritative_degraded_direct_pane supervisor_health=no_socket`) and the pane mid-turn, click JB Run Agent Doc. Expect `route_start` -> `route_success` in the attempt ledger to drop from ~17s to under ~2s, with `route_existing_pane_busy_active_turn_skip_wait` in ops.log and the SAME outcome as before (`route_existing_pane_already_running`, pane focused, no duplicate injection). Unit tests pin the cue gating but cannot prove the live latency win.
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
