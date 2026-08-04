---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
resume: 019fcb26-742b-7ee3-b4b9-b4166e561551
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
queue: stop
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

### Re: do [#recommendedungatereview-0xc5] — gpt-5

> **Queue prompt:**
>
> do [#recommendedungatereview-0xc5]


Validated and resolved [#bbpe] against the restarted JetBrains editor and the freshly reloaded Agent Doc library.

- The live JetBrains 0.2.340 registration now has a functioning CRDT replica; the prior `editor_attached_model_missing` failure is absent.
- `agent-doc preflight tasks/brookebrodack-dev.md --probe` completed without `queue_maintenance_not_persisted`.
- The current preflight schema reports `queue_active: true`, `queue_trigger: auto`, and `queue_drainable_head_count: 12`. That explicit nonzero drainable-head count is the current equivalent of the older `queue_supervisor_drainable: true` expectation.
- The probe was inspection-only and did not open a cycle or modify the document.

The operator verification is complete, so [#bbpe] and [#recommendedungatereview-0xc5] can move to done.

Validated and resolved [#bbpe] against the restarted JetBrains editor and the freshly reloaded Agent Doc library.

- The live JetBrains 0.2.340 registration now has a functioning CRDT replica; the prior `editor_attached_model_missing` failure is absent.
- `agent-doc preflight tasks/brookebrodack-dev.md --probe` completed without `queue_maintenance_not_persisted`.
- The current preflight schema reports `queue_active: true`, `queue_trigger: auto`, and `queue_drainable_head_count: 12`. That explicit nonzero drainable-head count is the current equivalent of the older `queue_supervisor_drainable: true` expectation.
- The probe was inspection-only and did not open a cycle or modify the document.

The operator verification is complete, so [#bbpe] and [#recommendedungatereview-0xc5] can move to done.
### Re: do [#recommendedungatereview-0xc5] — gpt-5

### Re: do [#recommendedungatereview-krpw] — gpt-5

> **Queue prompt:**
>
> do [#recommendedungatereview-krpw]


Validated [#hvw0], but it cannot move to done yet.

- `gh secret list --repo btakita/skill-harness` reports `CARGO_REGISTRY_TOKEN` was last updated on 2026-04-05, before the recorded failure.
- The latest Release run for v0.1.3 still shows `cargo publish` receiving `403 Forbidden: authentication failed`.
- There is no later tagged release proving that the repository secret was rotated successfully.

Rotating a GitHub repository secret is an external operator-gated action and was not authorized by the current Agent Doc task. [#hvw0] therefore remains active as `[operator-verify]`; [#recommendedungatereview-krpw] is complete as an evidence-backed review of its current state.

Validated [#hvw0], but it cannot move to done yet.

- `gh secret list --repo btakita/skill-harness` reports `CARGO_REGISTRY_TOKEN` was last updated on 2026-04-05, before the recorded failure.
- The latest Release run for v0.1.3 still shows `cargo publish` receiving `403 Forbidden: authentication failed`.
- There is no later tagged release proving that the repository secret was rotated successfully.

Rotating a GitHub repository secret is an external operator-gated action and was not authorized by the current Agent Doc task. [#hvw0] therefore remains active as `[operator-verify]`; [#recommendedungatereview-krpw] is complete as an evidence-backed review of its current state.
### Re: do [#recommendedungatereview-krpw] — gpt-5

### Re: do [#recommendedungatereview-h9sm] — gpt-5

> **Queue prompt:**
>
> do [#recommendedungatereview-h9sm]


Validated [#c8tb], but the exact operator proof is not present yet.
 
The ops log contains a live fast-path event for the healthy authoritative-actor branch: `route_dispatch_only_busy_active_turn_skip_wait` on a genuinely busy Claude pane. It does not contain the required degraded-route pair `route_dispatch_only_authoritative_degraded_direct_pane supervisor_health=no_socket` plus `route_existing_pane_busy_active_turn_skip_wait`.

Producing that missing evidence requires intentionally taking a live supervisor socket down and invoking JetBrains Run Agent Doc while its pane is mid-turn. I did not disrupt a live supervisor or synthesize UI evidence. [#c8tb] therefore remains active as `[operator-verify]`; [#recommendedungatereview-h9sm] is complete as an evidence-backed review of its current state.

### Re: do [#recommendedungatereview-xfmh] — gpt-5

> **Queue prompt:**
>
> do [#recommendedungatereview-xfmh]


Validated [#operatorverifylive], but its required live fault-injection proof is not available.

The current JetBrains logs contain no `agent_doc_replica_apply_local` panic, replica lock-poisoning event, or subsequent successful replica-call sequence that could establish the requested recovery behavior. Producing that evidence requires deliberately forcing a replica panic in the live IDE, which is disruptive and was not authorized by the current task.

I did not manufacture the fault or infer recovery from ordinary replica traffic. [#operatorverifylive] remains active as `[operator-verify]`; [#recommendedungatereview-xfmh] is complete as an evidence-backed review of its current state.

Validated [#operatorverifylive], but its required live fault-injection proof is not available.

The current JetBrains logs contain no `agent_doc_replica_apply_local` panic, replica lock-poisoning event, or subsequent successful replica-call sequence that could establish the requested recovery behavior. Producing that evidence requires deliberately forcing a replica panic in the live IDE, which is disruptive and was not authorized by the current task.

I did not manufacture the fault or infer recovery from ordinary replica traffic. [#operatorverifylive] remains active as `[operator-verify]`; [#recommendedungatereview-xfmh] is complete as an evidence-backed review of its current state.

### Re: do [#recommendedungatereview-xfmh] — gpt-5

### Re: do [#recommendedungatereview-rht8] — gpt-5

Validated [#mtds], but the required three-case live proof is not available.

The current panes and ops logs do not contain a complete sequence for the exact Claude footer variant: empty-composer dispatch success, unsent-draft refusal with `submit_or_clear_pane_draft` and no injection, and genuine mid-turn busy refusal. Producing that evidence would require commandeering a suitable live Claude pane and deliberately injecting both submitted and unsubmitted test input.

I did not interfere with an unrelated live pane or infer the UI result from unit coverage. [#mtds] remains active as `[operator-verify]`; [#recommendedungatereview-rht8] is complete as an evidence-backed review of its current state.

### Re: do [#recommendedungatereview-z0ps] — gpt-5

Validated and resolved [#editormodelmissing], [#jetbrainsrunroute-77k5], and [#jetbrainsrunroute] with live JetBrains evidence.

- After the actual IDE restart, the new JetBrains process (PID 3017541) registered `/home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/haiven-dev.md` as an exact CRDT replica under plugin 0.2.340 and projected the retained write after exact editor registration.
- Reliable-sync status now reports the document as live with the required lossless-tree CRDT capabilities.
- Retrying the dispatch-only route activated the existing queue head, sent it to pane `%4`, and returned `pane_state_changed` dispatch-start proof.
- A pane capture shows the submitted Agent Doc prompt, assistant tool output, and the Codex `Working` busy cue, so this is execution-start proof rather than pane-input acceptance alone.
- The root `agent-doc-bugs2.md` missing-replica path also recovered after the rebuilt plugin/shared library was activated and has completed multiple editor-authoritative closeouts.
- No forced disk fallback or disk-read authority bypass was used; the exact live-editor replica supplied the authority.

The permanent missing-replica wedge and the false-negative JetBrains/Codex route proof are therefore both resolved in the live reproductions.

### Re: do [#recommendedungatereview] — gpt-5

Validated [#falsetyping-guard] as far as the available live evidence permits, but left the operator review open.

- The implementation remains integrated, its guard tests were previously green, and the restarted IDE is now running a much newer JetBrains plugin (0.2.340 rather than the 0.2.205 build named by the original review).
- The current session supplies positive live evidence for the replica-churn half: exact CRDT replicas re-registered after restart, retained writes projected, repeated editor-authoritative Agent Doc closeouts completed, and the `haiven-dev.md` route entered a working Codex turn without a wedge.
- A repository evidence search found the implemented `unsaved_buffer_ahead_of_disk` classification and regression coverage, but no captured live operator proof for the required second half: a genuine unsaved editor edit must still fail closed.

Because deliberately creating an unsaved edit in the operator’s live IDE would mutate editor state and could interfere with active work, I did not simulate that proof. [#falsetyping-guard] therefore remains open for the final human live check; only this recommendation item is complete.

Verification: `tsift --envelope search 'falsetyping-guard genuine unsaved edit fails closed visible-write guard'`. Changed paths: none in this cycle. Confidence: high that the churn/no-wedge half is satisfied; insufficient evidence to resolve the unsaved-edit half.

### Re: first one-to-two pane auto-sync — gpt-5.6

> **Queue prompt:**
>
> Moving from fpe-service-topology.md with 1 tmux pane to agent-doc-bugs2.md does not auto-sync back to 2 tmux panes. Forward navigation to lazily.md then auto-syncs back to 2 tmux panes. It seems the first move from 1 -> 2 tmux panes is not autosynced.

Fixed and released as agent-doc `0.35.129`.

The defect was an interstitial JetBrains projection: `selectedFiles` had already advanced to `agent-doc-bugs2.md`, while the detected editor columns still described `fpe-service-topology.md`. Readiness checked only the selected/visible list, and selection settling stopped once that list contained the new document, so the mixed one-column projection reached the controller. The following navigation supplied another event and incidentally repaired it.

`SelectionProjectionSettling` remains the policy owner. A surface is now publishable only when both selected files and detected layout columns contain the preferred document. Its bounded later-EDT reconciliation repairs visible files and layout columns independently, replacing the prior document only where stale evidence exists and otherwise failing closed. The reactive topology remains selection event → generation-fenced observation → bounded EDT surface read → pure settled projection → controller Source/Computed graph → tmux Effect/receipt; the only imperative exception is the existing bounded reread needed because IntelliJ may expose this interstitial state without another layout event.

Regression coverage reproduces the exact transition: selected files advance, layout stays stale, readiness waits, settling exhausts, and the final projection preserves the current visible selection while repairing the stale layout. The paired mixed-projection assertion would fail under the previous readiness rule. Specs now explicitly require selected/layout coherence before publication.

Changed paths: `EditorTabSyncListener.kt`, `EditorTabSyncListenerTest.kt`, `editors/SPEC.md`, `editors/jetbrains/SPEC.md`, `VERSIONS.md`, and the mechanical 0.35.129 release projection across manifests/lockfiles/installed skill copies.

Verification: the focused JetBrains test was red before the fix and green afterward; private-name denylist and `git diff --check` were clean; full `make check` passed 8,596 tests with 244 skipped plus editor, clippy, simulation, version-sync, and TLA+ gates. The exact committed build reports `agent-doc 0.35.129`; its JetBrains package was installed into four detected IDE installations. Public commit `848e58a70` and tag `v0.35.129` are pushed; parent gitlink commit `d7f95bb37` is pushed. CI run `30880394366`, Release run `30880398285`, and PyPI run `30880398283` were observed once in progress.

Confidence: high. Escalation: no code or release blocker. A non-blocking operator review records the one remaining live step: restart JetBrains to activate plugin 0.2.340, then repeat the original one-pane → two-column navigation and confirm the first selection restores both tmux panes.

### Re: stranded response replay heading compact failure — gpt-5.6

> **Queue prompt:**
>
> Fix "agent-doc command failed (exit 1): Error: project controller command `compact_document` failed: [integrity-gate] INTERRUPTED: response heading `### Re: do [#recommendedungatereview-0xc5] — gpt-5` in /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md has no response body; prompt/response ordering was interrupted [dogfood] ACTIONABLE_AGENT_DOC_FIX_PROMPT issue_class=interrupted_closeout issue_key=c3ca409e1d2b8a91c170585dd64afd055632bf159af8818c5563822996a237bf Agent Doc did not complete this turn successfully. Diagnose and fix the underlying Agent Doc defect before treating a retry as the remedy. Preserve any retained capture and obey no-resubmit/no-recycle guidance in the original diagnostic."

Fixed and released as agent-doc `0.35.130`.

The integrity gate was correctly refusing a malformed exchange; the producer/recovery defect was earlier. Response replay already knew how to move a body that landed immediately before its matching tail heading, but `materialize_response_in_current_exchange` returned early when an earlier complete copy of the response was present. That left the later heading-only replay shell stranded. Once subsequent responses followed it, the tail-only repair could no longer reach it, and `compact_document` failed closed.

`agent_doc_turn::response_replay` is now the single policy owner for this shape. Before response materialization or compact integrity validation, a pure canonicalizer removes only an empty `### Re:` heading whose normalized topic already has an earlier response with a real body. It preserves every body, prompt, comment, transient marker, and unrelated response byte. A unique empty heading, a non-empty duplicate, or any other malformed structure remains untouched and still interrupts before tags, archives, snapshots, writes, or commits. The topology is authoritative document Source → pure replay-shell canonicalizer → integrity Computed → compact/materialization Effect; no imperative recovery loop, disk election, force-disk path, or controller recycle was added.

Regression coverage reproduces the historical document shape: a complete response, duplicated body, matching empty `(HEAD)` shell, and a later response. The test was red because the early exact-content return preserved two same-topic headings, then green after canonicalization. Compact integration proves the same authority now passes the gate and archives normally. A paired negative test proves a genuinely unanswered unique heading still fails closed. Focused `agent-doc-turn` and `agent-doc-compact-io` suites passed 355 tests plus retained-write ownership guards.

Changed paths: `agent-doc-turn/src/response_replay.rs`, `agent-doc-compact-io/src/lib.rs`, `specs/07-closeout-commands.md`, `VERSIONS.md`, and the mechanical 0.35.130 release projection across manifests, lockfiles, and skill copies.

Verification: private-name denylist, `git diff --check`, and `cargo fmt --check` were clean. Full `make check` passed all 8,599 tests with 244 skipped, release projection, editor/build checks, simulations, clippy, and TLA+ models. The exact committed build reports `agent-doc 0.35.130` and is installed. Public commit `1f1aa2dbb` and tag `v0.35.130` are pushed; CI `30881343676`, Release `30881344222`, and PyPI `30881344223` were observed once in progress. Parent gitlink commit `4fc035095` is isolated locally but not pushed because four unrelated Agent Doc session commits already precede it on local `main` and publishing those other sessions was outside this task.

The File Cache Conflict reported during installation is being handled under the existing retained-write contract: keep the editor buffer with **Cancel / Keep memory changes**, then save or make one editor mutation so the plugin publishes the current cut. I did not force disk, recapture the response, or issue another controller recycle. A non-blocking operator review records that live convergence/compact confirmation.

Confidence: high for the code and deterministic compact behavior. Escalation: only the currently visible JetBrains conflict decision and editor-cut publication; the binary-owned closeout remains exact-once if that ACK is still pending.

### Re: Response

Implemented and live-verified the underlying fixes.

The architecture is now reactive at the equal-content boundary. A real document advance still has one imperative effect boundary and one causal ACK because the controller must distinguish “intended” from “actually rendered by this replica/generation.” Once canonical content already equals the rebased retained target, however, that state is now a Lazily fixed point: the controller derives `TargetVisible`, emits no CP write, creates no new delivery generation, and waits for no new ACK. The relay independently rejects equal-content CP writes without moving the acknowledged delivery frontier. Imperative republish was therefore a recovery artifact, not the intended steady-state model.

The related causes were fixed together:

- Retained targets are revalidated and rebased against current editor authority. Already-visible rebased content converges reactively, preventing retained-version loops and duplicate queue materialization.
- A replacement controller now returns typed `missing_replica` invalidation when an attached plugin pulls without a registered member. JetBrains and VS Code both interpret that as a re-registration request, fixing the post-install/recycle wedge where a successful-looking refused envelope was parsed as an empty delta.
- The handoff reaper now uses the authoritative current Preparing/Promoted handoff clock. Immutable predecessor argv is discovery metadata only, so a fresh successor is not kept in `preparing` because an old process name appears stale.
- Automatic editor-surface projection is autostart-capable for documents actually visible in the restored/open layout. Background open documents remain excluded, and passive sync does not steal editor focus. This removes the hard-coded `no_autostart` behavior that made `Sync Tmux Layout` necessary after startup, file-open, restoration, or editor switching.
- Codex route recognition ignores terminal blank padding below a valid busy/working footer, covering the `#jetbrainsrunroute`, `#jetbrainsrunroute-8x66`, and `#jetbrainsrunroute-tgp5` failure shape.
- JetBrains now captures exact `DocumentEvent` splices and treats explicit editor reloads as non-operator provenance. Whole-buffer snapshots remain only a compatibility/fallback seam; changed cells/splices are the canonical mutation input to the reactive document tree.
- “Compatibility bridge” means the narrow editor-native event/wire seam between JetBrains or VS Code and the controller-owned CRDT/Lazily graph. It is not a second state authority.
- JetBrains and VS Code implement the same missing-member recovery and passive projection contracts. JetBrains package version `0.2.342` was built successfully but deliberately not installed over the active IDE: the structural Kotlin change requires the documented uninstall/restart/install/restart sequence, and the open buffers were preserved.

Protobuf support is justified for cross-language schema identity, evolution, and deterministic interop—not as a replacement for CRDT merge or Lazily dependency semantics. The plan is in `tasks/software/plan-lazily-protobuf-interop.md`, and `#lzprotobufinterop` was added once to the `lazily.md` backlog.

Live rollout installed the final core binary and matching native library without a second controller recycle. `haiven-dev.md` then resumed the retained closeout and reached `committed` without force-disk, resubmission, or a duplicate response. Controller inspection reports the installed inode active for both controller and supervisor and `projection_lag: false`.

Verification: Rust focused regressions plus full `make check` passed (8,610 passed, 244 skipped, TLA+/Lean green); VS Code passed 170 tests; JetBrains `test buildPlugin` passed; formatting passed. Source commits are `fa1a82524` (`fix: converge reactive editor recovery`) and root pointer `4248af9c6` (`agent-doc: converge retained editor recovery`). The latest visible GitHub CI run was green; it predates these local commits.
<!-- agent:boundary:52f9ac8b:agent-doc-bugs2 -->
<!-- /agent:exchange -->
## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" go -->
- [#jetbrainsrunroute-tgp5] + [#jetbrainsrunroute]
<!-- /agent:queue -->

## Notes

<!-- agent:notes -->
<!-- /agent:notes -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#jetbrainsrunroute-tgp5] JetBrains Run Agent Doc route/dispatch failed after bounded submit/start proof retries #jbrunautobug #agent-doc-bug failure_class=accepted_without_dispatch_start_proof document=/home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/haiven-dev.md stage=dispatch_only_dispatch_start_proof pane=%4 actor_generation=64 editor_attempt_id=1785826444942-1 dispatch_proof_state=accepted_without_dispatch_start_proof elapsed_ms=10000 diagnostic_path=none ops_log_path=/home/brian/work/btakita/agent-loop/src/haiven-dev/.agent-doc/logs/ops.log ops_log_marker=route_submit_issue(issue=accepted_without_dispatch_start_proof,phase=dispatch_only_dispatch_start_proof,result=accepted_without_dispatch_start_proof) [symptom-key invariant=run_agent_doc_route_dispatch_failure document=f29648cc93e0994425112e2fe48ff16f5cfa3385ceba1f9caf6615869a088474 component=route/dispatch_only_dispatch_start_proof content_hash=sha256:bc25eab815f07e720570f10455d1ead69008f5a5866d315b61e418e3d65345e5]
- [ ] [#jetbrainsrunroute] JetBrains Run Agent Doc route/dispatch failed after bounded submit/start proof retries #jbrunautobug #agent-doc-bug failure_class=accepted_without_dispatch_start_proof document=/home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/haiven-dev.md stage=dispatch_start_proof pane=%4 actor_generation=64 editor_attempt_id=1785826444942-1 dispatch_proof_state=none elapsed_ms=10123 diagnostic_path=/home/brian/work/btakita/agent-loop/src/haiven-dev/.agent-doc/logs/route-submit/1785826458430-direct_pane_dispatch_start_unproven-codex-_4-46f9c2a9a7af.txt ops_log_path=/home/brian/work/btakita/agent-loop/src/haiven-dev/.agent-doc/logs/ops.log ops_log_marker=route_submit_issue(issue=accepted_without_dispatch_start_proof,phase=dispatch_start_proof,result=accepted_without_dispatch_start_proof) [symptom-key invariant=run_agent_doc_route_dispatch_failure document=f29648cc93e0994425112e2fe48ff16f5cfa3385ceba1f9caf6615869a088474 component=route/dispatch_start_proof content_hash=sha256:5a24ed8c602e44e6ae77bb259405cf299b29f143e06cf10c3729df7780105181]
- [ ] [#jetbrainsrunroute-1ypa] JetBrains Run Agent Doc route/dispatch failed after bounded submit/start proof retries #jbrunautobug #agent-doc-bug failure_class=accepted_without_dispatch_start_pr((oof document=/home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/haiven-dev.md stage=dispatch_only_dispatch_start_proof pane=%4 actor_generation=64 editor_attempt_id=1785823625376-1 dispatch_proof_state=accepted_without_dispatch_start_proof elapsed_ms=10000 diagnostic_path=none ops_log_path=/home/brian/work/btakita/agent-loop/src/haiven-dev/.agent-doc/logs/ops.log ops_log_marker=route_submit_issue(issue=accepted_without_dispatch_start_proof,phase=dispatch_only_dispatch_start_proof,result=accepted_without_dispatch_start_proof) [symptom-key invariant=run_agent_doc_route_dispatch_failure document=f29648cc93e0994425112e2fe48ff16f5cfa3385ceba1f9caf6615869a088474 component=route/dispatch_only_dispatch_start_proof content_hash=sha256:bc25eab815f07e720570f10455d1ead69008f5a5866d315b61e418e3d65345e5]
- [ ] [#jetbrainsrunroute] JetBrains Run Agent Doc route/dispatch failed after bounded submit/start proof retries #jbrunautobug #agent-doc-bug failure_class=accepted_without_dispatch_start_proof document=/home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/haiven-dev.md stage=dispatch_start_proof pane=%4 actor_generation=64 editor_attempt_id=1785823625376-1 dispatch_proof_state=none elapsed_ms=10108 diagnostic_path=/home/brian/work/btakita/agent-loop/src/haiven-dev/.agent-doc/logs/route-submit/1785823639344-direct_pane_dispatch_start_unproven-codex-_4-89ec4bd30e9c.txt ops_log_path=/home/brian/work/btakita/agent-loop/src/haiven-dev/.agent-doc/logs/ops.log ops_log_marker=route_submit_issue(issue=accepted_without_dispatch_start_proof,phase=dispatch_start_proof,result=accepted_without_dispatch_start_proof) [symptom-key invariant=run_agent_doc_route_dispatch_failure document=f29648cc93e0994425112e2fe48ff16f5cfa3385ceba1f9caf6615869a088474 component=route/dispatch_start_proof content_hash=sha256:5a24ed8c602e44e6ae77bb259405cf299b29f143e06cf10c3729df7780105181]
- [ ] [#jetbrainsrunroute-8x66] JetBrains Run Agent Doc route/dispatch failed after bounded submit/start proof retries #jbrunautobug #agent-doc-bug failure_class=accepted_without_dispatch_start_proof document=/home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/haiven-dev.md stage=dispatch_only_dispatch_start_proof pane=%4 actor_generation=64 editor_attempt_id=1785819598771-2 dispatch_proof_state=accepted_without_dispatch_start_proof elapsed_ms=10000 diagnostic_path=none ops_log_path=/home/brian/work/btakita/agent-loop/src/haiven-dev/.agent-doc/logs/ops.log ops_log_marker=route_submit_issue(issue=accepted_without_dispatch_start_proof,phase=dispatch_only_dispatch_start_proof,result=accepted_without_dispatch_start_proof) [symptom-key invariant=run_agent_doc_route_dispatch_failure document=f29648cc93e0994425112e2fe48ff16f5cfa3385ceba1f9caf6615869a088474 component=route/dispatch_only_dispatch_start_proof content_hash=sha256:bc25eab815f07e720570f10455d1ead69008f5a5866d315b61e418e3d65345e5]
- [ ] [#jetbrainsrunroute] JetBrains Run Agent Doc route/dispatch failed after bounded submit/start proof retries #jbrunautobug #agent-doc-bug failure_class=accepted_without_dispatch_start_proof document=/home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/haiven-dev.md stage=dispatch_start_proof pane=%4 actor_generation=64 editor_attempt_id=1785819598771-2 dispatch_proof_state=none elapsed_ms=10131 diagnostic_path=/home/brian/work/btakita/agent-loop/src/haiven-dev/.agent-doc/logs/route-submit/1785819612774-direct_pane_dispatch_start_unproven-codex-_4-f38d339871ae.txt ops_log_path=/home/brian/work/btakita/agent-loop/src/haiven-dev/.agent-doc/logs/ops.log ops_log_marker=route_submit_issue(issue=accepted_without_dispatch_start_proof,phase=dispatch_start_proof,result=accepted_without_dispatch_start_proof) [symptom-key invariant=run_agent_doc_route_dispatch_failure document=f29648cc93e0994425112e2fe48ff16f5cfa3385ceba1f9caf6615869a088474 component=route/dispatch_start_proof content_hash=sha256:5a24ed8c602e44e6ae77bb259405cf299b29f143e06cf10c3729df7780105181]
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
- [/] [#operatorverifyresolve] [operator-verify] Resolve the current JetBrains File Cache Conflict with Cancel / Keep memory changes, then save or make one editor mutation so the plugin publishes the current editor cut; confirm the retained closeout commits and a later Compact Exchange no longer fails on the historical stranded duplicate heading.
- [/] [#operatorverifyrestart] [operator-verify] Restart a running JetBrains IDE to activate plugin 0.2.340, then move from fpe-service-topology.md in a one-pane surface to agent-doc-bugs2.md in its two-column surface and confirm the first selection restores two tmux panes without a second navigation.
- [/] [#rebootselfhealverify] [operator-verify] Live proof for #rebootselfheal after restarting the IDE onto JetBrains plugin 0.2.339 (reload-lib swaps the cdylib but NOT the Kotlin package, so a restart is required). Reboot the host (or stop the controller and leave a stale .agent-doc/controller.sock behind), then run Sync Tmux Layout and Run Agent Doc from the IDE and confirm both succeed without the 'pane-layout state projection publish failed ... Connection refused' notification and without a human deleting the socket. Then confirm the passive lane is still free: click between editor tabs with NO controller running and verify no controller is launched (editors/SPEC.md requires the surface-observation lane to never launch one) - grep ops.log for a controller bootstrap around the tab clicks. Both reboot states are already proven end-to-end against the installed cdylib; only the in-IDE path needs a human eyeball.
- [/] [#hvw0] [operator-verify] Rotate the CARGO_REGISTRY_TOKEN secret on github.com/btakita/skill-harness — the current one returns 403 authentication failed, so CI cannot publish to crates.io (v0.1.3 was published manually from the passage token btak/CARGO_REGISTRY_TOKEN). Needs a repo-secret write, which is operator-gated. Until it is rotated, every tagged release will now FAIL loudly at the publish step (that is the intended new behavior after 32836ad; it previously reported success)
- [/] [#c8tb] [operator-verify] Live #jbroutebusystall proof (0b49105e1): with the supervisor socket down (route logs `route_dispatch_only_authoritative_degraded_direct_pane supervisor_health=no_socket`) and the pane mid-turn, click JB Run Agent Doc. Expect `route_start` -> `route_success` in the attempt ledger to drop from ~17s to under ~2s, with `route_existing_pane_busy_active_turn_skip_wait` in ops.log and the SAME outcome as before (`route_existing_pane_already_running`, pane focused, no duplicate injection). Unit tests pin the cue gating but cannot prove the live latency win.
- [/] [#operatorverifylive] [operator-verify] #relaylockpoison live proof: with the JB plugin loaded, force a replica panic (the crash-2026-07-09 shape: agent_doc_replica_apply_local) and confirm later replica calls still succeed instead of returning -1/null for the life of the IDE. This is the behavior the whole migration exists to guarantee and it cannot be proven without a host IDE.
- [/] [#mtds] [operator-verify] #panedraftunblocker live proof: on a Claude pane whose footer reads `⏵⏵ bypass permissions on · 1 shell · ← for agents` (the variant without `(shift+tab to cycle)`), with an EMPTY composer and no active turn, trigger JB Run Agent Doc. Expect dispatch to succeed instead of refusing with `timed_out` / `unblocker=wait_for_dispatch_ready_prompt`. Then repeat with unsent text typed in the composer and expect the new refusal naming the draft with `unblocker=submit_or_clear_pane_draft`, and NO injection over that text. Finally confirm a genuinely mid-turn pane (live spinner, bare `❯`) still refuses as busy. Fixed in 17848a927, installed 0.35.6; needs the supervisor idle-boundary recycle to pick up the fresh binary.
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
