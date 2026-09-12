---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: 3d56a2a6-95ce-42c4-b71c-66c102e54fb4
  codex: 01a08ca6-2d17-7822-948a-aaff42a157ac
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#actionable-review': Add actionable review items into backlog + queue
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: start
---

## Status

<!-- agent:status patch=replace -->

Editor-route layout supersede, controller bind-before-serve, the JB accept-leg bound, and the queued-clear mislabel are fixed and pushed (5577cfeeb, b0e9530a6, 232c6c103); make check 9316 passed. #fixrunlazily is gated on one IDEA restart to load plugin 0.2.380. Open: #tmuxautosyncwedge, #cargotokenrevoke.
<!-- /agent:status --> 

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20260912-032834.md`*

Compacted content:
- Archived 38 response topic(s): #actionable-review; #restartideaplugin restarted, do #pypiquotafix, do #cargotokenrevoke; #deferredmutdrop, #pypicadence, GH #52 + release + publish; 35 more
- Prior summary/context: prior compacted content: Archived 20 response topic(s): Compact commits, editor routes, and controller timeout; GH #4...

### Re: #timestampresponseheader — opus-5 · 2026-09-12T00:04-04:00

This heading is the feature. Format: model short name and response timestamp after the spaced em dash, joined to each other by a spaced middle dot. Local time with UTC offset (`YYYY-MM-DDTHH:MM±HH:MM`) — sortable and unambiguous, unlike the offset-less local time the commit subjects use. No resolvable clock emits the model alone, which is the pre-timestamp heading and stays valid.

**The middle dot is not cosmetic.** Four parsers read this heading and they do not agree on which dash bounds the attribution:

| parser | splits on |
|---|---|
| `response_prompt_target_from_re_heading` | first ` — ` |
| `queue_response::response_heading_topic` | first ` — ` |
| `response_replay::normalize_replay_topic` | first ` — ` |
| `transient_markers::strip_re_heading_attribution` | **last**, via `rfind(" — ")` |

The obvious format — a second em dash before the timestamp — leaves ` — opus-5` behind on the strip path. That strip backs `normalize_post_commit_re_heading_drift`, the comparison that lets an attributed heading match an unattributed one, so the residue would read as **permanent heading drift on every committed response**. One em dash preserves all four readings unchanged.

The binary has no production heading generator — it canonicalizes whatever the agent writes — so the deliverable is the contract plus proven tolerance. `agent_doc_turn::response_text` now owns the format in one place (`response_heading`, `response_heading_attribution`, `response_heading_model_and_timestamp`, `response_heading_timestamp_is_wellformed`, both separator constants), and `a_timestamped_heading_reads_the_same_through_every_parser` drives all four parsers off one timestamped heading — `agent-doc-topic` became a dev-dependency of `agent-doc-turn` so the compact summarizer is included rather than assumed.

Instruction surfaces updated at source (`SKILL.md`, `runbooks/respond.md`, and `runbooks/harness-invocation.md`'s own Response Header Attribution section — a fourth surface I initially missed) with every mirror regenerated, superproject and submodule-local. The submodule-local set needs `skill install --root .` (`#skillinstallstalemirror`); `audit-docs` is what caught that, not me.

Three existing tests pinned the old untimestamped example and were **raised rather than weakened** — `bundled_skill_contains_model_short_name_attribution_rule`, `codex_content_uses_plain_text_invocation`, `harness_invocation_runbook_content` now assert the timestamped example, the shape, and the one-em-dash invariant. The ~90 test *fixtures* using `### Re: topic — gpt-5` as sample content are deliberately untouched: that heading stays supported, pinned by `an_untimestamped_heading_still_round_trips`.

Two mutations, checked cold: swapping the middle dot for a second em dash — the naive design — reddens both the cross-parser test (on the strip assertion, exactly the drift this avoids) and the instruction guard. Transposing the offset sign and colon byte indices reddens `timestamp_shape_check_pins_each_separator_position`; that was a real slip I made and caught before testing.

`make check` green, `EXIT=0`, 9406 tests / 9406 passed. Committed `dec47694f`, pushed.

### Re: #idledispatchstack — opus-5 · 2026-09-12T00:04-04:00

> **Queue prompt:**
>
> do [#timestampresponseheader]
>
> do [#idledispatchstack]


Closing this out — the code landed last turn as `b666a8d71` (pushed, installed, verified in the binary); only the document closeout was owed, because the mandated post-fix `make install` stranded this document's Lazily replica and blocked every write. That is filed below as `#installstrandsreplica`.

**The item's diagnosis named the wrong mechanism.** It proposed `pane_composer_has_pending_trigger` as an idle-watch precondition. That predicate is scope-limited by `autotriggerscrollbackecho` to brand-new panes with empty scrollback — on a long-lived pane the trigger literal also sits in consumed transcript history, so reusing it in the idle watch would report "already pending" forever and defer the drain permanently. And the pane-level check it wants already exists on the go-mode drain path (`supervisor_pane_payload_observation` → `dispatch_payload_pending_in_current_input`).

The real defect, from `src/haiven-dev/.agent-doc/logs/ops.log`, all one `turn=cycle-1789167305907`:

| time | event |
|---|---|
| 02:08:33 | dispatch authorized, in-flight receipt opened |
| 02:08:35 | `route_dispatch_only_sent proof=transport_submitted` |
| 02:09:04 | `state=busy caller=supervisor reason=restart_continue_spawn` — unrelated |
| 02:09:06 | `dispatch_in_flight_released ... reason=actor_ready` — 33s in, premature |
| 02:09:07 | `state=busy caller=dispatch reason=auto_trigger_inject` — the turn starts |
| 02:09:12 | duplicate dispatch admitted; a second trigger stacks |

`mark_open_dispatches_consumed` set `dispatch_start_proven` on any `Ready`, on the premise in its own comment — "a transition to Ready means the turn finished". False for a submitted-but-unstarted trigger: admission runs first, so the actor projects `Ready` for the whole pre-turn window. A field named `dispatch_start_proven` was set without the start ever being proven. The `qflood` discriminator was *provenance*, and the log defeats it — the premature release came from the blessed controller boundary (`caller=supervisor reason=prompt_ready`).

Now the turn start is observed: a `Busy` with `caller=dispatch` promotes receipts to `running` (already inside the open-set predicate, so neither gate changes meaning), and `Ready` consumes only `running`, or a row past `DISPATCH_PRE_TURN_GRACE_SECS` (120s) so a lost trigger cannot wedge the coalescing gate. **The `caller` filter is load-bearing and I nearly shipped without it** — promoting on any `Busy` lets that 02:09:04 supervisor transition vouch for the receipt, leaving this exact incident unfixed.

`qflood_coalesces_in_flight_despite_ready_projection_drift_and_releases_on_ready` asserted that a controller `Ready` releases the marker, which the log disproves; extended rather than weakened, its tail now drives premature `Ready` → unrelated `Busy` → dispatch `Busy` → `Ready`. Six mutations checked cold, each reddening its own guard.

Two notes on this turn's own mechanics. The exchange was compacted at 23:28 and the new summary carries **no** `dynamic_context_ref` block — `#fixcompactexchange` working incidentally. And `make check` reported `EXIT=2` twice while its wrapper reported exit 0, the RTK exit-masking trap; both failures were real and are fixed.

CI: `fc0f992c6` (`#fixcompactexchange`) came back green (`34669245364`); `b666a8d71` is run `34670800452`, in progress when last observed; `dec47694f` is new.

### Re: preflight stall fixed; docs.md reverse-order lists traced to one cycle — opus-5 · 2026-09-12T00:54-04:00

**The stall that blocked this document is fixed and shipped as 0.35.368.** A JetBrains replica restart at 04:06:44 left client `2121428668057853` registered with a queued canonical projection receipt and then silent — no pull, no ACK, no projection — so `delivery_converged` stayed false for five minutes and every invocation was refused with `Lazily current authority remained delivery_pending` while your queue edits sat unread on disk. `#pullnoackdeadlock`'s escape valve only advances on a *pull*, so a replica that never pulls held the barrier forever. The barrier now bounds total silence too (`charge_barrier_wait_without_progress` + `MAX_BARRIER_WAITS_WITHOUT_PROGRESS`, charged from the expired delivery-convergence await); a replica that merely pulls without ACKing clears the new streak and stays under the 50-redelivery budget sized for it. Three regression tests, one mutation-checked. Authority and disk are now byte-identical at 36048 bytes with `delivery_converged=true`.

> **Queue prompt:** docs.md had reverse order ordered lists (2 then 1). Please fix agent-doc causes.

**Traced to one cycle, cause not yet proven — I did not guess at a fix.** Provenance is exact: `src/haiven-dev` commit `9b084e4` (2026-09-11 13:28:19), 91 seconds after `02d8cb7`, during a `queue: start` → `queue: go` drain write. That cycle appended two correct new response blocks **and** spliced **three copies** of an 8-line run into the middle of the earlier `### Re: #plcite` block. The run is the list lines of the *later* `### Re: PR 20 — ADR 0012 review addressed` block in **exact reverse order** (`2.`, `1.`, then the six bullets bottom-to-top), with the intervening prose (`**Verification:**`, `**Two things need you:**`) dropped. So: list lines only, reversed, anchored into the wrong block, three times.

What I ruled out, each by direct check rather than reading:

- `document_cell_merge` on that exact mixed shape (bullet run + prose + ordered list in one block) does **not** reverse — I wrote the test and it passes. Committed as `54296e903`; the two existing `#ocreverselist` tests each hold one list shape alone, so this shape had no coverage.
- `order_union` / `weave_order` preserve order: the `j = i-1` lookup always resolves, because every earlier theirs key is either in the spine or was just inserted.
- `component_scoped_region_restore` / `apply_region_restore` are correct — descending start offsets with `replace_range`, so each edit addresses pre-edit text.

The signature (keyed *list items* only, prose skipped, exact reversal, wrong anchor, ×3) points at a keyed-list re-emit inserting each item at a fixed index against a base it does not belong to. I could not localize it further: `src/haiven-dev/.agent-doc/logs/ops.log` no longer reaches back to 13:28 on 09-11, and I could not construct a live repro. Filing it at the top of the backlog with this forensic record rather than landing a speculative change to the merge path that guards operator text.

**Queue:** `queue_drainable_head_count` is 0 — no in-session-drainable head remains, so I am not looping. `#fixtmuxpane` also carries a semantic-completion warning: it may already be resolved by `#stashfocuswindow` in `agent-doc-bugs.done.md`.

### Re: hook silence on a mangled FILE argument, fixed — opus-5 · 2026-09-12T02:51-04:00

Your two deletions are read as closeout: `#fixtmuxpane` is gone from backlog (its queue entry was already struck), and the answered `docs.md` free-text prompt is gone from the queue. Nothing owed on either.

**One stray keystroke needs your editor, not mine.** The `## Pending / Not Built` heading currently reads `t## Pending / Not Built`, so it renders as a paragraph rather than a heading. It sits outside every managed component, so no `agent-doc write` flag owns it, and hand-patching disk under the attached editor is the one move that manufactures the divergence class this document tracks. One character, in your editor.

**The hook defect from the failed invocation is fixed, tested, and pushed (`aa695fd39`).**

`handle_user_prompt_submit` read the prompt with two different parsers. Session tracking binds its document through the permissive `resolve_agent_doc_path`; the tracking-failure guard asked the strict `invoked_document`, which refuses a second positional argument so `agent-doc compact exchange FILE` is left alone. That asymmetry is harmless until tracking *fails* — and then a prompt tracking had recognized, bound a document from, and errored on emitted **neither** stdout marker. That is the exact state `SKILL.md` defines as "the hook never ran", so the agent reports an undiagnosable harness-wiring defect instead of the path typo it actually is.

The doubled slash-command argument `/agent-doc <FILE>.md/agent-doc <FILE>.md` canonicalized `<FILE>.md/agent-doc`, failed with `Not a directory (os error 20)`, and exited 0 in silence.

Both tracking-failure guards — Claude and Codex — now consult `tracking_recognized_trigger`, the union of the two parsers. The union only ever *adds* a named refusal on a path that already failed and never suppresses one; trigger recognition for admission itself is untouched, so the strict parser still owns which prompts open a cycle.

The regression test drives the real hook over stdin rather than asserting on source text, because the whole defect was which parser one runtime branch consulted — the existing `test_preflight_hook_never_emits_silence_on_admission_failure` is a source-text check and passed throughout. Mutation-checked cold: restoring `invoked_document` at the two call sites reproduces the field symptom byte for byte (`stdout=""`, canonicalize error on stderr only).

`make check`: **9419 tests run, 9419 passed**, 259 skipped, `EXIT=0` — captured directly, not via the RTK-wrapped status. CI run `34677869227` was still in progress for an earlier commit when observed, so this is local evidence; `aa695fd39` has not had its own run yet.

Per `#installstrandsreplica` in this document's own backlog, the mandated post-fix `make install` goes **after** this closeout, not before it.

### Re: why the queue reports zero drainable heads — opus-5 · 2026-09-12T02:51-04:00

Worth stating plainly, because it is not a stall and not a bug: `queue_drainable_head_count: 0` alongside `queue_active: true` and seventeen live heads is **activation scope**.

`active_queue` calls `active_queue_for_supervisor_start(content, false)`, so `explicit_start` is hard-wired off for the in-session loop. This document carries `queue: start` in frontmatter and `start` on the queue marker, and no `go` anywhere. The gate `if !explicit_go && !explicit_start { return None }` therefore returns `None`, and `drainable_head_count` short-circuits to `0` before it ever looks at an item. `DrainScope::Supervisor` passes `true` for that same flag, which is the whole design: **`start` authorizes the supervisor drain; `go` additionally authorizes the in-session loop.** Your queue is not stranded — the supervisor idle-watch owns it. Set `queue: go` if you want this session draining it instead.

**But preflight should have told me that, and it did not.** It emitted `queue_continuation_required: false` with `queue_continuation_guidance` null and no `ui_outcome`, routing a full supervisor-scoped queue through the same silent branch a genuinely drained queue uses. `effective_continuation_output_with_preemption` deliberately stays silent when `raw_required` is false — "there is no reason to give" — and that premise is false here: there are seventeen heads and the reason is scope. This is the same failure mode `#queueblockquoteintent` already fixed one branch of, and I only recovered the cause by reading `active_queue_for_supervisor_start`. Filed as `#queuestartscopesilent` at the top of the backlog with the insertion points.

### Re: #dispatchturnstartreceipt — opus-5 · 2026-09-12T03:10-04:00

Done — audited, and the audit found a gap, so the durable fix landed with it (`8497ab76c`, pushed, installed).

**The audit.** Every path that transitions the actor to `Busy`, and which of them can start a dispatched turn:

| site | caller | reason | starts a dispatched turn |
|---|---|---|---|
| `start_runtime_io::auto_trigger_inject_command` | `dispatch` | `auto_trigger_inject` | yes |
| `supervisor_io::ipc::mark_supervisor_inject_dispatched` | `dispatch` | `ipc_inject` | yes |
| `auto_trigger_clear_command` / `IpcMethod::Clear` | `operator` | `auto_trigger_clear`, `ipc_clear` | no — submits `/clear` |
| `supervisor_io.rs` permission-prompt resolve | `supervisor` | `harness_permission_prompt_resolved` | no |
| `run.rs` restart | `supervisor` | restart reasons | no |

So the `caller` filter is exact wherever it fires — both real dispatch-start paths do carry `caller=dispatch`, and no side-effect path impersonates one. The item's stated worry was the wrong shape.

**The real gap is a path with no transition at all.** `agent-doc-route-io` contains no `transition_actor_state` call anywhere — `dispatch.rs` and `dispatch_only.rs` open a controller receipt through `handle_dispatch`, then submit straight to the pane. Those are the Claude Code / OpenCode dispatch-only routes. Their receipts were therefore **never** promoted, and every one of them waited out the full `DISPATCH_PRE_TURN_GRACE_SECS` (120s) before `mark_open_dispatches_consumed` would release it. Bounded and non-wedging exactly as the item predicted, but the stall is unconditional on those routes, not a corner case — and invisible in the original `fpe.md` repro, whose log carried only supervisor-driven claude turns. That is why the harness suspicion pointed the right way for the wrong reason: what varies is the *route*, not the harness.

**The fix is the fact the item asked for, and it was already in the system.** `#preflightinbinary` runs preflight in the binary for the arriving prompt, so a `preflight_started` cycle **is** the dispatched turn starting — on every harness and every route, with no new controller request shape and no receipt identity to thread. `mark_dispatch_turn_started_for_file` now runs beside the cycle-open in `preflight-command-io`.

It is also a strictly *better* observation point than the lifecycle edge. `auto_trigger_inject_command` transitions `Busy` **before** its own pane write, so the existing promotion fires at submit intent; preflight runs inside the turn. The two are additive — the `caller` promotion stays — idempotent (a re-entrant preflight, or one with no dispatch in flight, promotes zero rows), and non-fatal, since a promotion failure must not refuse a turn that is already admitted and the grace still bounds the receipt.

**What the test asserts is the release gate, not the row.** An unpromoted receipt must still NOT be released inside the grace — that is `#idledispatchstack`'s protection and the easy way to "fix" this would be to weaken it — while a preflight-promoted one is released by the next `Ready` with no 120s wait. The structural wiring guard is extended beside the existing Busy/Ready pair, and now also fails if `agent-doc-route-io` ever starts transitioning the actor, since that would invalidate the audit above. Two mutations checked cold: neutering the promotion reddens the behavioural test; deleting the preflight call site reddens the wiring guard.

Spec updated in `specs/08b-single-process-control-plane.md` — turn start now has two named observations with preflight authoritative for routes the lifecycle edge cannot see.

`make check`: **9420 tests run, 9420 passed**, `EXIT=0`. CI runs `34680037781` (this commit) and `34679131776` (the prior hook fix) were both in progress when observed; this is local evidence.

`#dispatchreceiptperturn` stays open and is unaffected — it is about which receipt a promotion settles when two are legitimately open, and preflight promotes per document exactly as the lifecycle edge did.

### Re: do [#installstrandsreplica] — opus-5 · 2026-09-12T14:02-04:00

> **Queue prompt:**
>
> 📌 do [#installstrandsreplica]


Fixed and pushed as `fbdbdf0c3` (0.35.371). The install that carries it runs **after** this closeout, per the trap this item documents.

**The refusal was never the bug.** `realtime_doc_resolve_disk_read_refused ... invariant=attached_editor_never_descends_to_disk` is correct and is unchanged. What was missing sits one layer up: the `reload_library` fan-out never asked whether it was safe to retire the generation. It does now.

`native_reload_admission` lands in `agent-doc-supervisor::lifecycle` beside `supervisor_recycle_action` and `supervisor_restart_admission`, reading the same open-cycle fact, and a test pins all three to one answer on the same facts. That pair already had to be reconciled once (`#haivendupsession`) after they disagreed about an open cycle and put two children on one pane; the reload fan-out is the third path that retires live state mid-cycle, and it was the one with no gate at all.

An endpoint holding an attached document with an open cycle keeps its working generation, logs `reload_library_deferred_cycle_open`, and records the deferred version under `.agent-doc/pending-lib-reload`. The owning supervisor's idle watch publishes it at the first tick after the cycle closes — the same boundary the recycle gate waits for — so the editor reaches the new build with no operator step and no `admin reload-lib`. The marker self-clears on any fan-out with nothing left to defer, so a project whose endpoints have gone away does not retry forever.

Two facts deliberately do **not** block. A *stalled* open cycle publishes: an abandoned older turn must not freeze every editor in the project on the build it happened to load, and it is the same `STALLED_CYCLE_RESOLVE_SECS` deadline the recycle path already force-closes past. An unreadable cycle projection *does* block — the safe outcome there is an editor keeping a generation that demonstrably works. Deferring costs it the previous build for one cycle; stranding it costs the operator the whole closeout.

`lib-install` and `admin reload-lib` (text and `--json`) both report the deferred count, so an install that held a reload back says so instead of reporting a quiet `delivered 1/3`.

**The test drives real cycle state, not a synthetic flag.** `start_preflight` must defer and `mark_committed` must release, measured on both sides of closeout in a temp project. Mutation-checked on a cold build: forcing `document_cycle_blocks_native_reload` to `false` reddens the deferral half by name (`an attached document mid-cycle must defer the native generation handoff`), and restores green. `make check` EXIT=0, 9427 tests / 9427 passed. CI run 34709997891 is still pending at the time of writing — this turn stands on local evidence.

**What this does not cover, stated plainly.** The JetBrains-side re-registration that fails to converge for a mid-cycle document is still latent — `restartAfterNativeReload` → `forceRefreshOpenDocumentReplicas` re-registers every open `.md` tab, and it demonstrably did not bring this document's replica back. It is now only reachable when a document is attached *without* an open cycle, where there is no closeout to block and the terminal rebuild path recovers — which is exactly why `tasks/software/existence-lang.md` resolved normally throughout the observed episode. Filed as `#jbreloadreregconverge` at the top of the backlog rather than gated: it is real follow-up work, not a blocked one, and it needs a plugin build plus an IDE restart to verify, which `#jbplugin382restart` already owes.

One note on this tree: `src/session_actor_cmd.rs`, `editors/jetbrains/gradle.properties`, `TurnStateBannerRefresher.kt` and two untracked `InputRequiredNotifications*.kt` are another session's uncommitted work. `make check` ran with them present and the commit used `--only` over my path set, so none of them were carried in — but that also means the green suite included their source, and HEAD's own state is theirs to prove.

### Re: do [#dispatchreceiptperturn] — opus-5 · 2026-09-12T14:42-04:00

> **Queue prompt:**
>
> 📌 do [#dispatchreceiptperturn]


Done — `c13cc7505` (0.35.372), pushed and installed. And a second, unwelcome finding I have to report with it.

**The receipt fix.** Both `mark_open_dispatches_turn_started` and `mark_open_dispatches_consumed` operated on every open row for the document, so one turn promoted and released both receipts when an operator reopen had legitimately bypassed coalescing. The reopen's receipt then stopped blocking a duplicate into that same composer — the `#idledispatchstack` stacking shape the receipt exists to prevent.

A turn's receipt is the **oldest open** one, because a stacked composer runs its entries in submission order; one `Ready` ends one turn and settles one `running` row. The pre-turn grace stays a *sweep* rather than a settlement — a trigger that never started is lost, not pending, and every such row goes together so none can wedge the per-generation coalescing gate.

The `NOT EXISTS` guard on promotion is the load-bearing part. **Two** edges promote for the same turn — the `caller=dispatch` `Busy` transition and `mark_dispatch_turn_started_for_file` from preflight (`#dispatchturnstartreceipt`, last week). Under promote-every-row the second was a natural no-op; narrowed to one row it would instead promote the *next* receipt and claim a turn that has not started. So the idempotence had to become deliberate: a document's actor runs one turn at a time.

The backlog item deferred this because "the lifecycle request carries no receipt identity, so threading one through is a controller request-shape change." It does not need one — oldest-open plus the one-turn-at-a-time guard identifies the receipt without touching the request shape.

`qflood_coalesces_in_flight_despite_ready_projection_drift_and_releases_on_ready` **asserted the defect**: it opens two receipts and required one turn to clear both. Its tail now asserts the correct settlement instead. Mutation-checked both halves on cold builds — M1 promote-every-row reddens the two-open-receipt and two-edge tests by name; M2 release-all-running reddens a legacy two-`running`-row ledger test. `make check` EXIT=0, 9432/9432.

### `#installstrandsreplica` does not fire on the real path — I am reopening it

I installed mid-cycle to *prove* last turn's gate, and it reported `0 deferred mid-cycle` while this document held open `cycle-1789236397866`. It shipped inert. Two causes, one fixed and one not.

**Fixed (`fa203f003`, 0.35.373):** the gate read its at-risk documents from `reliable_sync_status.registrations` alone, and that record can be empty while the editor is alive on its PID-scoped socket — the fan-out's own socket-discovery fallback exists for exactly that state, and its comment says so. With no candidates there is nothing to find an open cycle on. `plane_open_paths` is not a second opinion; it resolves from the same `registrations`. Candidates now union in `open_supervisor_documents`, which walks live `agent-doc start --route-owned` processes and never consults the editor record. An empty candidate set now logs `reload_library_no_document_candidates` instead of being silent.

**Not fixed — and it is structural.** After 0.35.373 the install *still* reported `0 deferred`, and **neither** new diagnostic appeared in this project's ops log, so the agent-loop leg never reached the endpoint loop at all. Meanwhile the IDE demonstrably reloaded (`crdt_replica_deregister` at 18:26:01). One cdylib serves a whole editor process, and pid 2238596 is registered across several of the nine projects — so a reload delivered through another project's endpoint retires the generation holding *this* project's replicas, and this project's open cycle never gets a vote. The gate is per-project; the library is per-process. Deferring per-process instead is not a free tightening either: with nine projects in this workspace something is almost always mid-cycle, so a naive process-wide defer would freeze the cdylib permanently.

I am not going to claim that as done. Filed as `#reloadgateperprocess` at the top of the backlog with the measurements, superseding the closed `#installstrandsreplica`. The honest status of last turn's work: the refusal reasoning and the deferral machinery are right and tested, and they do not yet reach the path that actually strands a replica.

Exchange is at 168 lines / 7 response sections — noting it rather than stopping the queue. CI runs 34711887506 and 34711105224 are both still in progress; this turn stands on local evidence.
<!-- agent:boundary:330692a0:agent-doc-bugs -->
<!-- /agent:exchange -->

<!--
-->

t## Pending / Not Built

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority start -->
- ~~📌 do [#installstrandsreplica]~~
- ~~📌 do [#dispatchreceiptperturn]~~
- 📌 do [#ctxrefsynthsource]
- 📌 do [#ctxrefdocnoise]
- 📌 do [#cargotokenwire]
- 📌 do [#jbplugin382restart]
- 📌 do [#isolationrelocate]
- 📌 do [#clearunobservedrecycle]
- 📌 do [#clearsubmitpanesettle]
- 📌 do [#queuegorevokedonfreetext]
- 📌 do [#layoutbindstale]
- 📌 do [#jbfocusdocroute]
- 📌 do [#compactackflake]
- 📌 do [#retainedresumewakeflake]
- 📌 do [#cargotokenrevoke]
- 📌 How can we make the pypi package slim? Can we make it download & extract the tarballs from github instead of sending the entire binary into PYPI? When we do the release, please aggresively delete old releases.
<!-- /agent:queue -->


<!-- agent:backlog priority -->
- [ ] [#reloadgateperprocess] [P1] The #installstrandsreplica mid-cycle reload gate does not fire on the real path; supersedes that closed item. Measured 2026-09-12 18:25 and 18:42 on this repo with tasks/agent-doc/agent-doc-bugs.md holding open cycle-1789236397866 in state.db: `lib-install` reported `0 deferred mid-cycle` BOTH times, and after 0.35.373 NEITHER new diagnostic (`reload_library_deferred_cycle_open`, `reload_library_no_document_candidates`) appeared in this project's ops.log — so `reload_library_for_project` never reached its endpoint loop for agent-loop, while the IDE demonstrably reloaded (`crdt_replica_deregister` 18:26:01). Two things to settle. (1) Why the agent-loop leg bails: either the project is absent from `controller_project_roots(std::process::id())` or `reliable_sync_status` fails for it (`failed: 3` with only 3 endpoints is consistent with one status-read failure); `agent-doc admin list` answers for this project, so the controller itself is reachable. Both bail paths are silent — add a named log line before fixing. (2) The structural half: one cdylib serves a whole editor process and IDE pid 2238596 is registered across several of the nine projects, so a reload delivered through ANY project's endpoint retires the generation holding every replica in that process. A per-project gate can never see that; the gate has to be process-scoped. But a naive process-wide defer freezes the cdylib — in this workspace something is almost always mid-cycle — so it needs per-(editor pid, document) attribution that survives an empty registration record, which is the same blind spot 0.35.373 worked around for the candidate set. 0.35.371's refusal reasoning (agent-doc-supervisor::lifecycle::native_reload_admission, the three-gate agreement test) and the pending-marker/idle-watch re-arm are correct and tested; only the reach is wrong. Related: [#jbreloadreregconverge] is the plugin-side half.
- [ ] [#jbreloadreregconverge] [P2] The JetBrains native-generation handoff re-registers every open .md tab (`NativeReloadCoordinator.requestReload` -> `CrdtReplicaManager.restartAfterNativeReload` -> `forceRefreshOpenDocumentReplicas`, `ensureOpenDocumentReplica(await = false, forceRefresh = true)`) and that re-registration does NOT converge for a document that was mid-cycle when the reload ran — the residue of #installstrandsreplica, which fixed the trap in the binary by deferring the reload but left the plugin-side non-convergence latent. Evidence 2026-09-11 23:36: the IDE had both libagent_doc-1789182099748.so and libagent_doc-1789184163251.so mapped, tasks/agent-doc/agent-doc-bugs.md stayed in editor_attached_model_missing across admin reload-lib (1/3 endpoints, no clear) and cleared only on an operator tab re-attach, while a sibling document resolved normally throughout. Suspects, in order: `quiesceAllForNativeReload` captures `captureResumeState()` into `nativeReloadResumeStates` ONLY when every worker terminated within NATIVE_RELOAD_WORKER_TIMEOUT_MS — a busy document lane (exactly what an open cycle produces) misses the capture, and `managers.forEach { dispose() }` then drops the replica either way; and `forceRefreshOpenDocumentReplicas` is fire-and-forget (`await = false`) with no per-document proof that the new generation actually holds the replica. Fix direction: carry each open document's encoded state into the replacement generation before the old one is dropped, and have the handoff assert per-document re-registration instead of firing and forgetting. Needs a plugin version bump, build, and an IDE restart to verify — #jbplugin382restart already owes a restart.
- [ ] [#queuestartscopesilent] [P2] A supervisor-scoped queue reports zero drainable heads with NO reason, through the same silent branch a genuinely drained queue uses. `active_queue` (agent-doc-queue/src/queue_continuation.rs:637) calls `active_queue_for_supervisor_start(content, false)`, so `explicit_start` is hard-wired off for DrainScope::InSessionLoop and the `if !explicit_go && !explicit_start { return None }` gate short-circuits `drainable_head_count` to 0 BEFORE looking at any item. That is correct design (`start` = supervisor drain, `go` = in-session loop too), but preflight then emits `queue_active: true` + `queue_drainable_head_count: 0` + `queue_continuation_required: false` + `queue_continuation_guidance: null` + no `ui_outcome`, which is indistinguishable from a drained queue. Observed 2026-09-12 on tasks/agent-doc/agent-doc-bugs.md with SEVENTEEN live heads; the cause was recoverable only by reading the source. Same failure mode as #queueblockquoteintent, which already fixed the exchange-preemption branch on the premise that "a stop with heads remaining must name its reason" — `effective_continuation_output_with_preemption` (queue_continuation.rs:114) still returns silence whenever `raw_required` is false, on the comment "genuinely nothing to drain stays silent - there is no reason to give", and that premise does not hold for the scope case. FIX: add `supervisor_scoped_activation_guidance(content)` in agent-doc-queue/src/queue_continuation.rs returning Some(reason) when `active_queue(content).is_none()` AND `active_queue_for_supervisor_start(content, true).is_some()` AND that activation has >0 prompts; thread it into the guidance at the single production caller, agent-doc-preflight-command-io/src/run.rs:1867. Guidance must say the heads are supervisor-owned and NOT stalled, and that `queue: go` hands the drain to the in-session loop. Extend the `a_preempted_drain_names_its_reason_instead_of_going_silent` test with the scope case.
- [ ] [#ocreverselistsplice] 2026-09-12 [P1] Three copies of an 8-line run spliced into the WRONG response block, list lines only, exact reverse order, prose dropped. Provenance pinned: src/haiven-dev commit 9b084e4 (2026-09-11 13:28:19), 91s after 02d8cb7, during a queue:start->queue:go drain write; same cycle also appended two CORRECT new blocks. Source run = the list lines of the LATER '### Re: PR 20 - ADR 0012 review addressed' block. RULED OUT (checked, do not re-check): document_cell_merge on the mixed bullets/prose/ordered-list shape (test exchange_response_mixing_bullets_prose_and_ordered_list_keeps_authored_order, commit 54296e903, passes); order_union/weave_order (j=i-1 lookup always resolves); component_scoped_region_restore/apply_region_restore (descending offsets + replace_range). Signature points at a keyed-list re-emit inserting each item at a FIXED index against a base it does not belong to. haiven-dev ops.log no longer reaches 13:28 on 09-11; needs a constructed repro.
- [ ] [#ctxrefsynthsource] [P2] `collect_candidates` (agent-doc-dynamic-context-io/src/lib.rs:716-721) falls back to `source_uri = report.target` for any candidate value with no `file`/`target` key, so the synthesized `next_context` and `agent_doc_queue` chunks are RECORDED against the session document and emit `expand="tsift --envelope source-read <the document>"` — a command that returns the document instead of the chunk. That mislabel is why #fixcompactexchange had to DROP document-sourced handles rather than keep them: the honest fix is to give a synthesized chunk its own source identity (e.g. `agent-doc://cycle/next-context`) plus an expansion command that actually resolves it, or mark it unexpandable. NOTE before landing: `source_uri` participates in the `already_injected` dedup key (agent-doc-sqlite/src/context_injection_ledger.rs), so changing it re-expands those chunks once per document — check that path first. Affects `as_prompt_section` and `as_orchestration_child_section` too, not just compaction.
- [ ] [#ctxrefdocnoise] [P3] DECISION, do not change before deciding. After the #fixcompactexchange self-reference filter, fpe.md's surviving handles are `AGENTS.md`, `.agent/runbooks/code-navigation.md`, `.gitmodules` and `.gitignore` — instruction-file and repo-metadata noise, not durable work context. Separately: inside a checkout that has `.agent-doc/state.db` the block is strictly REDUNDANT, because `durable_context_reference_for_document` renders it FROM that database; its only unique value is a document that travels without the state db, which is the transfer/extract path (src/extract.rs:101,731) and is already covered there. Options: (a) narrow document-facing pack selection to source/work files, (b) stop writing the block on compact and keep it only on transfer/extract, (c) keep as-is. 888037e9b added it deliberately in July and `exchange_compact_preserves_durable_context_handles_without_payload` pins it, so this is a design call, not a defect.
- [ ] [#cargotokenwire] [P2][security] Nine repo secrets still hold a PRE-rotation crates.io token: btakita/{agent-kit,agent-memories,agent-rules,agent-runbooks,instruction-files,skill-harness,tsift,webmaster} (Mar-May 2026) and existence-lang/existence (2026-08-28 — the one [[reference_cargo_publish_token_passage]] records as expired). `signal-space-rs` and `tmux-router` reference `secrets.CARGO_REGISTRY_TOKEN` in a workflow but have NONE set, so their publish steps would fail on a missing secret, not a stale one. The new token is in `passage btak/CARGO_REGISTRY_TOKEN` (verified by SHA-256 against the live page, no cleartext printed): scopes publish-new/publish-update/yank, 12 months, never used. GATED ON ONE OPERATOR DECISION, because its cleartext was printed into the 2026-09-11 session output while reading the tokens page: (a) Regenerate on crates.io first, re-store in passage, THEN wire all nine; or (b) wire the current token as-is. Do not wire before that decision — pushing a value that is about to be regenerated writes a dead secret to nine remotes. Either way trim the livestream VOD segment.
```
<dynamic_context_ref contract="agent-doc-dynamic-context-manifest-v1" session="b69e5aeb-eab5-4a25-bc03-a9ddb69f84fc" cycle="cycle-1789167305907" fingerprint="42b6c436b2fdbaa2b9b0077601e304dc5ff8c7a4b05bac21fca356ec79350a4e" token_count="748" modes="expanded:6">
<context_ref handle="tsift://tspack-7fd447e44bac/section-1-defbf5a0bc5b" hash="defbf5a0bc5bc203687c3fd33086227353c577f4d1550e756ce61e6015e4d73f" source="/home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/fpe.md" mode="expanded" expand="tsift --envelope source-read /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/fpe.md --budget normal" />
<context_ref handle="tsift://tspack-7fd447e44bac/xwin-117226c52f-fd80b9845d9a" hash="fd80b9845d9a79d2d59c780edaa872b75931b609edd74f9afb89f5cca235f4ec" source=".agent/runbooks/code-navigation.md" mode="expanded" expand="tsift --envelope source-read .agent/runbooks/code-navigation.md --start 1 --lines 96 --budget normal" />
<context_ref handle="tsift://tspack-7fd447e44bac/xwin-2490f05410-a61440a42505" hash="a61440a425051514bd6f149a83565d8974c0af83a2d4d95e40c42486089a07de" source="AGENTS.md" mode="expanded" expand="tsift --envelope source-read AGENTS.md --start 1 --lines 96 --budget normal" />
<context_ref handle="tsift://tspack-7fd447e44bac/xwin-3871b1fc98-ed54281eb37b" hash="ed54281eb37b047872472713efd5d011f8e32bd2ae73bcb9741ba850b66b5eea" source=".gitmodules" mode="expanded" expand="tsift --envelope source-read .gitmodules --start 1 --lines 96 --budget normal" />
<context_ref handle="tsift://tspack-7fd447e44bac/xwin-56e927068a-9edbb9f168ef" hash="9edbb9f168ef86b73318e3fed431db6a1499d4cf935ff88258920c5280bfdb10" source=".gitignore" mode="expanded" expand="tsift --envelope source-read .gitignore --start 1 --lines 96 --budget normal" />
<context_ref handle="tsift://tspack-7fd447e44bac/xwrk-66eb936119-217282636f1b" hash="217282636f1b0ab83d4b4ce18436bbc86656805b3b92981b1459de789f74758e" source="tasks/fpe.md" mode="expanded" expand="tsift --envelope source-read tasks/fpe.md --budget normal" />
</dynamic_context_ref>
```
- [ ] [#jbplugin382restart] [operator-verify] [P2] Restart IntelliJ to load JetBrains plugin 0.2.382. `make install` at 19:43 (binary 0.35.367) staged `~/.local/share/JetBrains/IntelliJIdea2026.1/agent-doc-jetbrains/lib/agent-doc-jetbrains-0.2.382.jar`; the live IDE is still on 0.2.379, which is what preflight's standing `stale_plugin` warning reports. Same shape as the closed `#restartideaplugin` — it re-broke the plugin floor for `#tmuxfocussyncverify`, `#activateinstalledjetbrai`, and `#harnessswitchverify`, and for the `#jbfocusdocroute` half already shipped in 0.2.381.
- [ ] [#isolationrelocate] [P2] `#reconcilesyntheticbase` proved a member insertion CAN materialize outside every component — the controller removes a component while the operator is typing inside it, every character around the caret is tombstoned, and the line lands in the framing run between two components. The narrow repair correctly REFUSES there: restoring that framing run would delete the operator's line, and no other replica has seen it yet, so the raw union is published with the text misplaced but present and `component_isolation_refused_lossy` logged. Nothing then moves it back — the operator repositions it by hand. The repair that closes this is relocation rather than restoration: restore the damaged region from the canonical AND re-insert the member's lost lines at the position its own intent shows, inside the component it was editing (or the nearest surviving component when the CP deleted that one), then re-check the `#queuelineclobber` post-condition on the result. Reproduction already in tree: `a_member_insertion_that_lands_outside_every_component_is_flagged_not_deleted` in `agent-doc-document-realtime/src/crdt_relay.rs`.
- [ ] [#clearunobservedrecycle] [P2] `/clear` reported `submission_unobserved` on two haiven-dev panes (%16 docs.md, %15 dev.md) at 2026-09-11T20:24:15Z / 20:25:01Z, both `command_visible=false`, `elapsed_ms≈908`, `capture_len=2146`/`1138`. Evidence: `src/haiven-dev/.agent-doc/logs/ops.log` shows the same window churning controller generations 1176→1178 (`controller_active_non_stable_deferred ... handoff_state=Preparing`), so the panes were mid-restart and held no dispatch-ready prompt line — which is why `context_clear_history_proves_cleared_state` (#clearsubmitunobserved) could not resolve the idempotent case and the guard fell through to blocked. The guard itself is correct (fail-closed, exact unblocker). What is missing is the preceding step: the clear was attempted against a controller that had already refused as non-authoritative. Check whether the client-side #handoffrefusalretry landed in e13404d9c now defers the clear until the generation promotes; if it does not cover the supervisor_ipc_command_repair_acceptance path, extend it there. Repro: run `make install` against live route-owned supervisors, then issue Clear Session Context within ~30s.
- [ ] [#clearsubmitpanesettle] [P3] [recommended] `context_clear_history_proves_cleared_state` requires a dispatch-ready prompt line, so a pane that is restarting reads identically to a pane that swallowed the clear. Consider a distinct `PaneRestarting` status (harness banner present, no prompt yet) whose unblocker is "wait for the harness to finish starting" rather than "verify pane state then retry" — the operator currently cannot tell those two apart from the message.
- [ ] [#queuegorevokedonfreetext] [P2] After #queuelineclobber, an operator who writes a free-text `re [#id]: <question>` queue line and flips `queue: go` keeps their text but still gets the activation revoked (`[preflight] queue: stripped activation token (drained)` then `queue: set queue: stop`) — that path is separate from the now-gated need_clear_drained_body. Defensible as-is (a `re` line is not a drainable head, and the inactive_queue_residue warning routes it to the next cycle's prompt), so this is a design question, not a proven defect: decide whether an operator activation should survive a body that holds only non-drainable operator text, or whether the revoke plus the residue warning is the intended answer. Decide before changing it — go-mode churn (#qchurn) is the failure mode on the other side.
- [ ] [#layoutbindstale] [P2] `derive_layout_actor_bindings` (src/agent-doc/agent-doc-controller-io/src/project_controller.rs:746-784) builds `desired_windows` from the RECORDED `record.window_id`, then binds every other actor whose recorded window is in that set as a co-window sibling of the layout. The record is provably untrustworthy: `agent-doc session doctor src/haiven-dev/tasks/infra.md` reports actor+registry `window=@0` for pane %14 while tmux has %14 in @3 (@0=agent-doc visible, @3=stash). So a stashed pane can be joined into a layout it is not part of, and after a `session doctor --repair` (which rebinds the record to the LIVE window) every stashed pane would share the stash window id and be bound together. Decide whether this join should consume a window id at all, or key on the desired document set alone. NOTE from #stashfocuswindow: resolving live is NOT automatically the right answer — for a selection effect the live window is the stash, which is precisely what must not be used. Status: a reading of the join, not an observed symptom; needs a repro before changing it.
- [ ] [#jbfocusdocroute] [P2] The JB "Focus Agent Terminal" notification is per-document ("Agent input required for $fileName", TurnStateBannerRefresher.notifyInputRequired) but IdeTerminalHost.focusExisting(project) ignores the file and focuses a single project-wide terminal tab named "agent-doc". In a multi-document tmux layout the correct target is the pane that OWNS that document. The plugin already has the machinery (TmuxPaneFocusSync.recordEditorFocusIntent takes a documentPath, though it only records a lease — the actual routing goes through the command plane via EditorFocusSyncListener). Do this together with #stashfocuswindow: both are pane->window/document resolution, and solving it in two places separately is how three tmux focus reports became three separate reports. The silent-no-op half of this is already fixed in plugin 0.2.381 (commit 50bc06de5); what remains is routing to the right pane rather than merely always navigating somewhere.
- [ ] [#compactackflake] [P2] CI run 34626410869 (0.35.366, workflow CI) is RED on agent-doc-document-realtime-io tests::compact_exchange_returns_retained_success_when_editor_ack_stays_delayed, panicking at agent-doc-document-realtime-io/src/lib.rs:9472 (3183 passed / 1 failed / 259 skipped, 1025s). It predates the #fixdocsturn commit 0f959e11f and is not reachable from it (that change touches agent-doc-diff + baseline_comparison only), and local `make check` on the same tree is green. Same delayed-editor-ack shape as #retainedresumewakeflake, and that item already carries the diagnosis to apply here: the test synchronises on a subscribe RETURNING rather than on the condition holding, so a contended runner returns from subscribe for something other than the awaited event. Check whether :9472 waits on a subscribe/timeout proxy and convert it to a bounded poll of the actual condition.
- [ ] [#retainedresumewakeflake] CI run 34621201563 (0.35.364) went red on agent-doc-controller-io `project_controller::tests::retained_resume_reacts_when_the_intent_arrives_after_delivery` — `assertion left == right failed; left: None, right: Some("retained_settled_delivery_reactive")` at project_controller.rs:15843. Already checked, do not re-walk: it does NOT reproduce locally (~900 executions, including `taskset -c 0` with --test-threads=8 and a 2-core taskset), and neither 0.35.364 change is reachable from it — pane_layout_projection_converged is in the pane-layout worker this test never drives, and capture_resume_unowned is in agent-doc-capture-io, which agent-doc-controller-io's lib test binary does not link. That CI run took 805s vs 166s locally, so the runner was heavily contended. Lead: the test's synchronization is a proxy, not a wait. It asserts `!subscribe_state_plane(CAPTURED_FINALIZE_WAKE_STATE_CHANNEL, None, 0, 2s).timed_out` and then reads runtime.captured_finalize_wakes immediately. record_captured_finalize_wake (rpc.rs:18944) inserts into that map BEFORE publishing the frame, so an absent key proves the subscribe returned for something other than this wake — check what state_plane_effective_after_version(None, 0, gen) plus the generationless-cursor reset makes `subscribe` return when the channel has no frames yet. Fix: wait on the condition (poll captured_finalize_wakes for the expected reason under a bounded deadline) rather than on subscribe-return, and apply it to all three sites using this pattern (15694, 15843, 15898). Keep both assertions — that settlement publishes the receipt AND that it records the reason.
- [ ] [#cargotokenrevoke] REVOCATION NO LONGER NEEDED — closed by expiry, verified 2026-09-11 23:0x on the live https://crates.io/settings/tokens page: the exposed `publish-token` (created 3 months ago, last used ~4h ago) shows **Expired about 4 hours ago** and crates.io offers it no Revoke action at all, only Regenerate. An expired token cannot authenticate, so the security goal of this item is met. Re-issue also happened: a replacement `publish-token` was minted and is in `passage btak/CARGO_REGISTRY_TOKEN`. What remains is not revocation but wiring — tracked as #cargotokenwire (nine repo secrets still hold the pre-rotation token), plus one operator decision because the new token's cleartext reached this session's output.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#fpecompactheal] [operator-verify] Confirm fpe.md's corrupted `<dynamic_context_ref>` block is gone after one compact on the FIXED binary (installed 0.35.367, ~/.cargo/bin/agent-doc at 23:01:39). Must be run FROM THE PANE THAT OWNS src/haiven-dev/tasks/fpe.md — that is a live session (b69e5aeb, `queue: go`) and this cycle deliberately did not hand-align its disk. Expected after `agent-doc compact src/haiven-dev/tasks/fpe.md`: exactly ONE block, with the two self-referential handles (absolute `.../src/haiven-dev/tasks/fpe.md` and relative `tasks/fpe.md`) gone, leaving at most the four instruction-file handles; and the `Prior summary/context:` line still summarising the prior compact list rather than quoting XML.
- [/] [#fixrunlazily] Run Agent Doc on lazily.md fails with `command-plane editor_route request failed via .agent-doc/controller.sock: Project Controller did not respond within 60000ms; the controller may be wedged`. NARROWED 2026-09-11, two hypotheses measured and DISPROVED — do not re-walk them. (1) The failure is the ACCEPT leg, not the work leg: the prefix `command-plane editor_route request failed` plus 60000ms is SOCKET_REQUEST_TIMEOUT_MS in sendAcceptedCommandSubmitToSocket; awaitCommandSubmitTerminal uses waitForReadySeconds*1000+grace instead. So the controller failed to ACKNOWLEDGE a command submit for 60s, and handle_editor_command_submit_async_rpc only enqueues. (2) NOT serialized accepts: rpc.rs:11185 listener.accept() spawns a thread per client. (3) NOT a slow pre-serve window: the socket binds at ListenerOptions::create_sync() (rpc.rs:11134) and the accept loop starts at 11184, so synchronous work between them leaves clients connected and hanging — the ss -lxp backlog signature — but the candidate, restore_reliable_sync_liveness, measures 0.08s for all 59,671 reliable_sync_liveness_journal rows / 6.5MB / 60,582 ops on this project, and its recovery effects are already spawned async. Live evidence at the failure window: ffi_ensure_controller_running_failed Connection refused (os error 111) x7 at 14:00:58-59, controller adopted 14:01:44, and a separate client still timing out at 14:02:23 (timed out after 5.0s waiting for project controller response) — unresponsive at least 40s after adoption. NEXT LEAD: what holds a lock across handle_request_locked (rpc.rs:12526) at runtime.bootstrap_snapshot() (12560) during a controller first seconds. Repro lever: kill the controller, then click Run Agent Doc while a replacement is launching.
- [/] [#pypiquotafix] [operator-verify] PyPI 10 GiB project-size block: non-destructive path (a) EXECUTED 2026-09-10 — filed https://github.com/pypi/support/issues/12214 requesting 25 GiB. Awaiting PyPI maintainer response; every agent-doc publish since 0.35.343 fails with 400 Project size too large, and 0.35.343 itself landed only 1 of 4 wheels (macosx_10_12_x86_64). Measured at filing: 9.990 GiB across 165 releases, avg 62.0 MiB. Unblocks: (1) PyPI grants or counters the increase, or (2) you authorize destructive path (b) — delete the 34 oldest releases 0.35.0 through 0.35.112 (all pre-2026-08-02) to free 1.75 GiB, permanent and those version numbers can never be reused. The publish cadence fix landed in 0.35.363 (#pypicadence): milestone tags publish automatically, other tags publish on demand, and a verify job now asserts the tagged version is resolvable on PyPI. ALSO GATED ON THIS: the GH #52 wheel-FFI path is written and proven at the maturin layout level (data = wheel-data puts libagent_doc.* in .data/scripts/ so pip lands it beside the binary, with a pre-upload assertion) but has never run in CI, because no publish has been possible since the quota block. On the first successful publish after this unblocks, confirm the wheel ships the library and then close GH #52.
- [/] [#tmuxfocussyncverify] [operator-verify] Restart IntelliJ to activate installed JetBrains plugin 0.2.379, then keep agent-doc-bugs.md and the cross-root infra.md visible, click each embedded tmux pane, and verify editor selection follows without stealing focus from an actively focused editor component. Automated lifecycle and transition regressions plus make check tmux-ci passed; source v0.35.362 is published. Plan: tasks/agent-doc/plan-tmux-auto-sync-reactive.md.
- [/] [#grokmcpverify] [operator-verify] Verify an authenticated Grok Build MCP admit/read/plan/finalize cycle and same-turn auto-queue continuation once the free usage quota renews or the planned subscription is active. Grok CLI authorization, native JSON/backend commit fixtures, pane detection and quit behavior passed; the live MCP probe stopped at the usage limit before committed closeout. Installed v0.35.359; Heavy is not required. Plan: tasks/agent-doc/plan-grok-build-support.md.
- [/] [#activateinstalledjetbrai] [operator-verify] Installed JetBrains plugin 0.2.379 is staged while running IDEA PID 1035669 remains on 0.2.378; one restart activates both the reverse tmux-focus fix and the current package generation. Remaining splice proof: captured local edits recover across an independently advanced canonical response in a disposable live editor document. The original retained capture committed as 1b35dcb028, and all automated regressions passed. Plan: tasks/agent-doc/plan-captured-editor-splice-recovery.md.
- [/] [#harnessswitchverify] [operator-verify] Current prerequisites verified: IntelliJ PID 1035669 runs plugin 0.2.378; CLI/native 0.35.361 is installed. Remaining proof: in a disposable live IntelliJ session, make an unsaved codex -> claude -> codex frontmatter switch while observing supervisor receipts; prove one spawn per authoritative switch, deferred restart for non-authoritative evidence, and no refusal/respawn storm. Unblocks: provide the disposable document path and perform the real unsaved editor edit; persisted CLI writes cannot substitute for this proof. The unrelated Stop-hook gate-selection defect is fixed in v0.35.361; plan: tasks/agent-doc/plan-stop-gated-queue-head.md.
- [/] [#coder7] [operator-verify] Phase 7 — run the five-case live Coder workspace validation matrix after a reachable Coder workspace and authenticated access are available, with JetBrains Gateway remote backend, VS Code Remote-SSH, simultaneous dual-IDE attachment, workspace stop/start, and a no-tmux image variant. Plan: tasks/agent-doc/plan-coder-ide-hosted-tmux.md. Unblocks: provide the workspace URL or SSH target plus authentication and enable the current agent-doc plugin/extension in both remote IDE hosts.
<!-- /agent:review -->

<!-- agent:icebox -->
- [ ] [#ha3hpolicy] [MULTI-USER FOLLOW-UP] Recipient-side policy config: .agent-doc/config.toml receive.auto_create_components = ["pending"] (or = false); flip notify/write default from auto-create to consult-recipient-config. Hard-blocked until multi-user workspace support exists; issue #40 (open) explicitly says not to ship before that prerequisite. Moved out of agent:review because it is an unstarted future feature, not code-complete work awaiting validation. Tracked: https://github.com/btakita/agent-doc/issues/40
- [ ] [#pdar] [recommended] Evaluate optional binary auto-resolution only for the narrow case where the prompt is explicitly do #id and there is exactly one matching open pending item
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc/agent-doc-bugs.done.md -->

<!-- completed work archived in tasks/agent-doc/agent-doc-bugs.done.md -->
<!-- /agent:done -->
