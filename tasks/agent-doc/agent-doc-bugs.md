---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: 3d56a2a6-95ce-42c4-b71c-66c102e54fb4
  codex: 01a098b6-69b6-7801-9c97-72ad1659e5c9
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#actionable-review': Add actionable review items into backlog + queue
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: stop
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

### Re: do [#ctxrefsynthsource] — opus-5 · 2026-09-12T14:58-04:00

> **Queue prompt:**
>
> 📌 do [#ctxrefsynthsource]


Done — `6722d04a7` (0.35.374), pushed and installed. **And the landing note in the item was wrong**, which is the useful part of checking it first.

**The mislabel.** `candidate_payloads` fell through to `source_uri = report.target` for any candidate with no `file`/`target` key, so the `next_context` and `agent_doc_queue` chunks agent-doc builds *from* the cycle report were recorded against the session document and rendered `expand="tsift --envelope source-read <the document>"` — a command that returns the document instead of the chunk. Those chunks now carry `agent-doc://cycle/next-context`, `agent-doc://cycle/agent-doc-queue`, and `agent-doc://cycle/report-summary`.

Exploration windows are untouched — they already carry a real `file`/`target`, and a second test pins that they keep their working `source-read`, so the fix cannot quietly make everything unexpandable.

**No command resolves a synthesized chunk, so it says so.** The injection ledger records identity, never payload — `StoredContextInjection` has no text column — so nothing can read one of these back after the cycle that built it. Inventing a command would move the lie rather than remove it, so the reference renders `expandable="false"` in place of `expand="…"`.

**`#fixcompactexchange`'s drop is now principled.** It had to drop document-sourced handles *because of* this mislabel; the document-facing projection now drops on `chunk_is_actionable_continuity` — a handle nobody can expand, or one naming the document itself. Behaviour there is unchanged; the reason is the true one. The substance of this fix lands in `as_prompt_section` and `as_orchestration_child_section`, which had no such filter and were emitting the lying command straight into prompts.

**The note to check before landing was wrong.** `source_uri` does **not** participate in the `already_injected` dedup key. That lookup keys on `chunk_id`, then `content_hash` (`agent-doc-sqlite/src/context_injection_ledger.rs:320`), and `chunk_id` is `<handle>-<content_hash[..12]>`. Neither derives from `source_uri`, so changing it re-expands nothing — there was no migration to stage and no once-per-document re-expansion to absorb.

Mutation-checked cold, both halves: M1 (synthesized source falls back to the pack target) reddens the identity test; M2 (drop the scheme check in `source_read_command`) reddens the identity and render tests. `make check` EXIT=0, 9437/9437. CI run 34712752244 is in progress; this stands on local evidence.

`#ctxrefdocnoise` is next in the queue and is a DECISION item about whether this block is worth keeping at all — I deliberately did not preempt it. This change only makes the labels honest, so that decision now gets to look at accurate data.

Exchange is at 199 lines / 8 sections — noting it rather than stalling the queue.

### Re: do [#ctxrefdocnoise] — opus-5 · 2026-09-12T15:16-04:00

> **Queue prompt:**
>
> 📌 do [#ctxrefdocnoise]


Decided **(b)** and landed it — `960f30d9c` (0.35.375), pushed and installed. What settled it was a measurement, not a preference.

**The block renders nothing in this workspace.** Three live projects hold **4** `context_injections` rows between them — agent-loop 0, agent-doc 0, haiven-dev 4 — and all four name the session document itself (`tasks/haiven-dev.md` twice, once absolute, plus `tasks/haiven-models.md`). `#fixcompactexchange`'s self-reference filter drops every one, so `durable_context_reference_for_document` returns `None` for every document here. The block has been dead output, not noisy output.

That is what rules out **(a)**. Narrowing document-facing pack selection to source/work files tunes a selector whose output is already empty — and the handles the item worried about (`AGENTS.md`, `.agent/runbooks/code-navigation.md`, `.gitmodules`, `.gitignore`) are instruction and repo-metadata files the harness already loads, so a narrower selector would be removing noise the block is not currently producing. **(c)** keeps a path that has never emitted anything here plus a test pinning it.

**And where it could work, it is redundant.** The block is a projection of `.agent-doc/state.db`; `agent-doc plan` and prompt-context rebuild it from those same rows every cycle. A compacted document that stays in its checkout gains nothing from carrying a copy. Its one unique use — a document that travels *without* that database — is the transfer/extract path, which renders it directly at `src/extract.rs:101,731` and is untouched.

**The strip survives, and now earns its keep on its own terms.** `#fixcompactexchange` made this strip-then-append so repeated partial compacts could not accrete one identical block each. With the append gone, the strip becomes a cleaner: a compact removes whatever block an earlier compact left in the preamble, so documents already carrying one converge to zero instead of getting a fresh copy. `durable_context_reference_for_document` stays public and unchanged.

Both pinned tests were rewritten to the new contract rather than deleted. The first seeds a live, non-self-referential manifest — precisely the shape that used to produce a block — asserts the compacted document holds none of it, and asserts the manifest is *still durable* so transfer/extract can still render it. The second asserts a carried-forward block reaches zero and that no later compact reintroduces one. Mutation-checked cold on both paths: restoring the append in the full compact reddens the first, restoring it in the partial compact reddens the second.

`make check` EXIT=0, 9437/9437. CI run 34713635510 is in progress; this stands on local evidence. Exchange is at 224 lines / 9 sections with 2 no-op closeouts in the last half hour — noting it, not stalling the queue.

### Re: IPC proof diagnostic (interrupted-cycle recovery) — agent-doc

```text
**IPC proof issue dogfood log**
Appended automatically during interrupted-cycle recovery to record the editor IPC issue.
This is binary-authored diagnostic content, not a user prompt, so it does not require a separate response cycle.
Issue class: `ipc_proof_insufficient`
Affected component: editor IPC / writeback

ipc_proof_insufficient file=/home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs.md source=socket_visible_write patch_id=0c51c73f-9745-4dc1-9286-87fb437ded73 invariant=live_prompt_drift_after_preflight recovery=visible_repair_required snap_source=lazily_visible_write_event candidate_len=23716 candidate_hash=cc5974fb0aaf3c7920b284a83e10d91419c1d8fa916fb77f4858aaede2cc75cc content_ours_len=26405 content_ours_hash=7535cbc69b449d68b00fdfde129ac2f357697209f25c0cd0e8f1733fe52c21ea doc=agent-doc-bugs session=3f069c68-7119-4c7b-84a9-4baeb84be35c turn=cycle-1788997134080
```

### Re: Response

Continued option (b), based on the prior Claude session already updating `existence-lang/existence`.

- Updated the remaining eight stale GitHub secrets.
- Configured the two missing consumers: `signal-space/signal-space-rs` and `btakita/tmux-router`.
- Verified `CARGO_REGISTRY_TOKEN` is present across all eleven repositories.
- Never printed the token value.

The [agent-doc cycle](/home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs.md:314) did not commit: a stale IntelliJ editor lease reports zero live replicas and blocks reconciliation, matching `#jbreloadreregconverge`. I did not resubmit or force-write.

Safe unblock: restart IntelliJ to load plugin 0.2.382, then reopen/focus this document and invoke `agent-doc tasks/agent-doc/agent-doc-bugs.md` again.

The VOD still needs its URL or platform/start time. The exposed output occurred at `2026-09-12T02:57:27.170Z` and remained visible until approximately `02:57:52Z`.

### Re: do [#cargotokenwire]

> **Queue prompt:**
>
> 📌 do [#cargotokenwire]


Completed and verified the token wiring without reading or printing any secret value.

The interrupted Claude turn had already updated the eight stale `btakita/*` repositories and added the previously missing secret to `signal-space/signal-space-rs` and `btakita/tmux-router`; `existence-lang/existence` had been updated earlier. A fresh GitHub metadata check confirms `CARGO_REGISTRY_TOKEN` now exists in all eleven repositories. The eight stale entries share update timestamps from 2026-09-13T03:04:38Z–03:04:40Z, the two missing consumers were created at 03:15:26Z, and `existence-lang/existence` reports its earlier 2026-09-12T04:54:53Z rotation.

The credential value was never fetched during verification. I split the remaining livestream-VOD redaction into `#cargovodtrim` under Review because it is operator-gated: the document still has no VOD URL or platform/start-time locator. Its known exposure window is 2026-09-12T02:57:27.170Z through approximately 02:58:14Z.

Exchange is large (over 270 lines), but the queue remains active, so I am continuing it rather than pausing for compaction.

### Re: do [#isolationrelocate] and Restart Agent handoff

Implemented, released, and installed as `agent-doc 0.35.376` in commit `c249b8781` (`v0.35.376`, pushed to `main`).

`#isolationrelocate` now completes the lossless repair instead of stopping at `component_isolation_refused_lossy`. After positive cross-component damage proof, the relay restores only the damaged framing, reconstructs the member's inserted lines from its own before/after intent, and reinserts them at the corresponding line boundary inside the edited component. If the controller concurrently deleted that component, the closest surviving component receives them; an equal-distance tie selects the preceding component so prose does not become a queue directive. The existing member-insertion loss check runs again on the repaired result and still fails closed to the raw union if any insertion cannot be accounted for.

The pinned deleted-component reproduction is now end-to-end green and asserts both restored framing and relocation into `exchange`. A second regression covers position preservation when the edited component survives. A deliberate tie-break mutation made the end-to-end test fail, then passed after restoration. The full release gate passed: 9,441/9,441 Rust tests, 179/179 VS Code tests, cross-editor simulation, documentation/parity audits, and all formal model checks.

The added `lazily.md` Restart Agent failure is fixed in the same release. An explicit restart at a `Ready` boundary may reclaim only an empty abandoned preflight; captured/busy cycles remain protected. Child shutdown targets the PTY foreground process group, so Claude's two subagents cannot survive the replacement. The project-controller receipt now waits for the first authoritative lifecycle decision, eliminating the false `accepted/background_started=true` result followed by a buried refusal.

The release-profile binary and native library are installed at `0.35.376`; VS Code `0.2.70` is installed, and the unrelated pre-existing JetBrains `0.2.382` package was preserved rather than downgraded. `lazily.md` still currently says `agent: restart`, not `agent: codex`, so I recorded `#restartlazilyverify` under Review for the actual Codex selection and live handoff check.

GitHub Release run `34737823202` was still externally queued across all six platform jobs after five minutes, so I recorded `#release035376verify` rather than claiming the release assets already exist.

Exchange is over 287 lines, but the queue remains active, so I am continuing it rather than pausing for compaction.

### Re: `#clearunobservedrecycle` — gpt-5 · 2026-09-13T00:53-04:00

Fixed and released as agent-doc 0.35.377.

The earlier `#handoffrefusalretry` change covered project-controller client retries, but the `operator_command` handler itself still accepted Clear while its controller generation was `Preparing`. That acceptance let the caller proceed to supervisor IPC during recycle; the retiring supervisor could acknowledge the write while its actor pane never rendered `/clear`, leaving the later verifier with `submission_unobserved`.

The controller now refuses every operator-command authorization from a non-stable generation before actor lookup or receipt insertion. The existing bounded client retry reconnects until the promoted generation answers, so Clear cannot contact tmux or the supervisor until controller authority is stable. The regression simulates two retiring generations followed by promotion and proves that only the stable generation creates the single durable acceptance receipt.

Changed paths are the controller RPC implementation and regression, the session/tmux command specification, the 0.35.377 changelog/parity entry, and generated version manifests/skill projections. Pre-existing unrelated worktree edits remain unstaged.

Verification and delivery:

- The regression failed before the guard (`attempts: 1`, expected `3`) and passes after it.
- `cargo test -p agent-doc-controller-io clear_authorization_waits_out_a_non_authoritative_controller_generation` passes.
- `make check` passes in full.
- Commit `533a7a5cd` (`fix: defer clear across controller handoff`) is pushed to `origin/main`; tag `v0.35.377` is pushed.
- The clean release build installed `agent-doc 0.35.377` and `libagent_doc-0.35.377.so`; VS Code remains 0.2.70 and the intentionally newer local JetBrains development package was restored at 0.2.382.
- GitHub CI run `34738628529` is in progress; per the project workflow this is non-blocking after the green local full gate.

Confidence: high. No escalation is required.

### Re: `#clearsubmitpanesettle` — gpt-5 · 2026-09-13 01:18 EDT

Implemented and released as agent-doc v0.35.378 (`2d5b7a47f`, tag `v0.35.378`, pushed to origin).

The clear acceptance probe now distinguishes a harness that is visibly restarting from a swallowed submission. A Claude Code, Codex, or OpenCode startup banner without a dispatch-ready prompt produces the non-accepted `pane_restarting` status, with `wait_for_harness_to_finish_starting` as its explicit action. A ready prompt overrides an old banner in scrollback, and `pane_restarting` cannot authorize either a submit-key retry or a full `/clear` resend.

Changed paths: `agent-doc-turn-executor-tmux/src/context_clear.rs`, `src/session_actor_cmd.rs`, `specs/07-session-tmux-commands.md`, `VERSIONS.md`, `editors/release-parity.tsv`, and the v0.35.378 manifest/skill projections. Pre-existing unrelated worktree changes were excluded from the commit.

Verification:

- Captured the intended pre-fix failure: the new startup-banner regression failed while the classifier stub returned false.
- `cargo test -p agent-doc-turn-executor-tmux` passed (65 tests).
- The explicit wait-only/no-resend regression passed after the final test addition.
- `cargo test -p agent-doc --no-run` passed.
- Full `make check` passed after all edits.
- Installed and verified `agent-doc 0.35.378` and `libagent_doc-0.35.378.so` from a clean tagged worktree; VS Code is 0.2.70 and the intentionally newer local JetBrains build 0.2.382 was restored.
- GitHub Actions CI run `34739714791` for this commit is in progress; local verification is complete.

Confidence: high. Escalation: none.

### Re: `#queuegorevokedonfreetext` — gpt-5 · 2026-09-13 01:22 EDT

Decision: keep the current activation revocation. This is intentional queue semantics, not a defect.

A `re [#id]: <question>` residue line is preserved operator text but is not an addressable, drainable queue head. Queue activation represents executable priority work, so retaining `queue: go` when no executable prompt remains would leave an active queue with nothing it can consume and reintroduce the `#qchurn` failure mode. The correct outcome is therefore:

- preserve the operator text in the document and snapshot;
- strip the one-shot activation token and set `queue: stop`;
- emit `inactive_queue_residue` for the edit cycle so the text is surfaced rather than silently ignored.

When execution is intended, the operator should use an addressable `do [#id]` queue prompt or place the question in the exchange. No production or specification change was needed: the existing format contract already says activation requires prompts, freeform content is inert, and an empty queue drains.

Changed paths: none.

Commands and verification:

- Inspected `run_queue_maintenance`, activation resolution, queue parsing, lossless drain gating, and the queue-format/orchestration specifications.
- `cargo test -p agent-doc-preflight-io queue_maintenance_never_clears_a_drained_body_over_operator_text -- --nocapture` passed and reproduced the intended `stripped activation token (drained)` / `queue: stop` outcome while preserving the exact operator line.
- `cargo test -p agent-doc-queue activation_start_fence_bare_no_prompts_after -- --nocapture` passed, confirming a consumed start fence with no remaining prompt resolves inactive.
- The full `make check` gate already passed on this unchanged v0.35.378 tree in the immediately preceding cycle.
- Latest CI run `34739714791` remains in progress.

Confidence: high. Escalation: none.

### Re: `#layoutbindstale` — gpt-5 · 2026-09-13 01:40 EDT

Implemented and released as agent-doc v0.35.379 (`86b71340a`, tag `v0.35.379`, pushed to origin).

The reproduction confirmed the defect: when a requested actor and an unrelated stashed actor both had the repaired recorded window `@3`, `derive_layout_actor_bindings` included both documents even though only one appeared in the desired columns. Recorded-window equality cannot prove layout membership because unrelated stashed actors legitimately share the stash window.

The reactive join is now keyed strictly by the desired document set. Actor records provide bindings only for requested documents, while the existing structural-receipt fallback remains generation- and desired-document-fenced. Actor-store changes still invalidate the Computed projection, but cannot widen layout ownership by coincidental window ID.

Changed paths: `agent-doc-controller-io/src/project_controller.rs`, `specs/07-session-tmux-commands.md`, `VERSIONS.md`, `editors/release-parity.tsv`, and the v0.35.379 manifest/skill projections. Pre-existing unrelated worktree edits were excluded from the commit.

Commands and verification:

- Added a common-stash-window regression and captured its pre-fix failure: bindings incorrectly contained both `infra.md` and `lazily.md`.
- The new regression passed after narrowing the join.
- `pane_layout_actor_authority_is_a_reactive_join` passed with its expectation corrected to the desired document set.
- Full `make check` passed.
- Installed and verified `agent-doc 0.35.379` and `libagent_doc-0.35.379.so` from a clean tagged worktree; VS Code is 0.2.70 and the intentionally newer local JetBrains build 0.2.382 was restored.
- GitHub Actions CI run `34740585562` for this commit is in progress; local verification is complete.

Confidence: high. Escalation: none.

### Re: `#jbfocusdocroute` — gpt-5 · 2026-09-13 02:14 EDT

Implemented and released as agent-doc v0.35.380 (`37e9ba840`, tag `v0.35.380`, pushed to `origin/main`) with JetBrains plugin 0.2.383.

The input-required notification now retains its document identity: before activating the shared IDE terminal tab, it routes `filePath` through `EditorFocusSyncListener` and the existing `EditorTabSyncListener.onEditorFocusGained` command-plane projection. The controller therefore selects the pane/window that owns the notified document instead of leaving whichever project-wide `agent-doc` terminal pane was already active. A regression pins both halves of that wiring.

Changed paths: `EditorFocusSyncListener.kt`, `TurnStateBannerRefresher.kt`, `EditorFocusSyncListenerTest.kt`, release notes/parity metadata, and generated 0.35.380 manifests. The release was assembled in a clean detached worktree so the existing uncommitted input-notification deduplication work was not included or overwritten.

Verification: focused JetBrains Gradle tests passed in both the shared checkout and the exact clean release tree; full `make check` passed; `git diff --check` and Cargo metadata validation passed. Installed proof is `agent-doc 0.35.380`, `libagent_doc-0.35.380.so`, JetBrains 0.2.383, and VS Code 0.2.70. GitHub CI run 34741517865 and Release run 34741520704 were still in progress with no failure when inspected; the CI tmux step was pending behind `make check`.

The additional `lazily.md` operator verification failed and has been moved back into the active bug queue as `#restartlazilyverify`. Live receipts 662–664 reproduced a split authority: actor/status reported `harness: codex`, but the supervisor repeatedly launched Claude with the old Claude resume ID because restart-time frontmatter came from the attached editor CRDT revision still containing `agent: restart`. The temporary disk edit was restored so no divergent session-document edit remains. The queued fix now requires one retained target-harness/document-revision intent across controller admission, supervisor recycle, and child launch, plus proof that Claude and both subagents terminate before Codex starts.

Confidence: high for the JetBrains routing fix and live `lazily.md` reproduction. Escalation: none; the remaining restart defect is queued with exact lifecycle evidence rather than being misreported as a successful switch.

### Re: `#compactackflake` — gpt-5 · 2026-09-13T02:41-04:00

Fixed and released as agent-doc 0.35.381 (commit `75d1b5f17`, tag `v0.35.381`).

The referenced CI failure was a test-harness flake, not a retained-write behavior failure: run 34626410869 completed the delayed-ACK path correctly, but the regression test required the calling thread to be scheduled again within 500 ms during a saturated 9,325-test run. The test now asserts the semantic proof directly—the unconverged retained receipt, the `compact_crdt_delivery_deferred` operation with `recovery=retained_async_editor_delivery`, and the typed refusal that blocks snapshots/commits—without using scheduler latency as control-flow evidence.

Changed paths:
- `agent-doc-document-realtime-io/src/lib.rs`: removed both brittle 500 ms assertions and documented why the state/operation assertions are authoritative.
- `VERSIONS.md` and `editors/release-parity.tsv`: recorded 0.35.381.
- Release-version projection updated the workspace manifests, lockfile, package metadata, and bundled skill surfaces to 0.35.381.

Verification:
- The focused delayed-ACK regression passed 25 consecutive runs.
- `make check` passed completely, including 9,325 Rust tests, JetBrains and VS Code suites, native cross-editor parity, release-version checks, and editor-parity audit.
- `agent-doc --version` reports 0.35.381; `libagent_doc.so` resolves to `libagent_doc-0.35.381.so`.
- JetBrains 0.2.383 and VS Code 0.2.70 were rebuilt/reinstalled.
- CI run 34743219946 and Release run 34743227480 started for `75d1b5f17` and remain in progress; the preceding 0.35.380 CI and Release runs completed green.

Confidence: high. The failure condition was reproduced from the exact CI log, replaced with deterministic semantic assertions, stress-run locally, and validated by the full release suite.

Escalation: none.

### Re: `#retainedresumewakeflake` — gpt-5 · 2026-09-13T02:57-04:00

Completed and released as agent-doc **0.35.382** (`5cc22b566`, tag `v0.35.382`).

The CI-only failure was a stale state-plane barrier in the retained-resume regression itself. `capture_response` publishes the ordinary captured-finalize wake and then the test clears only the in-memory wake map; subscribing later from version zero could immediately consume that historical frame before the reactive retained-resume path inserted its new wake. Both intent/delivery arrival-order tests now snapshot the exact pre-trigger channel cursor and wait for a strictly newer publication, whose producer has already populated the wake map.

Changed paths:

- `agent-doc-controller-io/src/project_controller.rs` — pin the pre-trigger wake cursor in both retained-resume arrival-order tests.
- `VERSIONS.md`, `editors/release-parity.tsv`, workspace manifests, lockfile, and bundled skill surfaces — project release 0.35.382.

Commands and verification:

- Reproduced CI run `34621201563` and traced the failed assertion to the stale historical `ResponseCaptured` frame.
- Ran both `retained_resume_reacts_when_` regressions 100 times each (200 executions): passed.
- Ran full `make check`: passed (`tart-b91fb8ef2c`).
- Pushed commit `5cc22b566` to `main`, pushed tag `v0.35.382`, and ran `make install` successfully.
- Installed CLI reports `agent-doc 0.35.382`; native library resolves to `libagent_doc-0.35.382.so`; JetBrains 0.2.383 and VS Code 0.2.70 packages installed.
- GitHub CI `34743942989` and Release `34743946714` are in progress; the preceding 0.35.380 CI/release are green.
- Preserved the existing 27-file dirty development checkout while advancing its baseline to `5cc22b566` and merging only this cursor patch into its already-modified controller source.
- Checked `/tasks/software/lazily.md`: the replacement receipt has reconciled from pending to an abandoned interrupted cycle (`cancel_preflight_cycle_abandoned`), while its original “Resume multiple Claude code sessions…” prompt remains queued for a fresh continuation.

Confidence: high.

Escalation: none.

### Re: `#cargotokenrevoke` — gpt-5 · 2026-09-13T03:00-04:00

Closed. No revocation action remains: the exposed crates.io `publish-token` was verified on the live token-settings page as expired, and crates.io offered only regeneration rather than revocation. An expired token cannot authenticate, so the security objective of this item is already satisfied. A replacement token was separately minted and stored in `passage btak/CARGO_REGISTRY_TOKEN`; repository-secret rotation was tracked and handled under `#cargotokenwire`.

Changed paths: none outside this binary-owned session-document closeout.

Commands: `agent-doc plan`; scoped `tsift` reads of the backlog item and `memory/reference_cargo_publish_token_passage.md`.

Verification: the backlog record and durable operator memory agree on the live crates.io observation, expiry state, replacement-token location, and separation of the wiring work. No secret value was read or printed during this cycle.

Confidence: high.

Escalation: none.

### Re: Slim PyPI package and aggressive release cleanup — gpt-5 · 2026-09-13T13:11-04:00

> **Queue prompt:** How can we make the pypi package slim? Can we make it download & extract the tarballs from github instead of sending the entire binary into PYPI? When we do the release, please aggresively delete old releases.

Implemented and released the slim-package design as `agent-doc` 0.35.383 (`f467696a9`, pushed to `origin/main`, tag `v0.35.383` pushed). PyPI now receives one small universal Python wheel instead of platform-native binaries. On first invocation, the launcher detects the supported platform, downloads the matching pinned GitHub Release archive plus `SHA256SUMS`, verifies the exact SHA-256 digest, safely extracts only the expected executable and FFI library into a versioned user cache, and executes the native CLI. Subsequent launches reuse the verified cache.

The release workflow now publishes six platform archives plus `SHA256SUMS`; the PyPI workflow waits for those assets, rejects native payloads or wheels larger than 256 KiB, publishes the universal wheel, then verifies a clean install and first-run native download. Release documentation and the old native-wheel regression were updated to enforce the new contract.

The requested aggressive cleanup is authorized but not yet executed. Public PyPI metadata shows 165 releases consuming about 9.99 GiB. Deleting the 34 oldest existing releases, 0.35.0 through 0.35.112 (all before 2026-08-02), would reclaim about 1.75 GiB. PyPI release deletion is permanent and breaks pinned installs of those versions; yanking would not reclaim storage. The Chromium PyPI tab is logged out and no web-account password is available, so I did not bypass authentication or pretend deletion succeeded. `#pypiquotafix` now records the exact authorized range and login step; support request pypi/support#12214 remains open.

The added IntelliJ `fpe.md` wedge has converged without a manual disk overwrite: controller evidence records `write_applied`, `delivery_converged=true`, and terminal `committed` state for cycle `cycle-1789316971151` after the retained finalize resumed. The tab's document response is no longer stranded.

The earlier `lazily.md` Claude-to-Codex replacement is still a live reproducible issue, not falsely closed. Its response cycle is safely `abandoned`, but the healthy supervisor still exposes `agent: restart`, an old Claude child, and a ready/busy probe conflict. I preserved the active pane and updated existing backlog item `#restartlazilyverify` with this exact lifecycle state; it remains next in the queue after this prompt.

Changed paths: `.github/workflows/pypi.yml`, `.github/workflows/release.yml`, `.gitignore`, `AGENTS.md`, `Makefile`, `README.md`, `pyproject.toml`, `python/agent_doc_bootstrap/{__init__.py,__main__.py,cli.py}`, `python/tests/test_bootstrap.py`, `tests/test_cli.rs`, `VERSIONS.md`, `editors/release-parity.tsv`, and projected 0.35.383 Cargo/skill manifests. The dirty main source checkout was aligned to `f467696a9` while preserving all 27 pre-existing user changes.

Commands: `tsift status`; Python bootstrap unit tests; `uv build --wheel`; Python 3.8 grammar check; YAML parse; real GitHub archive/checksum extraction integration; clean-venv local HTTP first-run smoke; targeted Cargo regression; full `make check`; `make install-full`; Git commit/push and tag/push; `gh run list`; PyPI JSON inventory; authenticated-browser check; `agent-doc session-check`, `session status`, `session doctor`, `session history`, and scoped ops diagnosis for the two steered documents.

Verification: 5 Python tests passed; the produced wheel is `py3-none-any`, contains no native library or executable, and is 35,392 bytes; real archive verification extracted the expected native executable and sibling FFI library; clean installation invoked `agent-doc --version`; targeted Cargo coverage passed; full `make check` passed (`tart-cb9b428980`); installed CLI and FFI library both report 0.35.383; the release worktree is clean. CI run 34770263766, Release run 34770575670, and PyPI run 34770575683 were still in progress at closeout with no early failure, so local verification—not an unstarted remote runner—is the completion proof.

Confidence: high for the slim-package implementation, release workflow, local install, preserved dirty checkout, and `fpe.md` convergence. Escalation: PyPI deletion requires the operator to authenticate in Chromium; the live `lazily.md` harness replacement remains tracked under `#restartlazilyverify` and should not be force-cleared while its pane may still be active.

### Re: #restartlazilyverify — gpt-5 · 2026-09-13 13:34 EDT

Completed the live Claude-to-Codex replacement and verified the lifecycle beyond admission. The original failure was receipt 661 reaching `operator_ready` while the old Claude child still owned an empty preflight cycle; the supervisor then deferred the restart. Installed `agent-doc 0.35.383` already contains the narrow ready-boundary reclaim in `agent-doc-supervisor-io/src/ipc.rs`: it abandons only an empty `preflight_started` cycle and continues to protect captured/advanced work.

For the live repro, I restored `tasks/software/lazily.md` to `agent: codex` through its authoritative IntelliJ buffer and requested a normal, non-force restart. Receipt 665 completed the replacement: old Claude PID 1732363 exited, the existing healthy supervisor stayed PID 1732141, and live child PID 3139518 is `/home/brian/.npm-global/bin/codex -s danger-full-access`. Final status is `harness: codex`, actor `ready`, pane `alive-idle`, `prompt_ready=true`, `restart_count=1`, and no failed controller stage.

The separately reported IntelliJ `src/haiven-dev/tasks/fpe.md` tab was reconciled without editing its document. `agent-doc session-check` reports its retained cycle committed with `ui_outcome=no_drainable_work`; reopening/focusing the existing tab now shows `agent-doc: idle`, and session status confirms a healthy supervisor plus ready/alive-idle Codex pane.

Changed paths:
- `tasks/software/lazily.md`: frontmatter selector `agent: restart` -> `agent: codex` through the live editor buffer.
- No `src/agent-doc` source files were changed in this cycle; all 27 existing dirty paths were preserved.
- No `src/haiven-dev/tasks/fpe.md` content change.

Commands:
- `cargo test -p agent-doc-supervisor-io restart_agent_reclaims_empty_preflight_at_ready_boundary`
- `cargo test -p agent-doc-supervisor-io restart_agent_at_ready_boundary_still_protects_captured_cycle`
- `agent-doc session restart-agent tasks/software/lazily.md`
- `agent-doc session status tasks/software/lazily.md`
- `ps -p 1732363,3139518 -o pid=,ppid=,stat=,args=`
- `agent-doc session-check src/haiven-dev/tasks/fpe.md`
- `gh run list --workflow CI --limit 1`

Verification:
- Both targeted supervisor regression tests passed (1/1 each).
- Receipt 665 has `accepted_stage=operator_ready`, `failed_stage=none`; the old Claude PID is absent and the new Codex PID is live under the same supervisor.
- Latest CI run 34770263766 completed successfully.
- `fpe.md` session-check is OK and its IntelliJ status is idle.
- The transient malformed keystroke text introduced while restoring the live buffer was fully repaired; exact queue/backlog text was rechecked and `rg` found no remaining `keyReturn`, `keyHome`, `clearmodifiers`, `0Di`, or `Backlogsting` fragments.

Confidence: high. Escalation: none.

### Re: #reloadgateperprocess — gpt-5 · 2026-09-13T14:10-04:00

Released `agent-doc 0.35.384` with native-library reload admission scoped to the editor process. The invariant is now: a shared native generation for editor PID P may reload only after every document attributable to P, across every project endpoint, is non-blocking. `project_controller::native_reload` owns that policy; the RPC adapter owns evidence collection and effect delivery.

Transition coverage:

- Registered documents from multiple project endpoints are merged by PID, so one open cycle defers the process once.
- Independent editor PIDs retain independent decisions; a blocked process does not globally freeze unrelated editors.
- Empty registration projections use live PID sockets and attach otherwise-unattributed supervisor documents fail-closed.
- Supervisor-discovered project roots recover omissions from controller-process discovery; status-read and endpoint failures now emit named diagnostics rather than disappearing silently.
- When every attributed document is clear, one hot-reload intent is emitted per PID. A deferred marker is re-evaluated by the same global process planner at the supervisor idle boundary and moves to the project containing any remaining blocker.

The reactive topology is `controller/status + PID sockets + live supervisors -> per-PID document sets -> typed Publish/Defer decision -> one reload effect or pending marker -> idle re-evaluation`. The install command is the one-shot trigger; all retries remain lifecycle-driven.

Changed surfaces are `agent-doc-controller-io/src/project_controller/{native_reload,rpc}.rs`, the source-wiring regression in `tests/test_cli.rs`, `VERSIONS.md`, release parity metadata, and the 0.35.384 version projection. The JetBrains re-registration convergence issue `#jbreloadreregconverge` remains intentionally out of scope; plugin 0.2.383 is installed on disk, while the running IDE still reports 0.2.382 until it restarts.

Verification passed:

- `cargo test -p agent-doc-controller-io native_reload`: 10 passed.
- Targeted CLI source-wiring regression: passed.
- `make check`: 9,448 Rust tests, 179 VS Code tests, cross-editor simulation, release projection/parity, bootstrap tests, and all TLA+ models passed.
- `make install`: installed CLI/native library 0.35.384 and both editor packages. With admitted `cycle-1789322821240`, the live receipt was `delivered 0/1 ... 1 deferred mid-cycle`; ops recorded PID `2238596`, 19 attributed documents, three project endpoints, and this document as the blocker.

Source commit `1bebad974` and tag `v0.35.384` are pushed. Superproject release pointer/memory commit: `401d33478`. CI, PyPI, and GitHub Release workflows are running. Confidence is high; no escalation is needed. The exchange is large (518 lines), but the active queue remains drainable, so compaction stays deferred by policy.

### Re: #jbreloadreregconverge — gpt-5 · 2026-09-13T14:32:51-04:00

Implemented, verified, installed, committed, tagged, and pushed as agent-doc **0.35.385** / JetBrains plugin **0.2.384**.

The reload contract is now fail-closed at the document boundary:

- **Invariant:** every attached pre-handoff replica must have a JVM-owned encoded checkpoint before its native generation can unload, and every current open Markdown tab must produce a replacement-registration result before the reload gate is released.
- **Owner:** `CrdtReplicaManager` owns per-document checkpoint and restart proof; `NativeReloadCoordinator` owns application-wide ordering and refuses generation publication when capture/quiescence is incomplete.
- **Topology:** typed `reload_library` intent → coordinator gate → per-document serialized lanes → JVM-owned checkpoints → old-worker quiescence → native generation swap → awaited current-tab registration → exact convergence report → gate release. The typed install/reload intent remains the sole trigger; polling and a second handoff path were not added.

| Transition | Result |
|---|---|
| All attached lanes checkpoint | Workers may quiesce and the native generation may swap. |
| Any checkpoint is missing or times out | Existing managers are re-enabled and the old generation remains active. |
| Checkpoints exist but worker quiescence fails | No generation swap; captured state rebuilds the same generation in `finally`. |
| Reload completes | Every current open Markdown document is synchronously re-registered with `await=true` and `forceRefresh=true`. |
| A replacement does not attach | The report names the exact failed path; the existing retry path remains recovery rather than primary correctness. |
| Tabs close/open during handoff | Restart targets the current open-tab set, so closed documents are not resurrected and newly open Markdown tabs are included. |

Changed surfaces:

- `CrdtReplicaManager.kt`: per-lane checkpoint-before-dispose, fail-closed handoff state, awaited replacement registration, and deterministic exact-path restart reports.
- `NativeReloadCoordinator.kt`: typed handoff ownership, unload refusal on incomplete capture/quiescence, and convergence reporting before gate completion.
- `NativeReloadCoordinatorTest.kt` plus the VS Code cross-editor source-boundary test: checkpoint ordering, awaited attachment, exact failure reporting, and typed-call parity.
- Release metadata/projections: agent-doc 0.35.385, JetBrains 0.2.384, parity ledger, skill mirrors, and operator memory.

Verification and delivery:

- `./gradlew test --tests com.github.btakita.agentdoc.NativeReloadCoordinatorTest` — passed.
- `./gradlew test` — passed.
- `npm run compile && node --test out/reloadLibraryIntent.test.js` — 4/4 passed.
- `make check` — passed: 9,448/9,448 Rust tests, 179/179 VS Code tests, JetBrains/native cross-editor integration, release parity, bootstrap tests, and all model checks.
- `make install` — installed CLI/native 0.35.385, JetBrains package 0.2.384, and the VS Code package; the reload intent reached 1/1 available editor endpoint.
- Source commit `f82c538b1` and tag `v0.35.385` pushed to `btakita/agent-doc`; Release, PyPI, CI, Book, and Pages workflows started.
- Superproject release record commit `3e2cbe4a0`; the final document-response commit follows this response.

The new JetBrains package is installed on disk, but the running IDE still needs a restart to activate plugin 0.2.384. That live activation/proof remains intentionally routed through the separate `#jbplugin382restart` item; no IDE process was terminated during this cycle.

**Proof:** changed paths are the two JetBrains handoff owners, their focused test, the cross-editor reload-intent test, release metadata/version projections, and the focused reload memory. Confidence is high from the full green repository check and installed artifacts. Escalation: none; the only remaining operator-visible action is the already-tracked IDE restart verification.

### Re: #queuestartscopesilent — gpt-5 · 2026-09-13T14:47:41-04:00

Implemented, verified, installed, committed, tagged, and pushed as agent-doc **0.35.386**.

The silent state now has an explicit owner and reason. When `queue: start` leaves ready prompts for the supervisor but intentionally gives the attended in-session loop zero drainable heads, preflight still emits `queue_continuation_required: false`, but `queue_continuation_guidance` now says that the supervisor idle-watch owns the drain, the queue is neither stalled nor drained, and `queue: go` hands ownership to the in-session loop.

Architecture closeout:

- **Invariant:** every suppression with prompt work remaining names its reason; a zero in-session count cannot masquerade as an empty queue when supervisor activation is live.
- **Owner:** `agent-doc-queue::queue_continuation` owns the pure scope classification and canonical guidance; preflight owns serialization into the user-visible contract.
- **Transitions:** exchange-preemption and recycle-yield guidance retain precedence. Only an otherwise-guidanceless result falls back to supervisor-scope guidance, and only when in-session activation is absent, supervisor activation is present, and that activation still contains prompts.
- **Topology:** unchanged—this adds an observation to the existing queue-policy → preflight-projection path, with no new controller state, polling, or dispatch path.

Verification and delivery:

- Focused queue-policy regression passed.
- End-to-end preflight JSON regression passed with `drainable_head_count=0`, `continuation_required=false`, and non-null supervisor guidance.
- `make check` passed: 9,449/9,449 Rust tests plus all editor, parity, bootstrap, integration, and model checks.
- `make install` installed CLI/native 0.35.386 and rebuilt/installed editor artifacts. The active document correctly deferred its native reload until the next idle boundary.
- Source commit `89ada744e` and tag `v0.35.386` were pushed; CI, Release, PyPI, Book, and Pages workflows started. The latest earlier CI run was still in progress, so local full-suite evidence is the closeout authority.
- Superproject release record commit: `745e80a31`; the document response commit follows this response.

**Proof:** changed paths are `agent-doc-queue/src/queue_continuation.rs`, `agent-doc-preflight-command-io/src/run.rs`, release/version projections, and the parity ledger. Commands were the two focused tests, `rustfmt --check`, `make check`, `make install`, exact-path staging, commit/tag, and pushes. Confidence is high. Escalation: none.

### Re: #ocreverselistsplice — gpt-5 · 2026-09-13T15:05-04:00

> **Queue prompt:**
>
> 📌 do [#jbplugin382restart]


Fixed and released as agent-doc 0.35.387.

The provenance at `src/haiven-dev` commit `9b084e4` matches the constructed failure exactly: eight newly introduced list nodes were emitted in authored order, but every IPC insert named the same last-existing `after` anchor. Applying those patches sequentially inserted at one fixed boundary, so the run landed in exact reverse order. Repeated delivery explains the three identical copies; this was not the already-ruled-out document-cell merge, order weave, or region-restore path.

Architecture closeout:

- Owner: `agent_doc_ipc_protocol::build_ipc_node_patches_json` owns target-order encoding for node-patch batches.
- Invariant: each consecutive new sibling anchors to its immediate predecessor in the target, including a sibling introduced earlier in the same batch.
- Topology: patch builder → JSON parser → native markdown patch applier. Consumers remain literal and sequential; the producer now supplies an order-preserving dependency chain.

Changed paths:

- `agent-doc-ipc-protocol/src/lib.rs`: chain insert anchors through newly introduced siblings and add an eight-item builder-to-applier regression.
- `VERSIONS.md`, `editors/release-parity.tsv`, workspace manifests/lockfile, package metadata, and generated skill projections: release 0.35.387 parity.

Verification:

- The constructed regression failed before the fix and passed afterward.
- `cargo test -p agent-doc-ipc-protocol` passed.
- `make check` passed.
- `make install` passed; `agent-doc --version` reports `0.35.387`, the native reload intent delivered to the available editor endpoint, and safe idle recycling was scheduled.
- Latest prior GitHub CI run `34775701722` was still in progress when checked; local verification is complete.

Commits/releases:

- agent-doc source: `532a2b3d8` (`fix: preserve consecutive node patch order`), pushed to `main`.
- tag: `v0.35.387`, pushed.
- agent-loop projection: `52df0b058`, pushed to `main`.

Confidence: high. Escalation: none.
<!-- agent:boundary:330692a0:agent-doc-bugs -->
<!-- /agent:exchange -->

<!--
-->

t## Pending / Not Built

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority start -->
<!-- /agent:queue -->


<!-- agent:backlog priority -->
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
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#release035376verify] [release] Verify GitHub Release run 34737823202 completes green and v0.35.376 publishes all six platform assets; all matrix jobs were still externally queued at closeout.
- [/] [#cargovodtrim] [operator-verify] [P2][security] Trim the credential exposure from the livestream VOD once the operator supplies the VOD URL or platform/start-time locator. Known exposed interval: 2026-09-12T02:57:27.170Z through approximately 02:58:14Z.
- [/] [#fpecompactheal] [operator-verify] Confirm fpe.md's corrupted `<dynamic_context_ref>` block is gone after one compact on the FIXED binary (installed 0.35.367, ~/.cargo/bin/agent-doc at 23:01:39). Must be run FROM THE PANE THAT OWNS src/haiven-dev/tasks/fpe.md — that is a live session (b69e5aeb, `queue: go`) and this cycle deliberately did not hand-align its disk. Expected after `agent-doc compact src/haiven-dev/tasks/fpe.md`: exactly ONE block, with the two self-referential handles (absolute `.../src/haiven-dev/tasks/fpe.md` and relative `tasks/fpe.md`) gone, leaving at most the four instruction-file handles; and the `Prior summary/context:` line still summarising the prior compact list rather than quoting XML.
- [/] [#fixrunlazily] Run Agent Doc on lazily.md fails with `command-plane editor_route request failed via .agent-doc/controller.sock: Project Controller did not respond within 60000ms; the controller may be wedged`. NARROWED 2026-09-11, two hypotheses measured and DISPROVED — do not re-walk them. (1) The failure is the ACCEPT leg, not the work leg: the prefix `command-plane editor_route request failed` plus 60000ms is SOCKET_REQUEST_TIMEOUT_MS in sendAcceptedCommandSubmitToSocket; awaitCommandSubmitTerminal uses waitForReadySeconds*1000+grace instead. So the controller failed to ACKNOWLEDGE a command submit for 60s, and handle_editor_command_submit_async_rpc only enqueues. (2) NOT serialized accepts: rpc.rs:11185 listener.accept() spawns a thread per client. (3) NOT a slow pre-serve window: the socket binds at ListenerOptions::create_sync() (rpc.rs:11134) and the accept loop starts at 11184, so synchronous work between them leaves clients connected and hanging — the ss -lxp backlog signature — but the candidate, restore_reliable_sync_liveness, measures 0.08s for all 59,671 reliable_sync_liveness_journal rows / 6.5MB / 60,582 ops on this project, and its recovery effects are already spawned async. Live evidence at the failure window: ffi_ensure_controller_running_failed Connection refused (os error 111) x7 at 14:00:58-59, controller adopted 14:01:44, and a separate client still timing out at 14:02:23 (timed out after 5.0s waiting for project controller response) — unresponsive at least 40s after adoption. NEXT LEAD: what holds a lock across handle_request_locked (rpc.rs:12526) at runtime.bootstrap_snapshot() (12560) during a controller first seconds. Repro lever: kill the controller, then click Run Agent Doc while a replacement is launching.
- [/] [#pypiquotafix] [operator-auth] [P1] AUTHORIZED cleanup: after 0.35.383 thin wheel is live, sign into https://pypi.org/manage/project/agent-doc/releases/ in Chromium tab 0; then permanently delete the 34 oldest existing releases, 0.35.0 through 0.35.112 (all pre-2026-08-02), freeing about 1.75 GiB. Pinned PyPI installs of deleted versions will stop working. Browser is currently logged out; support request pypi/support#12214 remains open.
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
