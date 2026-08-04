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

### Re: binary-owned preflight admission — gpt-5

> **Queue prompt:**
>
> do [#recommendedungatereview-zw66]


Validated and resolved `#preflightinbinary`. Its original four-part live recipe predates the current admission protocol and is now covered more strongly:

- The binary hook emits the explicit `cycle contract (preflight already ran in the binary...)` marker only after recognizing a real harness-native Agent Doc trigger; parser tests prove a prompt that merely mentions the command is a no-op and does not resolve a session document.
- Hook installation is idempotently tested to install exactly one Claude `UserPromptSubmit` admission command while preserving existing hooks.
- Direct run admission is tested to open one `preflight_started` cycle through `realtime_admit`, return `preflight_required=false`, and emit no `preflight_diff_start` or maintenance/repair side effects.
- The connected MCP integration contract pins admit → plan/read → finalize to the original cycle id and explicitly proves no `repaired stale preflight_started` recovery.
- This live cycle itself admitted as `cycle-1785813153721` with `maintenance_required=false` and `preflight_required=false`, without a model-owned preflight call.

No source changes were required. The current full `make check` was already green in this turn. The obsolete manual review can move to done with high confidence.

### Re: JetBrains editor-surface swap verification — gpt-5

> **Queue prompt:**
>
> do [#recommendedungatereview-e8jn]


Validated and resolved `#jbsurfaceswap`. The 0.2.296 manual checklist is superseded by the current controller-owned editor-surface protocol and substantially stronger coverage:

- JetBrains tests prove document selection publishes one latest-wins observation, ordinary focus does not force a competing full reconcile, layout changes retain selected-document intent until acceptance, the short structural coalescing window is bounded, and controller submission happens off the EDT.
- The reactive surface graph proves first-layout `Sync`, changed-document `Focus`, identical observations becoming silent/idle, returning layouts firing again, and tmux drift alone driving reconciliation with no new editor event.
- Controller generation tests reject duplicate, stale, retired, and late-after-forget observations while allowing the next plugin generation.
- Registry tests prove project roots are independent, `forget` discards history, and `forget_all` disposes every local effect.
- Current pane-layout tests additionally prove stale generations cannot apply focus, automatic structural edges cannot reuse a prior generation, physical pane order is independently checked, and an active dispatch pane outside the surface reaches terminal `operator_owned` rather than retrying forever.

The old “Sync hint exactly once” observation is no longer the authority boundary: plugins report facts and do not plan tmux consequences or derive receipts locally. This turn also exercised the live JetBrains/Haiven route that exposed the newer pane-layout race; the fix is committed and pushed, and the current JetBrains tests plus full `make check` passed. Confidence is high; no operator escalation remains for this legacy review.

### Re: boundary accumulation verification — gpt-5

> **Queue prompt:**
>
> do [#recommendedungatereview-jcgv]


Validated and resolved `#boundaryaccumverify` using both the live wedge recovered in this turn and current invariant coverage:

- A retained finalize was genuinely wedged behind a missing editor replica, then converged after the native/plugin recovery without recapturing or resending the response. The recovered response appears exactly once.
- The current visible document and committed `HEAD` each contain exactly one standalone `agent:boundary` marker.
- The document's ops log contains no `boundary_invariant_violation` for `agent-doc-bugs2.md`.
- `reposition_collapses_snapshot_boundaries_even_during_active_run` constructs the exact wedged/direct-commit shape with three stale snapshot boundaries and an active retained response, then proves the binary snapshot collapses to exactly one boundary.
- The post-commit guard independently inspects committed `HEAD`, self-heals any count above one through a single collapse commit, re-verifies the result, and fails closed if collapse cannot produce exactly one marker.

No source changes were needed. The current full `make check` was already green in this turn. Confidence is high; the old restart-and-reproduce checklist is now satisfied by the live recovery plus deterministic snapshot/commit coverage.

### Re: busy Codex Run Agent Doc dispatch — gpt-5

> **Queue prompt:**
>
> do [#recommendedungatereview-a3ew]


Validated and resolved `#r4gq` with the requested live evidence on the current implementation:

- During this turn, JetBrains `Run Agent Doc` targeted `/src/haiven-dev/tasks/haiven-dev.md` while Codex pane `%4` was visibly busy (`Working ... esc to interrupt`). The Haiven ops log records `route_dispatch_start_queued_behind_active_turn` at `2026-08-03T16:06:07Z` for that exact pane and document.
- The same log contains no `accepted_without_dispatch_start_proof` entry for `haiven-dev.md`; the rerun was accepted as queued behind the active turn instead of producing the historical false dispatch-start failure.
- `short_circuit_dispatch_start_when_pane_busy` gives a busy pane only the short proof probe, then emits the queued outcome and diagnostic rather than paying the former double 10-second wait.
- `queued_behind_active_turn_is_accepted_even_when_proof_required` proves that outcome remains accepted for Codex's proof-required path, and `busy_dispatch_start_outcome_short_circuits_queued_when_busy_without_probe_proof` pins the no-proof busy case directly.

The owner pane subsequently drained and completed its Haiven work. No source changes were needed for this review; the current full `make check` was already green in this turn. Confidence is high and no operator escalation remains.

### Re: JetBrains dim-suggestion relay verification — gpt-5

> **Queue prompt:**
>
> do [#recommendedungatereview-jfxh]


Validated and resolved `#zqv8`. The exact historical input shape is now deterministic and the current IDE path was exercised live:

- `codex_dim_suggestion_is_not_protected_but_identical_operator_draft_is` feeds the ANSI-dim `Summarize recent commits` composer suggestion to the direct-pane classifier and proves it is not protected; the identical non-dim operator draft remains protected as `drafted prompt input`.
- `protected_prompt_input_reason_ignores_unlisted_dim_codex_suggestion` independently proves the dim suggestion makes the current composer visible/ready without being mistaken for an approved literal prompt line.
- The route path preserves SGR attributes specifically so that distinction survives pane capture.
- During this turn, repeated JetBrains `Run Agent Doc` actions against `haiven-dev.md` entered the controller route. A representative current attempt (`1785781244900-9`) started at `18:20:44Z` and settled with exit code 0 at `18:21:00Z`; the available log contains no protected-input block for the document. The UI remained responsive while the controller-owned route ran asynchronously.

No source changes were needed. The current JetBrains tests and full `make check` were already green in this turn. Confidence is high; no restart or operator escalation remains for the obsolete 0.2.247 review.

### Re: Compact Exchange independent-cell verification — gpt-5

> **Queue prompt:**
>
> do [#recommendedungatereview-w90z]


Validated and resolved `#compactindcellsverify`. The old 0.35.3 / JetBrains 0.2.276 live recipe is superseded by exact semantic-cell regression coverage:

- `compact_targets_rebase_onto_concurrent_sibling_queue_deletions` compacts the exchange while two queue items are concurrently deleted, then proves both the live and committed targets retain the compacted exchange, retain the untouched queue item, and do not resurrect either deletion.
- `partial_compact_preserves_trailing_prompt_after_boundary` and `full_exchange_compact_preserves_trailing_prompt_after_boundary` prove a post-boundary prompt survives in the visible exchange while remaining live drift rather than being committed into the snapshot or archived.
- `assert_non_exchange_items_preserved_fails_closed_on_dropped_backlog_item` proves any accidental loss of a non-exchange item is rejected with the `#compactdropitem` diagnostic rather than silently committing.
- The production rebase path emits `compact_rebased_authoritative_siblings` when the authoritative sibling-cell rebase is used; the drop guard emits `compact_dropped_non_exchange_item` only on a fail-closed violation.

No source changes were required. The current full `make check` was already green in this turn. These tests exercise the exact data race more deterministically than an operator timing reproduction, so confidence is high and no operator escalation remains.

> **Queue prompt:**
>
> do [#recommendedungatereview-sskd]

Resolved the review using the current deterministic proof, which supersedes the stale plugin-0.2.278 log procedure.

The requested `compact_crdt_relay_acknowledged` and `compact_crdt_ack_recovery_signal` markers no longer exist in the current write path, so their timing assertion cannot be reproduced meaningfully. The invariant they were intended to validate is now enforced directly in three places:

- `PatchWatcher` routes every controller-published CRDT frontier through `requestUrgentRemoteDrain`, bypassing speculative no-op backoff without replacing editor authority.
- `requestUrgentRemoteDrain` resets `consecutiveNoOpReschedules` after useful work, preventing the next controller push from inheriting an idle document's prior backoff.
- Route startup re-requests the urgent drain every 750 ms while delivery remains pending and still fails closed at its maximum wait.

Regression coverage verifies each condition in `CrdtReplicaProjectionFrontierTest` and `route_startup_retries_urgent_delivery_drain_while_delivery_stays_pending`. The full `make check` and JetBrains test suite passed earlier in this session on the pushed source state.

Changed paths: none for this review cycle. Commands: targeted source/test inspection with `tsift`, exact current-log searches with `rg`, and repository status checks. Verification: current implementation and regression tests match the intended no-backoff behavior; `src/agent-doc` is clean and aligned with `origin/main`. Push: no new source commit was needed; the previously implemented agent-doc fixes are already pushed. Confidence: high. Escalation: none.
### Re: do [#recommendedungatereview-sskd] — gpt-5

### Re: do [#recommendedungatereview-sskd] — gpt-5

> **Queue prompt:**
>
> do [#recommendedungatereview-sskd]

Resolved the review using the current deterministic proof, which supersedes the stale plugin-0.2.278 log procedure.

The requested `compact_crdt_relay_acknowledged` and `compact_crdt_ack_recovery_signal` markers no longer exist in the current write path, so their timing assertion cannot be reproduced meaningfully. The invariant they were intended to validate is now enforced directly in three places:

- `PatchWatcher` routes every controller-published CRDT frontier through `requestUrgentRemoteDrain`, bypassing speculative no-op backoff without replacing editor authority.
- `requestUrgentRemoteDrain` resets `consecutiveNoOpReschedules` after useful work, preventing the next controller push from inheriting an idle document's prior backoff.
- Route startup re-requests the urgent drain every 750 ms while delivery remains pending and still fails closed at its maximum wait.

Regression coverage verifies each condition in `CrdtReplicaProjectionFrontierTest` and `route_startup_retries_urgent_delivery_drain_while_delivery_stays_pending`. The full `make check` and JetBrains test suite passed earlier in this session on the pushed source state.

Changed paths: none for this review cycle. Commands: targeted source/test inspection with `tsift`, exact current-log searches with `rg`, and repository status checks. Verification: current implementation and regression tests match the intended no-backoff behavior; `src/agent-doc` is clean and aligned with `origin/main`. Push: no new source commit was needed; the previously implemented agent-doc fixes are already pushed. Confidence: high. Escalation: none.
<!-- agent:boundary:9903e408:agent-doc-bugs2 -->
<!-- /agent:exchange -->
## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" go -->
- ~~Read /tmp/agent-doc-0.35.119-claude-handoff.md completely, then continue its remaining work to completion. Preserve every session-document boundary stated in the handoff.~~
- ~~do [#recommendedungatereview-b1cb]~~
- ~~do [#recommendedungatereview-9g0v]~~
- ~~do [#recommendedungatereview-zw66]~~
- ~~do [#recommendedungatereview-e8jn]~~
- ~~do [#recommendedungatereview-jcgv]~~
- ~~do [#recommendedungatereview-a3ew]~~
- ~~do [#recommendedungatereview-jfxh]~~
- ~~do [#recommendedungatereview-w90z]~~
- ~~do [#recommendedungatereview-sskd]~~
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
- [ ] [#jetbrainsrunroute-77k5] JetBrains Run Agent Doc route/dispatch failed after bounded submit/start proof retries #jbrunautobug #agent-doc-bug failure_class=accepted_without_dispatch_start_proof document=/home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/haiven-dev.md stage=dispatch_only_dispatch_start_proof pane=%4 actor_generation=64 editor_attempt_id=1785815283483-1 dispatch_proof_state=accepted_without_dispatch_start_proof elapsed_ms=10000 diagnostic_path=none ops_log_path=/home/brian/work/btakita/agent-loop/src/haiven-dev/.agent-doc/logs/ops.log ops_log_marker=route_submit_issue(issue=accepted_without_dispatch_start_proof,phase=dispatch_only_dispatch_start_proof,result=accepted_without_dispatch_start_proof) [symptom-key invariant=run_agent_doc_route_dispatch_failure document=f29648cc93e0994425112e2fe48ff16f5cfa3385ceba1f9caf6615869a088474 component=route/dispatch_only_dispatch_start_proof content_hash=sha256:bc25eab815f07e720570f10455d1ead69008f5a5866d315b61e418e3d65345e5]
- [ ] [#jetbrainsrunroute] JetBrains Run Agent Doc route/dispatch failed after bounded submit/start proof retries #jbrunautobug #agent-doc-bug failure_class=accepted_without_dispatch_start_proof document=/home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/haiven-dev.md stage=dispatch_start_proof pane=%4 actor_generation=64 editor_attempt_id=1785815283483-1 dispatch_proof_state=none elapsed_ms=10057 diagnostic_path=/home/brian/work/btakita/agent-loop/src/haiven-dev/.agent-doc/logs/route-submit/1785815300877-direct_pane_dispatch_start_unproven-codex-_4-34369eb6bbf2.txt ops_log_path=/home/brian/work/btakita/agent-loop/src/haiven-dev/.agent-doc/logs/ops.log ops_log_marker=route_submit_issue(issue=accepted_without_dispatch_start_proof,phase=dispatch_start_proof,result=accepted_without_dispatch_start_proof) [symptom-key invariant=run_agent_doc_route_dispatch_failure document=f29648cc93e0994425112e2fe48ff16f5cfa3385ceba1f9caf6615869a088474 component=route/dispatch_start_proof content_hash=sha256:5a24ed8c602e44e6ae77bb259405cf299b29f143e06cf10c3729df7780105181]
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
