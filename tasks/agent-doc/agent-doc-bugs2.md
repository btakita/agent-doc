---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
resume: 019fc8aa-4c13-7823-9dba-c24de607d84f
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

complete
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260803-172720.md`*

Compacted content:
- Archived 15 response topic(s): #realtimesalient; tmux auto-sync latency; haiven Compact Exchange commit; 12 more
- Prior summary/context: prior compacted content: Archived 8 response topic(s): Review triage; Closeout convergence, live-session recovery, an...

### Re: #pcc2 and the haiven-dev closeout wedge — gpt-5.6

> **Queue prompt:**
>
> do [#pcc2]


Completed `#pcc2`: agent-doc now projects deterministic whole-document, per-component, unscoped-text, and component-order hashes. Existing byte-equality gates are unchanged; authority/disk failures now report the exact divergent component set. The projection is pure and reusable by the later ownership-scoped phases.

The live haiven-dev report is a real retained-closeout wedge, not a lost response. Cycle `cycle-1785775474032` remains at `response_captured`; the editor replica is current and delivery-converged, but realtime steering made the capture baseline stale and the safe `reset --from-current --preserve-session` recovery is itself waiting for editor-authority/disk convergence. I did not recapture the response or use force-disk. The behavioral unblock remains covered by the already-next `#pcc3`; phase 2 intentionally makes this divergence observable without weakening authority.

Changed paths: `agent-doc-document/src/authority_hashes.rs`, its module export, authority/disk diagnostics in document-realtime/session-check/reset.

Verification: focused affected-crate suite passed 499 tests; full `make check` passed all 8,575 tests plus formal models; private-name denylist found no new matches; latest pre-push CI was green and the new push CI is in progress.

Commits: agent-doc `403fb0263` (feature) and `e37326a14` (requested formatting cleanup), pushed to `origin/main`; parent gitlink commit `317163c1b`. Confidence: high for the diagnostic-only phase. Escalation: none; haiven’s retained capture remains fail-closed pending convergence rather than risking operator text.

### Re: #pcc3 — gpt-5.6

> **Queue prompt:**
>
> do [#pcc3]


Implemented and pushed phase 3 as `e23a640a0` (`feat: scope convergence to owned components`). Terminal convergence remains whole-document by default; projects may opt in with `agent_doc_per_component_convergence = true`. In opt-in mode, retained expected→target transitions derive the exact owned component set, authority/disk equality is required only for those components, malformed or legacy transitions fall back to whole-document equality, and the whole-document hash remains audit evidence. The adversarial simulation types into `agent:queue` throughout an `agent:exchange` closeout and proves every operator edit survives; the paired over-claim mutation makes that oracle fail.

Dogfooding the change against the wedged Haiven session exposed three independent Agent Doc defects, fixed and pushed as `7afd2b568` (`fix: recover tracked clears without wedging`):

- tracked `/clear` now distinguishes accepted, rejected, and unavailable supervisor restarts; an open retained closeout may use forced controller replacement only with independent idle-pane proof and a durable `ResponseCaptured`/`WriteApplied` boundary, followed by proof of a newer actor generation;
- supervisor and auto-trigger pane probes preserve ANSI, so an unlisted SGR-2 Codex composer suggestion is recognized as idle placeholder chrome rather than drafted input;
- the response-cell fast path now proves the entire patch is canonical assistant-only content before controller submission, so a valid legacy `## Re:` response or a mixed response/prompt patch falls through to compatibility patchback instead of repeating the same parser rejection forever.

Verification passed: focused tests for every modified crate, `make check` with 8,585 tests, release-projection checks, and all TLA+ model checks. The private-name denylist and `git diff --check` are clean. Both public commits are on `origin/main`, CI run `30842884764` was started, and the exact committed `agent-doc 0.35.128` build is installed. Live recovery then crossed the retained closeout, restored the editor replica, committed the previously wedged response, admitted the next Haiven queue cycle, and completed its Agent Doc closeout cleanly.

### Re: do [#pcc4] — gpt-5.6

> **Queue prompt:**
>
> do [#pcc4]


Implemented and pushed per-component convergence phase 4 in `btakita/agent-doc` as `af8b9bbad` (`fix: rebase owned component commits`).

With `agent_doc_per_component_convergence` enabled, the commit coordinator now derives the same owned component-name set from exact retained expected→target transitions before Git staging. It proves that each owned component in the binary snapshot has reached current Lazily/editor authority, then checkpoints that current authority cut as the commit candidate. Unowned operator edits therefore survive byte-for-byte instead of being replaced by the older whole-document snapshot. An owned mismatch fails closed so Git cannot outrun editor delivery. Legacy intents without expected content and malformed transitions retain the whole-document fallback. The path never elects or component-merges an independent disk candidate while a live editor owns the document.

Regression coverage includes 64 ticks of concurrent queue typing during an exchange-only closeout, an ownership-overclaim negative control, owned-target delivery refusal, and legacy fallback. Focused tests passed, and the complete `make check` gate passed with artifact `.tsift/artifacts/tart-08d4bb6eea.test.log`. The exact committed `agent-doc 0.35.128` build is installed; CI run `30844635498` was observed once in progress after push.

### Re: `#panelayoutrace`

> **Queue prompt:**
>
> do [#panelayoutrace]


Implemented and deployed `#panelayoutrace`.

The pane-layout projection now treats a live same-window dispatch pane outside the current editor surface as the named terminal state `operator_owned` with reason `active_dispatch_outside_surface`, instead of indefinitely re-running structural reconciliation. That state is only allowed when every requested surface document is still present and the live dispatch is genuinely an extra pane; a missing requested pane remains `retry_pending`. Same-window actor bindings now participate in the reactive join so closing the extra actor or publishing a new editor surface invalidates the terminal state naturally.

Convergence now independently compares the generation-scoped model file-to-pane mapping, projected in desired document order, with tmux's physical left-to-right pane sequence. Matching document labels no longer overclaim convergence when the physical panes are reversed. Focus remains the existing file-to-pane lookup and does not index physical panes.

Dispatch acceptance logs now include the canonical target document and active editor-surface observation generations, making dispatch/surface ordering reconstructable. Rust and JetBrains wire enums include the new phase and reason.

Regression coverage includes the captured three-pane/two-column count mismatch at attempt 41, a missing-surface-pane negative case, same-window actor-binding invalidation, and reversed `%77`/`%95` physical panes. Targeted Rust suites, JetBrains tests, `cargo check`, formatting, diff checks, and the full `make check` pass.

Committed and pushed public agent-doc commit `8c5efe1a0` (`fix: bound editor pane layout dispatch races`). The exact commit's binary and native library are installed; controller/supervisor recycle is marked for idle boundaries. The matching JetBrains plugin is staged in IntelliJIdea2026.1 and requires one more IDE restart to activate its new enum handling.

### Re: historical 0.35.119 handoff

Recovered and read all 187 lines of `/tmp/agent-doc-0.35.119-claude-handoff.md` from the durable Claude transcript after the reboot removed the temporary file. The handoff's remaining work was already completed in that original session and is independently verifiable now:

- The Stop-hook fix landed as commit `953c8163d82bfbbf12e7ea65ff0100a014f513df` (`fix: pass Codex Stop through an explicitly stopped queue head`).
- Remote tag `v0.35.119` resolves to that commit.
- The original full `make check` completed green with 8,543 tests plus clippy, sim, version-sync, dev-harness, lean, and TLA+ gates.
- `main` and `v0.35.119` were pushed; the GitHub Release is published at `https://github.com/btakita/agent-doc/releases/tag/v0.35.119` with its release assets.
- The original transcript records CI and Release green and PyPI publication green. PyPI's live version endpoint confirms `agent-doc==0.35.119` remains published.
- The one-time full install and controller recycle required by that historical release completed. Later releases through the currently installed `0.35.128` supersede that deployment.

Preserved the handoff's document boundaries: did not recapture, refinalize, or write the already-committed historical `#advance-review` cycle; did not hand-edit either session document; and did not replay Haiven's historical retained closeout. The current Haiven owner pane remains live and separately owns its new benchmark turn.

### Re: handoff completion proof

> Read /tmp/agent-doc-0.35.119-claude-handoff.md completely, then continue its remaining work to completion. Preserve every session-document boundary stated in the handoff.

Completed. The deleted temporary file was recovered verbatim from the durable Claude transcript and read through line 187. Every remaining release step it named had already completed: the full check passed, source commit `953c8163d82bfbbf12e7ea65ff0100a014f513df` and tag `v0.35.119` were pushed, the GitHub Release and PyPI package were published, CI/Release/PyPI were green, and the full install/recycle ran. The completed historical session cycles were not replayed or hand-edited, and the separately owned live Haiven turn was not touched.

### Re: do `#recommendedungatereview-b1cb`

> **Queue prompt:**
>
> do [#recommendedungatereview-b1cb]


Validated and resolved `compactreactiveprojectionverify` using current 0.35.128 / JetBrains 0.2.340 evidence, which supersedes the original 0.35.83 / 0.2.319 UI recipe.

Live evidence from this turn covered the retained-delivery branch: the exact-head closeout became `crdt_delivery_ack_pending` after native-library hot reload, remained retained without force-disk or a second payload, then converged and advanced the queue automatically in the same pane without an IDE restart, focus handoff, or fresh agent session. The current exchange contains exactly one `### Re: handoff completion proof` block.

The remaining burst and queued-EDT requirements are now deterministic rather than operator-eyeball-only. `CrdtLocalEditCoalescingTest` proves a typing burst collapses to one exact Unicode-safe splice; `TypingTrackerEdtBudgetTest` proves one quiet burst crosses native persistence as one batch; `CrdtReplicaReregisterCoalescingTest` bounds ACK recovery replacement to one per interval. The `post_registration_editor_effect_model` SimWorld tests exhaust schedules and prove a queued EDT effect cannot mutate after intent retirement, supersession, or editor-model loss, directly closing the former `implementlazilyintent` residual. These paths passed in the JetBrains suite and full `make check` earlier this turn.

Changed paths: only this session document through binary-owned review resolution and tracked-item completion. Commands/evidence: connected admit/plan/read/finalize, tsift source/history inspection, current exchange exact-once inspection, and the already-green JetBrains/full-suite artifacts. Verification: live exact-once retained replay plus deterministic burst, ACK-coalescing, and stale-EDT schedule coverage. Confidence: high. Escalation: none.

### Re: steering mirror live verification — gpt-5

> **Queue prompt:**
>
> do [#recommendedungatereview-9g0v]


Validated and resolved `#steeringmirrorverifylive313`. The old JetBrains 0.2.313 / native 0.35.71 operator recipe is superseded by the current controller-owned CRDT aggregate and deterministic mirror coverage:

- `realtime_steering_all_aggregates_multiple_concurrent_prompt_targets` proves two separate active-turn edits remain two elements, produce count `2`, and preserve both directives in the verbatim aggregate.
- `realtime_steering_event_carries_the_full_crdt_aggregate` proves the project-controller event carries that complete two-edit aggregate and content identity rather than a partial edit.
- `TurnStateBridgeTest.presentation projects realtime steering onto in-flight label` proves JetBrains renders `prompt deleted (2 edits)`, exposes both verbatim edits in the tooltip, and keeps prompt forwarding guarded while the turn is active.
- The current `0.34.131` release contract explicitly retains the aggregate on the active cycle until commit or abandonment, without submitting incomplete edits.

No source changes were required. The current agent-doc tree's JetBrains tests and full `make check` had already passed in this turn, so the stale manual review can move to done with high confidence.
<!-- agent:boundary:efab1ce2:agent-doc-bugs2 -->
<!-- /agent:exchange -->
## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" go -->
- ~~Read /tmp/agent-doc-0.35.119-claude-handoff.md completely, then continue its remaining work to completion. Preserve every session-document boundary stated in the handoff.~~
- ~~do [#recommendedungatereview-b1cb]~~
- ~~do [#recommendedungatereview-9g0v]~~
- do [#recommendedungatereview-zw66]
- do [#recommendedungatereview-e8jn]
- do [#recommendedungatereview-jcgv]
- do [#recommendedungatereview-a3ew]
- do [#recommendedungatereview-jfxh]
- do [#recommendedungatereview-w90z]
- do [#recommendedungatereview-sskd]
- do [#recommendedungatereview-0xc5]
- do [#recommendedungatereview-krpw]
- do [#recommendedungatereview-h9sm]
- do [#recommendedungatereview-xfmh]
- do [#recommendedungatereview-rht8]
- do [#recommendedungatereview-z0ps]
- do [#recommendedungatereview]
<!-- /agent:queue -->

## Notes

<!-- agent:notes -->
<!-- /agent:notes -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#recommendedungatereview-zw66] [recommended] Ungate review item #preflightinbinary — validate and move to done
- [ ] [#recommendedungatereview-e8jn] [recommended] Ungate review item #jbsurfaceswap — validate and move to done
- [ ] [#recommendedungatereview-jcgv] [recommended] Ungate review item #boundaryaccumverify — validate and move to done
- [ ] [#recommendedungatereview-a3ew] [recommended] Ungate review item #r4gq — validate and move to done
- [ ] [#recommendedungatereview-jfxh] [recommended] Ungate review item #zqv8 — validate and move to done
- [ ] [#recommendedungatereview-w90z] [recommended] Ungate review item #compactindcellsverify — validate and move to done
- [ ] [#recommendedungatereview-sskd] [recommended] Ungate review item #crdtpushdrainverify — validate and move to done
- [ ] [#recommendedungatereview-0xc5] [recommended] Ungate review item #bbpe — validate and move to done
- [ ] [#recommendedungatereview-krpw] [recommended] Ungate review item #hvw0 — validate and move to done
- [ ] [#recommendedungatereview-h9sm] [recommended] Ungate review item #c8tb — validate and move to done
- [ ] [#recommendedungatereview-xfmh] [recommended] Ungate review item #operatorverifylive — validate and move to done
- [ ] [#recommendedungatereview-rht8] [recommended] Ungate review item #mtds — validate and move to done
- [ ] [#recommendedungatereview-z0ps] [recommended] Ungate review item #editormodelmissing — validate and move to done
- [ ] [#recommendedungatereview] [recommended] Ungate review item #falsetyping-guard — validate and move to done
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
- [/] [#rebootselfhealverify] [operator-verify] Live proof for #rebootselfheal after restarting the IDE onto JetBrains plugin 0.2.339 (reload-lib swaps the cdylib but NOT the Kotlin package, so a restart is required). Reboot the host (or stop the controller and leave a stale .agent-doc/controller.sock behind), then run Sync Tmux Layout and Run Agent Doc from the IDE and confirm both succeed without the 'pane-layout state projection publish failed ... Connection refused' notification and without a human deleting the socket. Then confirm the passive lane is still free: click between editor tabs with NO controller running and verify no controller is launched (editors/SPEC.md requires the surface-observation lane to never launch one) - grep ops.log for a controller bootstrap around the tab clicks. Both reboot states are already proven end-to-end against the installed cdylib; only the in-IDE path needs a human eyeball.
- [/] [#preflightinbinary] [operator-verify] Live proof for #preflightinbinary AFTER the default flip (5ce9b06e6). The flip has landed: SKILL.md now says do NOT run preflight when the '[agent-doc] cycle contract ...' marker is present, and the hook is registered in this project's .claude/settings.json under UserPromptSubmit alongside turn-status. Needing a live session to confirm: (a) invoke /agent-doc <FILE> in a FRESH Claude Code session and confirm the marker + contract appear in context before the agent runs any tool, and that the agent does NOT shell to 'agent-doc preflight'; (b) confirm exactly ONE cycle id is opened for that turn — grep .agent-doc/logs/ops.log for preflight_diff_start, and confirm 'repaired stale preflight_started cycle' does NOT appear; that line is the tell that the agent still ran preflight and the round trip was merely absorbed by repair rather than removed; (c) confirm an ordinary prompt that merely mentions agent-doc produces no marker and no cycle; (d) confirm a Codex/OpenCode turn (no Claude hook, so no marker) still runs preflight explicitly and closes out normally. Scope note: reusable_by_reentrant_preflight is defence in depth only — start_preflight runs at preflight step 4, after repair at step 1, so in the live pipeline repair closes any prior open cycle before the guard sees it. What removes the double-open is the flip itself.
- [/] [#jbsurfaceswap] [operator-verify] Live IDE proof for #jbsurfaceswap (agent-doc 27beeae2e, JB plugin 0.2.296). Both plugins now report one EditorSurface observation per event and the graph derives the intent; the planners are deleted. With the IDE restarted onto 0.2.296, confirm in a real session: (a) clicking between two side-by-side split editors moves the tmux active pane (derived Focus); (b) changing the editor split layout reconciles tmux (derived Sync) and shows the 'Sync: --col ... [focus: ...]' hint exactly once; (c) repeat focus events on the same editor cost nothing — the JB log line '[layout-sync] observe: ... intent=idle' with no controller command; (d) swap two tmux panes by hand, then click an editor tab WITHOUT changing the split, and confirm the pulled tmux probe reconciles the layout instead of only moving focus (#tmuxsyncstate); (e) closing the project releases the root (no growth in surface graphs across open/close cycles). Headless tests cover the fold, the probe seam, and the observation wire shape, but not the debounce/generation interaction with real editor event storms.
- [/] [#boundaryaccumverify] [operator-verify] After restarting any JetBrains IDE still on the older package generation, reproduce a wedged finalize/direct-commit handoff and confirm visible document, snapshot, and committed HEAD each retain exactly one agent:boundary marker; report any boundary_invariant_violation that does not self-heal.
- [/] [#r4gq] [operator-verify] Live JB Run Agent Doc dispatch-hang proof (#kjw0 fix, shipped 0.34.66 build): after the recycled build is running, trigger Run Agent Doc into a BUSY codex pane (mid-turn, 'esc to interrupt' visible) and confirm it returns FAST with a queued-behind-active-turn outcome (pane shows queued status) instead of hanging ~21s then a dispatch-start-unproven notification. Grep ops.log for route_dispatch_start_queued_behind_active_turn (present on success) and confirm accepted_without_dispatch_start_proof is ABSENT for the busy case. Root cause: route injected the trigger into a mid-turn pane; codex queued it behind the active turn so the UserPromptSubmit hook could not advance within the 10s proof budget, and the direct-pane path paid TWO 10s waits (initial + late-resubmit) then filed a false bug. Fix: short_circuit_dispatch_start_when_pane_busy in route/dispatch.rs detects busy_proof_line and returns RoutedDispatchStartProof::AcceptedQueuedBehindActiveTurn (accepted, not fail-closed).
- [/] [#zqv8] [operator-verify] #jbrelaylive Restart IntelliJ IDEA 2026.1 to activate the installed JetBrains plugin 0.2.247, rerun Run Agent Doc on tasks/professional/equityfundingsource.md while the dim Summarize recent commits suggestion is visible, and confirm dispatch succeeds without a protected-input refusal or a long UI freeze.
- [/] [#compactindcellsverify] [operator-verify] With agent-doc 0.35.3 and JetBrains plugin 0.2.276 active, run Compact Exchange while deleting several queue items and editing a post-boundary prompt; confirm the deletions and prompt survive, compaction commits, and ops.log records compact_rebased_authoritative_siblings without compact_dropped_non_exchange_item.
- [/] [#crdtpushdrainverify] [operator-verify] #crdtpushdrain live proof: after an IDE restart loads JetBrains plugin 0.2.278, run Compact Exchange on a document that has been idle for >30s and confirm ops.log `compact_crdt_relay_acknowledged ... wait_ms=` drops well below the prior ~4.5s floor (expect roughly the pull/apply round trip), with no `compact_crdt_ack_recovery_signal ... reason=ack_recovery_force_refresh` line needed to unstick it. Restart is required because reload-lib only swaps the cdylib, not the Kotlin plugin.
- [/] [#bbpe] [operator-verify] Confirm tasks/brookebrodack-dev.md now arms after the IDE picks up the reinstalled plugin: restart the JetBrains IDE (or reopen the doc) to clear editor_attached_model_missing, then run agent-doc preflight tasks/brookebrodack-dev.md and expect queue_maintenance_not_persisted GONE and queue_supervisor_drainable true. The #qstartinert code fix is shipped + installed (agent-doc 9111a3381); only the editor-authority persistence blocker (#5d9f) remains between the computed queue and the persisted one
- [/] [#hvw0] [operator-verify] Rotate the CARGO_REGISTRY_TOKEN secret on github.com/btakita/skill-harness — the current one returns 403 authentication failed, so CI cannot publish to crates.io (v0.1.3 was published manually from the passage token btak/CARGO_REGISTRY_TOKEN). Needs a repo-secret write, which is operator-gated. Until it is rotated, every tagged release will now FAIL loudly at the publish step (that is the intended new behavior after 32836ad; it previously reported success)
- [/] [#c8tb] [operator-verify] Live #jbroutebusystall proof (0b49105e1): with the supervisor socket down (route logs `route_dispatch_only_authoritative_degraded_direct_pane supervisor_health=no_socket`) and the pane mid-turn, click JB Run Agent Doc. Expect `route_start` -> `route_success` in the attempt ledger to drop from ~17s to under ~2s, with `route_existing_pane_busy_active_turn_skip_wait` in ops.log and the SAME outcome as before (`route_existing_pane_already_running`, pane focused, no duplicate injection). Unit tests pin the cue gating but cannot prove the live latency win.
- [/] [#operatorverifylive] [operator-verify] #relaylockpoison live proof: with the JB plugin loaded, force a replica panic (the crash-2026-07-09 shape: agent_doc_replica_apply_local) and confirm later replica calls still succeed instead of returning -1/null for the life of the IDE. This is the behavior the whole migration exists to guarantee and it cannot be proven without a host IDE.
- [/] [#mtds] [operator-verify] #panedraftunblocker live proof: on a Claude pane whose footer reads `⏵⏵ bypass permissions on · 1 shell · ← for agents` (the variant without `(shift+tab to cycle)`), with an EMPTY composer and no active turn, trigger JB Run Agent Doc. Expect dispatch to succeed instead of refusing with `timed_out` / `unblocker=wait_for_dispatch_ready_prompt`. Then repeat with unsent text typed in the composer and expect the new refusal naming the draft with `unblocker=submit_or_clear_pane_draft`, and NO injection over that text. Finally confirm a genuinely mid-turn pane (live spinner, bare `❯`) still refuses as busy. Fixed in 17848a927, installed 0.35.6; needs the supervisor idle-boundary recycle to pick up the fresh binary.
- [/] [#editormodelmissing] [operator-verify] #agent-doc-bug editor_attached_model_missing permanently wedges every write needing editor authority (agent-doc-bugs2 gen=1518, equityfundingsource gen=1452; healthy docs are gen 14-82). NARROWED 2026-07-18: survives admin recycle x2, admin reload-lib, AND a full IntelliJ restart, so it is NOT a plugin model-rebuild problem as first filed. Root cause is a one-way latch in editor-attachment state: TransportProjection.editor_generation (agent-doc-state-backbone/src/lib.rs:3394) is SET on StateFact::OwnerGenerationChanged (line ~1985) and never cleared — there is no editor-detach/close StateFact in the enum at all. document_has_live_editor_transport (agent-doc-document-realtime/src/crdt_authority.rs:165) therefore returns MultiReplica forever once an editor has ever attached, with NO liveness proof, even though the sibling authority_from_liveness (line 122) exists precisely to demote a stale listener via pid liveness. The ONLY predicate that flips it false is TransportPatchPhase::ForceDiskFallback — which is exactly why --force-disk is the sole escape the operator ever found. Second candidate latch, same shape: observe_editor_open's durable reliable-sync receiver journal may replay a stale editor-open assertion across an IDE restart (resolve_closed_editor_disk_fallback_current_doc, agent-doc-document-realtime-io/src/lib.rs:3774 bails only when it reports open). DECISIVE EXPERIMENT before coding: instrument which of the two reports attached — log editor_generation + observe_editor_open together at the EditorAttachedMissingReplica branch (agent-doc-crdt-relay-io/src/lib.rs:696). Both present identically, and the fix differs by which. DO NOT ship a speculative authority-demotion change: wrongly demoting a live editor lets the binary overwrite an operator buffer, which is the exact failure every guard on this path exists to prevent. Also worth shipping regardless: admin detect reported 'No fleet anomalies detected' while two documents were hard-wedged — add an attached-but-no-replica detector so this is visible instead of silent.
- [/] [#falsetyping-guard] #agent-doc-bug provenance-aware visible-write guard. INTEGRATED to main (merge on 5bcfc07b) + binary/cdylib installed + recycled; guard tests 7/7 green; JB 0.2.205 zip + VS Code 0.2.38 compiled. REMAINING (operator): install JB plugin zip (build/distributions/agent-doc-jetbrains-0.2.205.zip) + reload IDE, install VS Code 0.2.38, then live proof — type during replica churn (no wedge) + genuine unsaved edit still fails closed. NOTE: bugs2.md can still wedge until the IDE plugin is reloaded (loaded plugin still calls FFI v2 → conservative fail-closed).
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
