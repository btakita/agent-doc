---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
resume: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
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

Installed Agent Doc 0.35.109 with reactive per-document authority, bounded Tokio scheduling, live Compact Exchange validation, and closeout coalescing; Monster Rodholders remains retained pending the post-install plugin restart.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260730-231401.md`*

Compacted content:
- Archived 3 response topic(s): Response; Response; Response

### Re: completed queue items and closeout churn — gpt-5

> **Queue prompt:**
>
> equityfundingsource.md has queue items for already done tasks. Please investigate why agent-doc did not strike the done items and whether the cause(s) were fixed in previous versions or not. Fix all remaining contributing factors.

Fixed and committed all remaining code-side contributors I found.

The queue cleanup had three independent gaps: it was a best-effort imperative callback that write and commit callers could miss; the controller Computed could be created before the document-authority key existed and therefore miss the later SourceMap insertion; and short free-text heads such as `staging deploy` were rejected by an unrelated four-word baseline threshold even when the captured response explicitly echoed the exact queue prompt. The old commit-seam callback and its dead capture path are now removed.

Answered free-text cleanup is now a pure projection of durable captured response + current authoritative markdown. The controller owns that Computed, subscribes to both value and dependency-slot membership, and admits one exact target to the document's detached actor worker. The slow editor/CRDT convergence and follow-up commit stay off the controller reactive thread; unrelated invalidations cannot enqueue the same projection twice. SQLite is not used to decide the hot-path projection.

I also added a semantic corruption boundary for the Monster Rod Holders failure shape: captured conversation text such as `> **Queue prompt:**` cannot be welded into tracked review/backlog/queue/done components. Both editor delivery and commit preserve the last valid authority when that structure is rejected.

Dogfood mode is now document-configurable with `agent_doc_dogfood: true` (and alias `dogfood_mode`). Terminal Agent Doc failures—including open-cycle wedges, dependency unavailability, timeouts, interruptions, and projection failures—append a stable `ACTIONABLE_AGENT_DOC_FIX_PROMPT` with an issue class/key so the dogfood turn is explicitly prompted to repair the product. `agent_doc_dogfood: false` disables the legacy inferred dogfood behavior. The native binary owns this policy and editor plugins surface its result, preserving plugin parity.

Verification is green: `make check` passed all 8,453 tests, release synchronization, static checks, and every TLA+ model. The focused tests cover the Equity Funding Source short-head case, capture-before-authority ordering, corruption rejection, and dogfood classification. Agent Doc commit: `3d8a83be6`; superproject pointer commit: `cb48b93e8`. The latest prior CI run was successful (`30580926037`), so I did not wait for a new run.

I did not install or recycle processes: the retained `tsift.md` editor-delivery capture explicitly forbids install/recycle until its owner reaches a terminal boundary. I queued the install plus live Equity Funding Source, Monster Rod Holders, Compact Exchange, and editor/tmux verification behind that exact dependency rather than disturbing the durable capture.

### Re: JetBrains native subscription CPU churn — gpt-5

> **Queue prompt:**
>
> Idea is periodically non-responsive. Please fix thrashing in the plugin UI process. Be more efficient with calculation and move CPU heavy/bound tasks into another thread. Ensure plugin parity.

I reproduced the churn against the restarted IDEA process. CPU sampling showed several native `agent-doc-editor-reactive` threads consuming cores while IDEA was otherwise idle; the Kotlin thread dump did not show Agent Doc work running on the EDT. The expensive work was already out of band, but the native document-authority subscriptions could reconnect in a tight loop.

The contributing defect was in the shared reactive authority stream: after a controller socket closed or was interrupted, every open document immediately reconnected with a cold cursor (`after_version = 0`, no controller generation). A stale or one-shot controller could therefore return the same projection and close again, multiplying redundant controller launches, decoding, and reactive invalidations across all open documents.

Fixed in the shared native editor authority layer:

- reconnects now resume from the last Lazily controller-generation/version cursor;
- duplicate same-cursor projections no longer invalidate dependents;
- one compatibility reconnect is allowed, then a second no-progress disconnect projects the authority as unavailable instead of becoming a CPU-bound retry loop;
- a real controller-generation or authority-version change reactivates the subscription;
- the path remains socket/event driven—no sleep or polling loop was added.

Because this is in the shared Rust/native authority stream rather than JetBrains-specific Kotlin code, the editor integrations receive the same behavior and retain plugin parity. CPU-heavy controller work remains off the IDEA UI thread.

Verification is green:

- focused authority reconnect tests pass, including cursor resume, no-progress churn termination, and controller-generation reactivation;
- full `make check` passes;
- latest completed CI run `30580926037` is green;
- source commit: `5413a80e0` (`fix: stop document authority reconnect churn`);
- superproject pointer commit: `45b3991cb` (`chore: update agent-doc reconnect guard`).

I did not install, recycle, or restart processes in this cycle. The retained `tsift.md` editor-delivery capture still owns that boundary. Existing gated backlog item `#reactive-closeout-install-verify` covers installing these commits and verifying editor/tmux responsiveness after that capture reaches a terminal state.

### Re: Open-tab authority readiness and editor/tmux latency — gpt-5

> **Queue prompt:**
> Run Agent Doc takes several seconds to complete. Editor -> Tmux Pane autosync performance has improved but is still slow at times. Can we have authority reactives ready for all open editor tabs...even tabs that are not currently active? Please improve performance to all issues.

Yes. The current implementation already projects every open Markdown tab into the authority warm set at plugin startup and on layout changes, with the focused document first, nearby tabs next, and a bounded 256-document capacity. Each document owns a cancellable Tokio task on a shared runtime; idle subscriptions do not consume an OS thread. The remaining live slowdown was a lifecycle fault rather than missing prewarming.

The IDEA process had four native-library generations mapped at once. A mapped generation was described as inert after hot reload, but native quiescence only stopped IPC listeners and CRDT replicas. It did not cancel that generation's open-document authority streams, queued editor surfaces, Tokio work, or Lazily Effects. Each retained mapping could therefore keep an entire duplicate reactive plane alive, explaining periodic UI CPU pressure, accumulated focus latency, and cold-looking authority behavior despite the open-tab warm set.

Fixed in `3fa0fa9d8` (`fix: quiesce retired editor reactives`):

- Native reload quiescence now rejects late editor ingress, cancels every per-document authority membership, clears queued surfaces, and unsubscribes every per-root Lazily Effect before retiring the generation.
- A mapped retired library is now genuinely idle even when the loader cannot unmap it.
- The reload coordinator republishes the complete open-tab surface after handoff, so focused, adjacent, and inactive open documents are prewarmed immediately in the winning generation.
- Replacement-load failure explicitly resumes the retained generation and its reactive ingress instead of leaving it silently quiesced.
- Shared native editor-surface logic supplies plugin parity; the JetBrains-only addition is the hot-reload handoff that JetBrains requires.
- Tests prove retired generations cancel authority subscriptions, reject recreated work, and stop all generation-owned Effects.

Verification passed:

- `cargo test -p agent-doc-editor-surface-io`
- full `make check` (`tart-777fa50a6e`)
- `git diff --check`
- latest three completed CI runs were green; latest was run `30580926037`

The Agent Doc source commit is `3fa0fa9d8`; the superproject pointer commit is `28cd57635`. I did not install, recycle, or restart IDEA because the existing retained closeout/install gate owns that boundary. Live verification should occur after that gated install; this cycle did not disturb the retained capture.

### Re: Behavior-descriptive abstraction names — gpt-5

> **Queue prompt:**
> Abstraction names should be descriptive instead of using project names. Please audit all abstraction names, particularly test names.

Audited Rust and Kotlin declarations for names derived from the documents/projects where bugs were first observed. One real violation remained in the write-runtime regression suite:

- `TsiftDuplicateContentFixture` → `DuplicateResponseBodyRedeliveryFixture`
- `tsift_md_duplicate_content_corruption_fixture` → `duplicate_response_body_redelivery_fixture`
- `tsift_md_duplicate_content_fixture_skips_stale_full_document_redelivery` → `stale_full_document_redelivery_does_not_overwrite_live_typing`

The historical document path and payload remain inside the fixture as concrete reproduction data; they no longer name the reusable abstraction or the test. The remaining `Tsift*` and Lazily declarations are actual integration/domain APIs (context collection, graph invocation, reactive receipts), not names borrowed from a customer or session document.

Verification passed:

- declaration audit has no remaining project/document-derived identifier matches
- focused renamed regression passed (`tart-1bbcf1d7f3`)
- full `make check` passed (`tart-8b1bc7f513`)
- `git diff --check`
- latest completed CI remains green (`30580926037`)

Committed as `9d1afd8a9` (`test: name redelivery regression by behavior`), with superproject pointer `18ea90954`.
> **Queue prompt:** Please audit the hotpath + startup paths to ensure the systems are fully reactive. Any imperative operations should be replace by reactive operations if possible. Consolidate any "split-brain" login into cohesive reactives. Convert any magic strings or magic values into enums or consts.

### Re: Reactive hot path and startup audit — gpt-5

Implemented and committed the audit as source commit `6d0bf4090` and root submodule-pointer commit `ff9c27808`.

The Run Agent Doc path no longer polls command status every 100 ms, sleeps between attempts, or resubmits the entire editor route when an actor is still starting. Admission and terminal completion now live in one process-scoped Lazily `SourceMap`; the JetBrains client performs one event-driven `editor_command_await`, and a condition variable only parks the socket waiter until that Source changes—it carries no parallel state.

The last-write-wins focus fence was another split authority: it lived in a static mutex `HashMap`. It now lives beside command completion in the same process-scoped reactive graph as a keyed focus-intent `SourceMap`. A narrow gate only serializes the final tmux effect, so a newer reactive focus intent supersedes stale work without accumulating requests.

Command endpoint names, command operation names, projection statuses, focus outcomes, and await bounds are typed enums or named constants. Startup-not-ready remains a typed nonterminal outcome, but the controller owns readiness and completion rather than the client retrying imperatively.

Plugin parity is preserved at the shared controller boundary. JetBrains consumes the new await operation; other clients keep the shared command projection and do not need a parallel status-poll loop.

Verification passed:

- focused controller tests for reactive terminal wake, last-write-wins focus supersession, expiration, and sync/async rejection semantics;
- focused JetBrains `CpRouteClientCommandPlaneTest` and `TerminalUtilTest`;
- source contract test for the typed editor-route wire operation;
- full `make check`, artifact `tart-c41786fbce`, exit 0;
- latest completed CI run `30580926037` is green.

No install, controller recycle, IDE restart, or retained-response resubmission was performed because the existing closeout/install gate still owns that boundary.

### Re: Retained closeouts reported during the audit — gpt-5

The `tsift.md` and `lazily.md` reports are retained controller-owned closeouts, not invitations to recapture their responses. I did not rerun finalize/session-check, force disk, recycle, or rewrite either document. I expanded the existing install/verification gate to require both retained editor-delivery lineages to reach terminal proof before installation. The `tsift.md` wording correction remains deferred until that proof makes a normal new edit safe.

### Re: Fresh-project supervisor logging — gpt-5

The shell currently evaluates `2>> project/.agent-doc/logs/supervisor-stderr.log` before Agent Doc starts, so a fresh project can fail before the binary can create its state directory. The follow-up is queued after the pending installation boundary: Agent Doc should create and open the project-local supervisor log itself, with a deterministic fallback directory when project-local initialization is unavailable, rather than requiring caller-owned shell redirection.

### Re: Reactive hot path and startup audit — gpt-5

> **Queue prompt:** Please audit the hotpath + startup paths to ensure the systems are fully reactive. Any imperative operations should be replace by reactive operations if possible. Consolidate any "split-brain" login into cohesive reactives. Convert any magic strings or magic values into enums or consts.

Implemented and committed the audit as source commit `6d0bf4090` and root submodule-pointer commit `ff9c27808`.

The Run Agent Doc path no longer polls command status every 100 ms, sleeps between attempts, or resubmits the entire editor route when an actor is still starting. Admission and terminal completion now live in one process-scoped Lazily `SourceMap`; the JetBrains client performs one event-driven `editor_command_await`, and a condition variable only parks the socket waiter until that Source changes—it carries no parallel state.

The last-write-wins focus fence was another split authority: it lived in a static mutex `HashMap`. It now lives beside command completion in the same process-scoped reactive graph as a keyed focus-intent `SourceMap`. A narrow gate only serializes the final tmux effect, so a newer reactive focus intent supersedes stale work without accumulating requests.

Command endpoint names, command operation names, projection statuses, focus outcomes, and await bounds are typed enums or named constants. Startup-not-ready remains a typed nonterminal outcome, but the controller owns readiness and completion rather than the client retrying imperatively.

Plugin parity is preserved at the shared controller boundary. JetBrains consumes the new await operation; other clients keep the shared command projection and do not need a parallel status-poll loop.

Verification passed:

- focused controller tests for reactive terminal wake, last-write-wins focus supersession, expiration, and sync/async rejection semantics;
- focused JetBrains `CpRouteClientCommandPlaneTest` and `TerminalUtilTest`;
- source contract test for the typed editor-route wire operation;
- full `make check`, artifact `tart-c41786fbce`, exit 0;
- latest completed CI run `30580926037` is green.

No install, controller recycle, IDE restart, or retained-response resubmission was performed because the existing closeout/install gate still owns that boundary.

### Re: Retained closeouts reported during the audit — gpt-5

The `tsift.md` and `lazily.md` reports are retained controller-owned closeouts, not invitations to recapture their responses. I did not rerun finalize/session-check, force disk, recycle, or rewrite either document. I expanded the existing install/verification gate to require both retained editor-delivery lineages to reach terminal proof before installation. The `tsift.md` wording correction remains deferred until that proof makes a normal new edit safe.

### Re: Fresh-project supervisor logging — gpt-5

The shell currently evaluates `2>> project/.agent-doc/logs/supervisor-stderr.log` before Agent Doc starts, so a fresh project can fail before the binary can create its state directory. The follow-up is queued after the pending installation boundary: Agent Doc should create and open the project-local supervisor log itself, with a deterministic fallback directory when project-local initialization is unavailable, rather than requiring caller-owned shell redirection.
### Re: Controller-owned editor state, SQLite boundary, and IDEA crash — gpt-5.6

Yes—I recommend a plan file for a change of this scope, followed by executing it in the same cycle. I created and completed `src/agent-doc/tasks/agent-doc/plan-controller-owned-editor-reactive-plane.md`; its status is implemented and verified.

The plugin was directly involved in the IDEA crash. The fatal frame is inside `libagent_doc-*.so`, specifically SQLite's `walFindFrame`, and the faulting address was in a deleted `state.db-shm` mapping owned by an older native plugin generation. The fix is therefore architectural, not just a retry or restart workaround.

All normal SQLite ownership is now on the project Control Plane. Once the editor native ABI initializes, the embedded editor process is fail-closed against opening `state.db`, including transitive legacy fallbacks. Editor clients use existing-controller-only paths and cannot bootstrap a controller or create a durable database locally. CLI/offline tools remain explicit non-editor exceptions.

The plugin does not need a durable copy. Its editor state is now ephemeral: open-tab/focus observations, generation and sequence fences, and small UI caches. The controller owns durable projections, document authority, convergence, and effects. Retired native generations are fenced and cannot reopen SQLite or resume stale work.

The wire protocol is request/response framed, but the automatic behavior is semantically reactive: editors publish Source observations and read controller-owned projections; the controller derives focus/sync intents and owns the effects. Imperative RPC remains only for explicit operator commands. The transport shape is not being mistaken for the state-model semantics.

Regression coverage proves a production-mode native plugin cannot create or mutate `state.db`, WAL, SHM, or the controller socket, and the two-generation reload handoff no longer leaves plugin database descriptors or deleted-library work alive. Full verification passed: `make check`, `make tmux-ci`, VS Code's 167 tests and production build, JetBrains tests plus plugin build, focused native/controller regressions, `git diff --check`, and the latest completed CI run is green.

The rebuilt JetBrains plugin `0.2.332` is installed in IntelliJ IDEA 2026.1. Restart IDEA once more—the installation happened after the earlier restart—so the running host loads this corrected package.

### Re: Response

> **Queue prompt:**
>
> do [#fresh-project-supervisor-log]: agent-doc start should create a config


Closed `#fresh-project-supervisor-log`. The installed `agent-doc 0.35.109` already owns supervisor stderr-log creation in the startup path: every start opens the configured/default target, creates missing parent directories, and uses append mode before deciding whether a route-owned TUI also needs process-level stderr redirection. The default is project-local `.agent-doc/logs/supervisor-stderr.log`; relative configured paths resolve from the project root; if the primary path cannot be created/opened, startup uses the deterministic temp fallback `agent-doc/supervisor-logs/<project-hash>/supervisor-stderr.log` and records the warning there.

No additional source edit was required because the implementation and regression coverage were already present in clean tracked source. Verification: `cargo test -p agent-doc-supervisor-process-io` passed all 4 tests, including `fresh_project_start_creates_default_supervisor_log_tree`, configured relative-path resolution, and deterministic fallback. The installed CLI reports `agent-doc 0.35.109`; the latest CI run is `completed success`.
<!-- agent:boundary:4a64c69b -->
<!-- /agent:exchange -->
## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" go -->
<!-- /agent:queue -->

## Notes

<!-- agent:notes -->
<!-- /agent:notes -->

## Backlog

<!-- agent:backlog priority queue -->
- [/] [#reactive-closeout-install-verify] After the retained tsift.md editor-delivery capture reaches a terminal boundary, install the committed Agent Doc binary and verify the equityfundingsource.md queue strike, Monster Rod Holders corruption rejection, Compact Exchange recovery, and editor/tmux responsiveness without recycling an active closeout owner.
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
- [/] [#compactreactiveprojectionverify] [operator-verify] With IntelliJ IDEA 2026.1 running JetBrains plugin 0.2.319 / native agent-doc 0.35.83, retry Compact Exchange from a saved large session without restarting IDEA. Verify ACK replay/hot reload either completes the retained editor delivery exactly once or leaves it retained without duplicate mutation, disk fallback, focus change, or a fresh agent session. Then type a sustained burst and confirm recovery is coalesced rather than per-keystroke. Record any residual queued-EDT late effect for #implementlazilyintent. Do not queue this item automatically; it requires operator UI observation.
- [/] [#steeringmirrorverifylive313] [operator-verify] With IntelliJ IDEA running JetBrains plugin 0.2.313 / native agent-doc 0.35.71, make two separate exchange edits during one active turn. Confirm the banner/status item shows the aggregate edit count and its hover contains both edits verbatim; then run Agent Doc once and verify the controller records one aggregate handoff without submitting a partial edit. Do not queue this item automatically; it requires operator UI observation.
- [/] [#preflightinbinary] [operator-verify] Live proof for #preflightinbinary AFTER the default flip (5ce9b06e6). The flip has landed: SKILL.md now says do NOT run preflight when the '[agent-doc] cycle contract ...' marker is present, and the hook is registered in this project's .claude/settings.json under UserPromptSubmit alongside turn-status. Needing a live session to confirm: (a) invoke /agent-doc <FILE> in a FRESH Claude Code session and confirm the marker + contract appear in context before the agent runs any tool, and that the agent does NOT shell to 'agent-doc preflight'; (b) confirm exactly ONE cycle id is opened for that turn — grep .agent-doc/logs/ops.log for preflight_diff_start, and confirm 'repaired stale preflight_started cycle' does NOT appear; that line is the tell that the agent still ran preflight and the round trip was merely absorbed by repair rather than removed; (c) confirm an ordinary prompt that merely mentions agent-doc produces no marker and no cycle; (d) confirm a Codex/OpenCode turn (no Claude hook, so no marker) still runs preflight explicitly and closes out normally. Scope note: reusable_by_reentrant_preflight is defence in depth only — start_preflight runs at preflight step 4, after repair at step 1, so in the live pipeline repair closes any prior open cycle before the guard sees it. What removes the double-open is the flip itself.
- [/] [#jbsurfaceswap] [operator-verify] Live IDE proof for #jbsurfaceswap (agent-doc 27beeae2e, JB plugin 0.2.296). Both plugins now report one EditorSurface observation per event and the graph derives the intent; the planners are deleted. With the IDE restarted onto 0.2.296, confirm in a real session: (a) clicking between two side-by-side split editors moves the tmux active pane (derived Focus); (b) changing the editor split layout reconciles tmux (derived Sync) and shows the 'Sync: --col ... [focus: ...]' hint exactly once; (c) repeat focus events on the same editor cost nothing — the JB log line '[layout-sync] observe: ... intent=idle' with no controller command; (d) swap two tmux panes by hand, then click an editor tab WITHOUT changing the split, and confirm the pulled tmux probe reconciles the layout instead of only moving focus (#tmuxsyncstate); (e) closing the project releases the root (no growth in surface graphs across open/close cycles). Headless tests cover the fold, the probe seam, and the observation wire shape, but not the debounce/generation interaction with real editor event storms.
- [/] [#jbpluginlazilyeffects] #agent-doc-feature The JetBrains/VS Code plugin should drive lazily state + Effects, not one-shot CLI commands. Today every editor action (file selected, layout changed, Sync Tmux Pane) shells a fresh agent-doc process that re-observes the world from scratch and returns one answer that is already stale, with no subscription and no invalidation. Keep the CLI supported as an entrypoint, but the plugin path should be: plugin reports an event -> FFI sets a Source in the document scope -> an Effect drives the tmux/pane consequence. Same Shared Foundation rule already in AGENTS.md, applied to observation rather than business logic. Consumes the derived owner map from syncownerreactive, so order it after that.
- [/] [#boundaryaccumverify] [operator-verify] After restarting any JetBrains IDE still on the older package generation, reproduce a wedged finalize/direct-commit handoff and confirm visible document, snapshot, and committed HEAD each retain exactly one agent:boundary marker; report any boundary_invariant_violation that does not self-heal.
- [/] [#r4gq] [operator-verify] Live JB Run Agent Doc dispatch-hang proof (#kjw0 fix, shipped 0.34.66 build): after the recycled build is running, trigger Run Agent Doc into a BUSY codex pane (mid-turn, 'esc to interrupt' visible) and confirm it returns FAST with a queued-behind-active-turn outcome (pane shows queued status) instead of hanging ~21s then a dispatch-start-unproven notification. Grep ops.log for route_dispatch_start_queued_behind_active_turn (present on success) and confirm accepted_without_dispatch_start_proof is ABSENT for the busy case. Root cause: route injected the trigger into a mid-turn pane; codex queued it behind the active turn so the UserPromptSubmit hook could not advance within the 10s proof budget, and the direct-pane path paid TWO 10s waits (initial + late-resubmit) then filed a false bug. Fix: short_circuit_dispatch_start_when_pane_busy in route/dispatch.rs detects busy_proof_line and returns RoutedDispatchStartProof::AcceptedQueuedBehindActiveTurn (accepted, not fail-closed).
- [/] [#zqv8] [operator-verify] #jbrelaylive Restart IntelliJ IDEA 2026.1 to activate the installed JetBrains plugin 0.2.247, rerun Run Agent Doc on tasks/professional/equityfundingsource.md while the dim Summarize recent commits suggestion is visible, and confirm dispatch succeeds without a protected-input refusal or a long UI freeze.
- [/] [#compactindcellsverify] [operator-verify] With agent-doc 0.35.3 and JetBrains plugin 0.2.276 active, run Compact Exchange while deleting several queue items and editing a post-boundary prompt; confirm the deletions and prompt survive, compaction commits, and ops.log records compact_rebased_authoritative_siblings without compact_dropped_non_exchange_item.
- [/] [#crdtpushdrainverify] [operator-verify] #crdtpushdrain live proof: after an IDE restart loads JetBrains plugin 0.2.278, run Compact Exchange on a document that has been idle for >30s and confirm ops.log `compact_crdt_relay_acknowledged ... wait_ms=` drops well below the prior ~4.5s floor (expect roughly the pull/apply round trip), with no `compact_crdt_ack_recovery_signal ... reason=ack_recovery_force_refresh` line needed to unstick it. Restart is required because reload-lib only swaps the cdylib, not the Kotlin plugin.
- [/] [#capturebacklogatomic] #agent-doc-bug Capture response plus backlog mutations atomically and let preflight supersede an uncommittable capture. LIVE RECURRENCE 2026-07-22 (#8ybz): retained #bbpe closeout with --backlog-gate spliced the exchange heading/body into the #fmgc backlog row, erased its item boundary, and minted synthetic id #8ybz from the fragment. This is the exact cross-component case missing from the response-only retry regression. Preserve exchange + granular backlog mutations as one cycle intent and add a retained-ACK/CAS-race regression proving component boundaries remain disjoint. Plan: tasks/agent-doc/plan-capture-includes-backlog-mutations.md
- [/] [#bbpe] [operator-verify] Confirm tasks/brookebrodack-dev.md now arms after the IDE picks up the reinstalled plugin: restart the JetBrains IDE (or reopen the doc) to clear editor_attached_model_missing, then run agent-doc preflight tasks/brookebrodack-dev.md and expect queue_maintenance_not_persisted GONE and queue_supervisor_drainable true. The #qstartinert code fix is shipped + installed (agent-doc 9111a3381); only the editor-authority persistence blocker (#5d9f) remains between the computed queue and the persisted one
- [/] [#hvw0] [operator-verify] Rotate the CARGO_REGISTRY_TOKEN secret on github.com/btakita/skill-harness — the current one returns 403 authentication failed, so CI cannot publish to crates.io (v0.1.3 was published manually from the passage token btak/CARGO_REGISTRY_TOKEN). Needs a repo-secret write, which is operator-gated. Until it is rotated, every tagged release will now FAIL loudly at the publish step (that is the intended new behavior after 32836ad; it previously reported success)
- [/] [#c8tb] [operator-verify] Live #jbroutebusystall proof (0b49105e1): with the supervisor socket down (route logs `route_dispatch_only_authoritative_degraded_direct_pane supervisor_health=no_socket`) and the pane mid-turn, click JB Run Agent Doc. Expect `route_start` -> `route_success` in the attempt ledger to drop from ~17s to under ~2s, with `route_existing_pane_busy_active_turn_skip_wait` in ops.log and the SAME outcome as before (`route_existing_pane_already_running`, pane focused, no duplicate injection). Unit tests pin the cue gating but cannot prove the live latency win.
- [/] [#operatorverifylive] [operator-verify] #relaylockpoison live proof: with the JB plugin loaded, force a replica panic (the crash-2026-07-09 shape: agent_doc_replica_apply_local) and confirm later replica calls still succeed instead of returning -1/null for the life of the IDE. This is the behavior the whole migration exists to guarantee and it cannot be proven without a host IDE.
- [/] [#compacttombgc] [recommended] Fenced Compact Exchange CRDT epoch — RE-SCOPED 2026-07-19 away from tombstone GC. The #bootstrapsize measurement (crdt_sync.rs) on a real 57KB document found 88,630 ops and ZERO tombstones, so TextCrdt tombstone GC would reclaim nothing on the observed shape; the payload is pre-compaction INSERT history. The valuable half is the epoch itself: rebootstrap attached replicas from the compacted snapshot and reject/component-rebase older-epoch deltas, which discards that insert history — the only mechanism that makes compaction actually shrink the register bootstrap. Keep tombstone GC as a trailing step for delete-heavy documents, not the headline. Still needs the all-replica stability frontier before any epoch fence is safe.
- [/] [#multilineheadbullet] [focused-cycle] Make multiline `---` queue prompts addressable — SHARPENED 2026-07-19. Damage is already PREVENTED (#qconsumenostrike fail-closed guard, 5b3761dfc); this is the remaining correctness half. Refined finding: the ENTRY model already handles multiline correctly — document_queue::parse surfaces the multiline head as a Prompt (preflight lists it), mark_first_n_prompts_completed targets it, and render emits Completed multiline as `~~~done` fences. The mismatch is that markdown_ast::item_nodes (used for IPC node ops and AST-keyed consume) enumerates only list items, so it returns the NEIGHBOUR for the same logical head — observed as node_ops=[queue:1:neighbour:0] while consumed_text was the multiline head. So the fix is to reconcile the two ENUMERATORS, not the entry model: either teach item_nodes the `---` block shape, or render multiline heads with a bullet + indented continuation that both round-trips through parse_spans and yields a node. Needs one clean reproduction to confirm which of the two paths actually mutated the document in the observed strikes.
- [/] [#zq8e] Editor-authority wedge blocks ALL queue maintenance (root of the #goal drain stall). Evidence in ops.log for this doc: at 19:32:45 `controller_crdt_current_text source=preflight_queue_maintenance` returns `status=current live_editors=1 delivery_converged=true`; 2s later the SAME queue_maintenance path logs `document_model_ensure_start initial_state=editor_attached_model_missing`, a `crdt_replica_register` succeeds (`data_kind=ok generation_fenced=true bootstrap_bytes=1312802`), then `document_model_ensure_republish_observer_not_ready` -> `document_model_ensure_failed` after 5s. EVERY `document_model_ensure_start` this session reports missing-replica across unrelated sources (queue_maintenance, pending_add_sync, controller_recycle_request) while the controller consistently reports current. Two observers of one document disagree; the ensure-path observer does not see a replica the controller demonstrably has. NOTE: likely already owned by the `src/agent-doc-editor-authority-fix` worktree (commits `preserve attached editor authority during retry`, `bound idle authority replay`) — coordinate before landing a rival fix in src/agent-doc.
- [/] [#7n20] Finish the ensure-window liveness fix across the CLI/controller process boundary. `bbc50c26e` extends the 400ms missing-replica ensure window when a `crdt_replica_register` proves the editor is alive, but the counter is process-global: on the split path the register lands controller-side while `ensure_document_model` runs CLI-side, so live telemetry still shows `window_extended=false` and the wedge persists. Two options: carry the liveness proof across IPC, or run the ensure controller-side where the registration already is. Do NOT route preflight's initial current-doc resolve through the ensure path — tried in this cycle and reverted; it violates `preflight_missing_editor_model_pauses_without_reading_disk_authority` and made preflight resolve to DISK content instead of waiting for the attached editor.
- [/] [#realtimesalient] #agent-doc-feature Write salient responses into the document in REAL TIME rather than buffering until finalize. Today only complete `### Re:` sections cross response-checkpoint and partial output is console-only (SKILL.md: "stream incomplete progress only to the console"). Requires deciding what counts as salient mid-turn, and preserving the invariant that incomplete token prefixes never become authoritative document text — so likely a distinct node kind (progress/salient) that is replaced by the final response rather than appended to it. Pairs with #promptselect threading.
- [/] [#mtds] [operator-verify] #panedraftunblocker live proof: on a Claude pane whose footer reads `⏵⏵ bypass permissions on · 1 shell · ← for agents` (the variant without `(shift+tab to cycle)`), with an EMPTY composer and no active turn, trigger JB Run Agent Doc. Expect dispatch to succeed instead of refusing with `timed_out` / `unblocker=wait_for_dispatch_ready_prompt`. Then repeat with unsent text typed in the composer and expect the new refusal naming the draft with `unblocker=submit_or_clear_pane_draft`, and NO injection over that text. Finally confirm a genuinely mid-turn pane (live spinner, bare `❯`) still refuses as busy. Fixed in 17848a927, installed 0.35.6; needs the supervisor idle-boundary recycle to pick up the fresh binary.
- [/] [#promptselect] #agent-doc-feature Selection-scoped `Run Agent Doc`: send ONLY the editor text selection as the prompt. Needs (a) a dedicated CLI verb — `agent-doc prompt "<text>"` (or `steer`; `run` is taken) accepting multi-line via stdin/cat, (b) JB+VSCode `Run Agent Doc` hooked to it so a non-empty selection routes through `prompt` instead of the whole-document trigger, (c) acknowledgement: agent-doc echoes the prompt into the document with the prompt prefix plus salient response, (d) responses attached UNDER their originating prompt — a threaded prompt->response layer in the Document Tree rather than a flat exchange tail. Must compose with realtime steering (#realtime-steering-verbatim) since a selection sent mid-turn IS steering. Design pass needed before implementation.
- [/] [#agentsignals] [future/separate] Add a realtime agent:signals element after core realtime convergence is stable. Start with deterministic built-in signals for tests, queue, supervisor, and ipc; design custom user-defined signals as plugin-backed tracked work with Rust as the first target and future multi-language support; use tasks/software/realtime-signals-patchboard.md as the GUI/semi-programmatic reference; keep MLP enhancement optional/experimental and not load-bearing for deterministic states.
- [/] [#steeringobservableset] #agent-doc-bug Phase 6 durable graph adoption: back RealtimeSteeringSet with a lazily reactive OBSERVABLE SET (not a drain-one-head QueueCell) keyed on the body-aware identity (Phase 3.1), reconciled on the reliable-sync plane; session-check reads set-emptiness (all directives settled) instead of re-diffing HEAD↔buffer. Plan: src/agent-doc/tasks/agent-doc/plan-exchange-tree-seqcrdt-and-ipc-unify.md (Phase 6). [recommended]
- [/] [#crde] JetBrains Run Agent Doc route/dispatch failed after bounded submit/start proof retries #jbrunautobug #agent-doc-bug failure_class=accepted_without_dispatch_start_proof document=/home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md stage=dispatch_only_dispatch_start_proof pane=%83 actor_generation=1214 editor_attempt_id=1783978030079-4 dispatch_proof_state=accepted_without_dispatch_start_proof elapsed_ms=10000 diagnostic_path=none ops_log_path=/home/brian/work/btakita/agent-loop/.agent-doc/logs/ops.log ops_log_marker=route_submit_issue(issue=accepted_without_dispatch_start_proof,phase=dispatch_only_dispatch_start_proof,result=accepted_without_dispatch_start_proof) [symptom-key invariant=run_agent_doc_route_dispatch_failure document=71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad component=route/dispatch_only_dispatch_start_proof content_hash=sha256:3251e072b5c1857ba06e9ba613e1004b5eefef103db7003d7dcf7efe0d698ea8]
  evidence: JetBrains Run Agent Doc route/dispatch failed after bounded submit/start proof retries #jbrunautobug #agent-doc-bug failure_class=accepted_without_dispatch_start_proof document=/home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md stage=dispatch_only_dispatch_start_proof pane=%83 actor_generation=1214 editor_attempt_id=1783978020608-2 dispatch_proof_state=accepted_without_dispatch_start_proof elapsed_ms=10000 diagnostic_path=none ops_log_path=/home/brian/work/btakita/agent-loop/.agent-doc/logs/ops.log ops_log_marker=route_submit_issue(issue=accepted_without_dispatch_start_proof,phase=dispatch_only_dispatch_start_proof,result=accepted_without_dispatch_start_proof)
- [/] [#supboundarylivelock] #agent-doc-bug Supervisor idle-boundary recycle can livelock, leaving a stale binary mapped indefinitely. supervisor_recycle_action (agent-doc-supervisor/src/lifecycle.rs:124) returns None on `!turn_boundary` for everything except write_wedged/editor_delivery_stale, and turn_boundary = prompt_visible && !turn_active (idle_watch.rs:1898) never holds for a continuously-active pane. The only bypass, stale_recycle_safe_checkpoint (idle_watch.rs:136) = supervisor_stale && inflight_handlers == 0, is gated on staleness, so an explicit `admin recycle` on a NON-stale busy supervisor can never be consumed and silently expires at DEFAULT_RECYCLE_REQUEST_TTL_SECS=900 (recycle_request.rs:17) with the ledger phase left Requested. --force does not help: rpc.rs:5905 only changes the reason string, and idle_watch.rs:2403 collapses the request to a plain bool. MAX_CYCLE_OPEN_DEFER_TICKS escalation cannot rescue it either (idle_watch.rs:2253 only accrues the streak AT a boundary). OBSERVED 2026-07-18: supervisor pid 2040014 stayed stale across this whole session even though it IS stale (so the checkpoint bypass should apply and inflight_connection_handlers is an in-process RAII-guarded atomic that cannot leak) - the stale-side path needs its own trace. Fixing the non-stale side means a mid-turn execve, which is exactly what the boundary gate exists to prevent, so this needs a design decision (manufacture a boundary via the recycle-yield path? bound the defer? re-arm instead of expiring?), not a quick patch. Related shipped fixes: #dispatchgateleak + #authorityfactretention (submodule 566a72f6d).
- [/] [#jbrunsteering] #agent-doc-bug JB `Run Agent Doc` while a turn is already running should feed the prompt in as realtime steering (#realtime-steering-verbatim / #realtime-steering-aggregate) instead of interrupting the active turn. Operator-reported: invoking it mid-turn produced "Interrupted · What should Claude do instead?" rather than surfacing the new prompt to the running turn. NOT STARTED.
- [/] [#ay80] CI red on main, pre-existing and unrelated to #jbsteerinterrupt: `finalize_skips_ignored_untracked_session_doc` (tests/finalize_integration.rs:248) fails in GitHub Actions with `Unexpected stderr, failed var.contains(skipped ignored untracked path scratch/session.md)` but PASSES locally (7627/7627 green). Run 29657026491 on b7e17f4d5. Likely a CI-environment gitignore/tmpdir difference; reproduce with the CI runner git config.
- [/] [#5d9f] Second session observed that finalize's commit boundary persists queue maintenance through a path standalone `agent-doc queue sync` never reaches — the queue landed on the next write after three straight sync failures plus an `admin reload-lib`. Two persistence paths with different reachability is itself the bug surface. Determine why the commit path succeeds where queue sync fails, and route queue sync through whatever the commit boundary does. Result: queue text populates but stays UNARMED (`queue_drainable_head_count: 0`, `queue_continuation_required: false`), so the auto-loop still never gets the head.
- [/] [#jbroutasync] #agent-doc-bug Honor submit.deadline_ms in route dispatch (NARROWED — the async-endpoint half moved to #jbrouteasyncchannel). CONFIRMED: deadline_ms is only ever written (rpc.rs:1116, command_plane.rs:93) and never read by dispatch; routed_cycle_ack_timeout is 30s with a live child / 15s without (agent-doc-controller/src/dispatch.rs:2099-2103); the plugin sends deadlineMs = waitForReadySeconds*1000 for editor_route (CpcRouteClient.kt:281), observed as timeout_ms=15000 in ops.log. Net effect: with a live child the client gives up at 15s while the controller waits to 30s, so the operator sees a timeout while the route is still running. FIX: cap the ack wait at min(client deadline, routed_cycle_ack_timeout). PLUMBING REQUIRED (why this is not a one-liner): the deadline lives in the controller RPC layer but the wait happens in agent-doc-route-io at cycle_ack.rs:334, and no type in that crate carries a deadline. Thread an Option<Duration> through require_routed_cycle_ack (cycle_ack.rs:313) and its 3 call sites (authoritative_dispatch.rs:987, authoritative_dispatch.rs:1142, pane_resolution.rs) plus the controller→route boundary. NOTE test_cli.rs:16811 pins the exact expression 'routed_cycle_ack_timeout(live_child_for_file, cfg!(test))' and must be updated to pin the invariant, not the syntax. Verify with a live route against a busy pane: the client-side timeout and the controller-side give-up must coincide.
- [/] [#f1s3] #queuenest phase 2 — `ol` -> `after=` compilation, per tasks/agent-doc/plan-queue-nested-list-grouping.md. PHASE 1 SHIPPED in b67dd3582 (indent-aware parse retaining depth, leaf-only execution so group parents are never runnable heads, byte-preserving render round-trip). REMAINING: parse `1.` ordered-list markers (today they fall through to Freeform), and compile a nested ol to `after=` edges at ingress next to #queueingressorder, keeping `after=` the single source of truth so reordering an ol re-derives edges. BLOCKED ON A DESIGN ANSWER the plan itself records as open: do groups need ids (`[#epic]`) for `after=` to reference a whole group? Compiling an ordered GROUP to edges needs to know whether a group is addressable. Second open question (does `priority=` on a parent apply to all its leaves?) affects the sort, not the compilation, and can be settled later.
- [/] [#editormodelmissing] [operator-verify] #agent-doc-bug editor_attached_model_missing permanently wedges every write needing editor authority (agent-doc-bugs2 gen=1518, equityfundingsource gen=1452; healthy docs are gen 14-82). NARROWED 2026-07-18: survives admin recycle x2, admin reload-lib, AND a full IntelliJ restart, so it is NOT a plugin model-rebuild problem as first filed. Root cause is a one-way latch in editor-attachment state: TransportProjection.editor_generation (agent-doc-state-backbone/src/lib.rs:3394) is SET on StateFact::OwnerGenerationChanged (line ~1985) and never cleared — there is no editor-detach/close StateFact in the enum at all. document_has_live_editor_transport (agent-doc-document-realtime/src/crdt_authority.rs:165) therefore returns MultiReplica forever once an editor has ever attached, with NO liveness proof, even though the sibling authority_from_liveness (line 122) exists precisely to demote a stale listener via pid liveness. The ONLY predicate that flips it false is TransportPatchPhase::ForceDiskFallback — which is exactly why --force-disk is the sole escape the operator ever found. Second candidate latch, same shape: observe_editor_open's durable reliable-sync receiver journal may replay a stale editor-open assertion across an IDE restart (resolve_closed_editor_disk_fallback_current_doc, agent-doc-document-realtime-io/src/lib.rs:3774 bails only when it reports open). DECISIVE EXPERIMENT before coding: instrument which of the two reports attached — log editor_generation + observe_editor_open together at the EditorAttachedMissingReplica branch (agent-doc-crdt-relay-io/src/lib.rs:696). Both present identically, and the fix differs by which. DO NOT ship a speculative authority-demotion change: wrongly demoting a live editor lets the binary overwrite an operator buffer, which is the exact failure every guard on this path exists to prevent. Also worth shipping regardless: admin detect reported 'No fleet anomalies detected' while two documents were hard-wedged — add an attached-but-no-replica detector so this is visible instead of silent.
- [/] [#bbdqprio] #agent-doc-bug brookebrodack-dev.md: agent:queue carries the priority attribute but a cycle's response explained next tasks without reordering the queue by backlog priority. Hypothesis: same failure class as the queue_maintenance_not_persisted warning seen in agent-doc-bugs2.md this cycle — preflight COMPUTES the reorder (logs 'queue: sorted do-prompts by backlog priority') and then fails to persist it because editor authority stayed in editor_attached_model_missing, so the previous queue text stays in effect and the reorder silently evaporates. Verify by correlating a brookebrodack-dev preflight's 'queue: sorted' line against the resulting document; if confirmed, the fix is to make a non-persisted queue maintenance pass fail loudly (or retry) instead of logging a warning three lines after printing work it did not do. Do NOT run preflight against brookebrodack-dev.md while pane %9 owns it.
- [/] [#falsetyping-guard] #agent-doc-bug provenance-aware visible-write guard. INTEGRATED to main (merge on 5bcfc07b) + binary/cdylib installed + recycled; guard tests 7/7 green; JB 0.2.205 zip + VS Code 0.2.38 compiled. REMAINING (operator): install JB plugin zip (build/distributions/agent-doc-jetbrains-0.2.205.zip) + reload IDE, install VS Code 0.2.38, then live proof — type during replica churn (no wedge) + genuine unsaved edit still fails closed. NOTE: bugs2.md can still wedge until the IDE plugin is reloaded (loaded plugin still calls FFI v2 → conservative fail-closed).
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
- [ ] [#exttasksources] #agent-doc-feature Add external task source adapters that project PM-tool tasks into the active document/work graph as source-backed nodes, keeping local queue/closeout authority document-owned and making remote push-back explicit and audited. PRD: tasks/agent-doc/prd-external-task-sources.md
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
