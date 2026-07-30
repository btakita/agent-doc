---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
resume: 019fafe5-8be5-73d1-add0-f9314b2080a1
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

Closed stale queued #5qjj as superseded; preserved unproven current-plugin UI proof as non-queued #steeringmirrorverifylive313.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260728-192145.md`*

Compacted content:
- Archived 30 response topic(s): Live Sync Tmux Layout crash verification; #60ev and Compact Exchange latency; Compact Exchange performance; 27 more
- Prior summary/context: prior compacted content: Archived 2 response topic(s): Live Sync Tmux Layout crash verification; Live Sync Tmux Layou...

### Re: Response

Implemented and released.

The short architectural answer is:

- Sidecars are write-only recovery artifacts. Runtime reads do not fall back to them. Canonical state lives in the cross-process Lazily state plane, and effects materialize crash recovery snapshots from that state.
- “Crash-state snapshots” is the clearest name for the persisted files. “Crash-state effects” names the writers. A projection is the computation that derives snapshot content, not the sidecar itself.
- The entire pane-layout control loop belongs in Lazily state: desired layout, observed tmux layout, effect receipts, retry/backoff, and reconciliation status. The last hop remains an imperative effect because tmux is external.
- Plugins should use RPC as transport to publish intent/observations into the state plane. RPC and projection are not alternatives: RPC crosses the process boundary; Lazily projection makes the result durable, idempotent, and reconcilable. Plugins should not own an imperative tmux retry loop.
- Typed enums are appropriate for phase and reason, with an explicit unknown/forward-compatible case at wire boundaries. This alone is not a reason to adopt protobuf. Protobuf becomes attractive only when polyglot generated schemas and long-lived binary compatibility justify its operational cost.
- The same plane helps CRDT convergence: desired revision/state vector, observed editor acknowledgment, retained effect identity, lease, attempt, and typed pending reason can all be projected. That lets observers report one retained exact-once operation without resending it.

Document replicas now use one lazy serialized actor lane per document, not one project-wide lane and not an unbounded thread per operation. Same-document work remains FIFO; different documents proceed concurrently; idle lanes release their threads. Native calls use a bounded four-worker generation pool. A timeout while merely queued cancels only that invocation and does not poison the generation; only a call that actually runs past its lease does. Rust now locks replicas independently rather than holding the global registry mutex throughout an operation.

This shipped as agent-doc 0.35.70 and JetBrains plugin 0.2.312. Focused tests, the full 389-test plugin suite, Rust FFI tests, and the mandatory full check passed. The release is published at https://github.com/btakita/agent-doc/releases/tag/v0.35.70 and the Python wheel was uploaded.

The post-install IDEA restart provided the live proof: plugin 0.2.312 loaded native 0.35.70; `agent-doc-bugs2.md`, `haiven.md`, and `monsterrodholders.md` ran on distinct document lanes. Haiven’s expensive attach took about 10.4 seconds, while monsterrodholders attached concurrently in about 1.6 seconds. The native generation remained healthy and haiven subsequently acknowledged its retained CRDT update.

Do not ask the haiven Codex to finalize again. I performed the single status-only check after recovery: its original retained cycle `cycle-1785289132562` is now committed successfully, with no queue continuation. A second finalize would create exactly the duplicate transaction the retention design prevents.

The pane layout is also reconciled through the new state plane: `agent-doc-bugs2.md` is visible in pane `%63`, `monsterrodholders.md` in `%43`, and the unrelated haiven pane is stashed. The monsterrodholders controller record remains intentionally closed from its earlier Ctrl-D, while its editor replica now attaches normally; focusing/syncing the editor no longer depends on reading a sidecar or on another document’s replica worker.

### Re: Response

> **Queue prompt:**
>
> 📌 do [#sidecarstate]


Implemented and released the agent-doc performance and state-authority repair.

The thread dump did not show a JVM deadlock. The Agent Doc workers were predominantly waiting normally; the visible IDEA freeze came from a feedback loop on the editor hot path. On each typing burst, the plugin was re-running open-document attachment work, normalizing the whole document, and advancing its shadow ahead of already-queued `DocumentEvent`s. Those older events then appeared to conflict with the new shadow, which manufactured stale-baseline recovery. Each ordinary keystroke also crossed FFI for `node.text()`, and recovery could publish 1.4–1.7 MB whole-document updates taking seconds.

The fix makes an already-attached replica an immediate no-work path. Each document actor now owns a cached `knownReplicaText`: registration materializes native text once, local deltas carry their exact resulting text into the actor, and remote updates refresh the cache. Ordinary typing no longer performs a native full-text read per key or mutates the shadow from a coalesced future editor cut. A 100K-document regression proves eight sequential deltas require only the initial native text read.

Text adoption now retains the operation durably and synchronously asks the controller to project `crdt_text_adopt`; success is returned only after the controller acknowledges it. That closes the retained-but-not-yet-projected race behind the repeated haiven convergence state, without resending or replacing the exact-once operation.

Sidecars are not runtime authority. The legacy `editor_op_captures` table is no longer read or imported, normal append/clear/GC paths do not consult it, and a regression proves a legacy row remains inert. Runtime recovery uses the typed Lazily state projection. If an actor socket disappears it may rebuild from the typed cold event ledger; if the live actor remains present but unresponsive, it fails closed instead of falling back to a sidecar. Sidecars should be named crash-state files/snapshots, materialized only by crash-state effects/writers.

The broader architecture is now explicit:

- Lazily desired/observed state represents the entire pane-layout pipeline; reconciliation/retry drives tmux toward the desired pane set.
- Plugins use RPC only as cross-process transport into typed Lazily projections. Effects, not plugins or state readers, perform tmux and crash-state writes.
- Phase and reason values should be typed enums. Protobuf is not needed merely for those enums; add a schema/IDL only when independently versioned non-Kotlin/non-Rust consumers justify it.
- Replica isolation is one serialized actor/worker per document on a bounded native pool, not one operating-system thread per replica and not a project-wide lock.
- The same cross-process state plane that first drives editor–tmux sync is the right substrate for CRDT convergence intent, observed acknowledgments, leases, and retries.

Released agent-doc 0.35.71 and JetBrains plugin 0.2.313. The matching binary/library and plugin package were installed; the controller accepted an idle-boundary recycle. Source commit `89d56d8aa` and parent integration commit `5e987c09c` are pushed, with tag `v0.35.71` published. Verification passed: 390 JetBrains tests, the op-capture authority tests, `cargo fmt --all`, and the full `make check` gate. The user-owned untracked `editors/zed/extension.wasm` was preserved.

Restart the running IntelliJ IDEA instance once to load plugin 0.2.313. After that, typing in a large agent-doc document should stay on the delta-only path instead of entering the attach/full-refresh recovery loop.

### Re: `#5qjj` operator verification: restart confirmed, aggregate UI proof still pending

The restart loaded the current installed generation: IntelliJ IDEA 2026.1 has `agent-doc-jetbrains-0.2.313.jar`, and its startup log confirms native agent-doc `0.35.71` loaded successfully.

I cannot yet close the aggregate-steering acceptance criterion. For the active cycle `cycle-1785295203530`, the typed `realtime_steering` observation is still empty and no post-baseline editor delta arrived during the observation window. Therefore there is no honest live evidence yet that the banner/status item renders `(<N> edits)` and exposes the full verbatim aggregate in its hover, nor that a subsequent Run Agent Doc queues the aggregate handoff without submitting a partial edit.

`#5qjj` remains pending. To finish it, make two separate exchange edits while a turn is active, observe the banner count and hover, then run Agent Doc once. I can verify the resulting aggregate projection and handoff from the controller logs; the operator must report the rendered banner/hover because that UI is not represented in the logs.

### Re: `#5qjj` queue-loop resolution

> **Queue prompt:**
>
> 📌 do [#5qjj]


The current-plugin prerequisite is verified: IntelliJ IDEA restarted with `agent-doc-jetbrains-0.2.313.jar`, and native agent-doc `0.35.71` loaded. The live two-edit banner/hover observation did not occur, so it would be incorrect to mark that UI behavior verified.

The old `#5qjj` item is now closed as superseded because its queued operator-verification shape repeatedly redispatched itself without an operator event. The still-required proof is preserved as the non-queued backlog item `#steeringmirrorverifylive313`, pinned to the installed generation. This separates agent-drainable work from operator-only UI observation and prevents another automatic queue loop.

The refusal to read disk while an editor was attached was correct: an unsaved editor buffer must remain the source of truth. The bug was that missing-replica recovery stopped at that refusal and asked the operator to close/reopen the tab. In agent-doc 0.35.74, current-document resolution instead sends a typed, non-projecting save request to the registered editor. The editor saves its exact live buffer and immediately republishes the current content; agent-doc requires the applied receipt before it may read that editor-written disk cut and rebuild the controller model. There is still no unproved disk fallback, force-disk path, or close/reopen requirement.

The safe save-only request can cross a same-protocol build mismatch by retrying with the live listener's reported build identity. That exception is deliberately limited to saving without a projection; all document-mutating intents remain strictly build-fenced.

Live verification succeeded on both affected documents:

- `agent-doc-bugs2.md` returned an applied editor-save receipt and advanced past the missing-current-document failure. Its older retained delivery then cleared through the existing status-only recovery path.
- `haiven.md` returned the same applied receipt and resolved successfully. Session-check repaired its historical committed snapshot drift and reported committed cycle `cycle-1785300038605`, `queue_continuation_required=false`, and `no_drainable_work`. The earlier instruction to save manually or reopen the tab is obsolete.

The editor-selection/tmux issue shipped in 0.35.73. JetBrains selection events are micro-coalesced; inactive-window, superseded-generation, and aged intents are refused; the controller holds a project-scoped latest-wins fence through the final `tmux select-pane`; and the low-latency path only selects an already-visible pane. There was no agent-doc pointer-warp command—the mouse jump was the window manager's side effect of a stale worker briefly selecting the other pane. Preventing that stale selection removes the trigger.

The earlier retained-closeout editor-save barrier shipped in 0.35.72, so both ordinary retained delivery and the zero-replica recovery now preserve editor authority end to end.

Verification passed: focused protocol/IPC/current-document tests, live JetBrains receipts on both documents, installation of CLI/native/editor packages, and the complete `make check` gate. An initial parallel run saw three temporary test-controller connection refusals; each reproduced cleanly in isolation, and the required full rerun passed. Agent-doc 0.35.74 is installed, source commit `84cafa20d` is pushed on `main`, and tag `v0.35.74` is published. The user-owned untracked `editors/zed/extension.wasm` was preserved.

Harper is LSP4IJ's local Markdown grammar/style language server and is unrelated to agent-doc's controller, CRDT replica, or tmux synchronization paths.
### Re: editor-authoritative recovery and focus sync — gpt-5

Fixed and released.
### Re: Response

Yes. Continue in the original Haiven Codex session in tmux pane `%62`; agent-doc did not send input to, focus, terminate, or restart that pane while applying this fix.

Implemented and committed `5183f104b` (`fix: preserve live sessions and editor convergence`) in `src/agent-doc`:

- Accepted non-forced supervisor replacement requests now remain in-place through timeout and drain only at an idle boundary. A stale timeout is observational and can no longer kill a live mid-turn harness or start a fresh Codex thread.
- Continue/restart recovery preserves the live document-owned harness and exact session binding. Cold recovery uses only the exact editor-authoritative `resume:` ID and fails closed when it is absent; it never falls back to a global latest session. Explicit Fresh remains the only route allowed to start a new thread.
- The controller-owned agent session ID is authoritative before, during, and after an in-place restart. Frontmatter `agent_doc_session`/`resume` is now specified as a durable projection and cold-recovery seed, not authority that may replace the live binding.
- Pane ownership recognizes an exact document-owned Codex wrapper even when tmux reports its current command as `node`, while unrelated Node panes remain protected.
- Committed cycles may now settle an exact live-editor projection through native editor save. Save operation IDs are stable per content hash, so retries converge instead of producing fresh operations and save churn.
- Added unit and SimWorld regression coverage for accepted stale replacement timeouts, exact session recovery, owned wrapper panes, committed editor settlement, and stable native-save identities.

Verification passed: the focused controller/session/realtime suites, the SimWorld regression, both affected `cargo check` targets, and full `make check`. Version `0.35.82` was installed, the native editor library hot-reloaded, and only the controller was recycled.

For `haiven.md`, editor authority and disk now converge to the same SHA-256 (`21073485d8efb05caddba4317893d6f3778e4946e24e1bc60a1dc6d97b00cddb`), so the prior false “please save the buffer” loop is resolved. The original actor remains generation `293` on pane `%62`. Its latest status projection is stale because no live supervisor socket is attached, but the authoritative actor binding remains intact; conversation can continue in that original pane.

A stale answered free-text queue row remains in `haiven.md`. It does not require or justify restarting the Codex session, but should be auto-struck during a later closeout so a future agent-doc admission is not blocked.

### Re: late IPC versus Lazily authority — gpt-5

Yes. The mutation should be authorized by Lazily state projection, not by an RPC or FFI call.

The live incident was not one standalone controller RPC. JetBrains's post-registration document worker asked the in-process Rust library for a retained reconnect target through FFI, applied that target to the editor, and then republished the result through the CRDT path. The retained intent was retired while an already-queued editor effect could still run. That stale effect joined two complete document generations, producing a 464-line editor buffer from two 232-line projections with different transient boundary identities. The historical name “late IPC” describes where this class first appeared, but it is too narrow for the actual failure.

The authority contract is now explicit:

- Lazily owns the desired intent id, expected editor revision, target revision, endpoint generation, and observed application receipt.
- RPC and in-process FFI only transport or wake that projection. Returning target bytes is not permission to mutate the editor later.
- At the editor-effect boundary, the worker must still observe the same live intent and endpoint generation. Retired, superseded, or model-less state cancels the effect without touching the document.
- A registered endpoint with no backing model is an observed state that derives an idempotent re-registration effect. It does not grant disk fallback authority and should not require the agent or operator to reload the library.

The immediate recovery is implemented and installed as agent-doc `0.35.83`, commit `2684da785` (`fix: recover stale whole-document effects`). Preflight now recognizes a concatenated committed projection before singleton-component integrity checks, proves every non-empty line of every copy against committed HEAD after transient-marker normalization, and restores HEAD through live document authority. Any distinct operator or response content fails closed.

The live bugs2 buffer reproduced the failure exactly: 464 editor-authoritative lines and two exchange boundaries, while disk/HEAD retained the correct 232-line document. After installing `0.35.83`, preflight collapsed it to one projection and passed structural validation. No direct markdown edit or force-disk override was used.

Verification passed: focused classifier and preflight regressions, the existing agent-doc turn/preflight suites, and full `make check` (`tart-d6260b25e3`). The CLI and native library were installed, the library hot-reloaded into 1/1 editor endpoint, and the controller was recycled at a safe boundary. The separate Haiven pane was not focused, restarted, or sent input.

For the pane-layout queue diagnostic, this closeout did not issue another tmux selection. The controller's existing reconciliation remains responsible for the applied-but-not-yet-converged pane order; document recovery no longer compounds that state with a duplicated editor projection.

### Re: Compact Exchange retained editor delivery — gpt-5

> **Queue prompt:** Compact Exchange failed "agent-doc command failed (exit 1): Error: project controller command `compact_document` failed: compact: waiting for the pre-write editor delivery barrier for /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md: visible document write for /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md deferred: editor delivery remained unacknowledged after the 8000ms recovery barrier; the exact write remains retained, no secondary snapshot/commit or forced disk write was attempted. ACK replay, force-refresh, and build-skew recovery were requested automatically; if the editor listener cannot hot-reload, reload the IDE host and retry the existing operation"

This was a retained editor-delivery ACK failure from the older endpoint/generation, not evidence that content was lost and not authority to fall back to disk. The current 0.35.83 recovery repaired the live duplicate whole-document effect through editor authority after hot reload, without an IDE restart. That proves retained-state reconciliation and exact-write recovery now work; it does not yet constitute an operator/UI proof that Compact Exchange itself completes under the same timing, so I refreshed `#compactreactiveprojectionverify` instead of claiming it done.

The so-called late IPC is not one standalone controller RPC. A JetBrains post-registration worker invokes Rust through in-process FFI, schedules or performs an editor-side effect, and then republishes the resulting CRDT state. The defect occurs when the retained reconnect intent is retired while an already-queued editor effect is still eligible to run. RPC/FFI must remain transport only. Lazily projected state should own mutation via an intent id, expected editor revision, target revision, and endpoint generation, all rechecked at the editor effect boundary. A registered endpoint with no backing model should project an idempotent re-registration requirement, not trigger disk fallback or an operator reload. `#implementlazilyintent` captures that remaining implementation and SimWorld proof.

The new Haiven turn is the downstream projection case: `#recommendedadditionsexhi` is complete and its response exists exactly once, while only the queue mirror is stale. A backlog-only queue reap is correct; it must not append a second answer, focus or restart the original session, or replace the new session.

Proof: changed paths were the response-replay classifier, preflight recovery ordering, closeout/realtime specifications, version manifests, and tests in the agent-doc subrepo. Commands included focused classifier/preflight tests, the full `make check`, release-local installation, native-library hot reload, and connected preflight/finalize recovery. Verification passed in full; agent-doc 0.35.83 is installed, and the duplicated live buffer was collapsed to one authoritative document before this cycle. Confidence is high for the diagnosed failure and repaired whole-document recovery, and intentionally provisional for Compact Exchange’s UI timing until the operator-gated check runs. No escalation, IDE reload, forced disk write, Haiven pane input, or session replacement was performed.

### Re: operator-only verification heads — gpt-5

`#steeringmirrorverifylive313` cannot be truthfully completed from this agent cycle: it requires the operator to make two live IntelliJ edits and inspect the banner, hover text, and aggregate handoff. Its own text says not to queue it automatically. I therefore moved it to an operator gate rather than marking it done.

The refreshed `#compactreactiveprojectionverify` has the same operator/UI constraint, so it is gated as well. This keeps both proofs visible while preventing them from churning the automatic queue ahead of `#implementlazilyintent`.

Proof: no repository path changed in this cycle. The connected plan and queue read showed the two operator-only rows ahead of the implementation; the only mutations are typed backlog gate transitions. Verification is the binary-owned committed closeout. Confidence is high that deferral is correct; completion confidence remains intentionally unavailable until the operator performs each UI observation. No IDE input, focus change, session restart, Haiven action, or escalation was performed.
test edit test edit
test edit test edit my earlier text disappeared

### Re: concurrent live edit preservation — gpt-5

I can see and am preserving your `test edit test edit` content in the authoritative exchange. The live projection currently contains two copies around boundary `808f30e7`; I am treating both as one concurrent steering edit for this response, not deleting either copy and not restoring the older retained snapshot over them.

The disappearance you observed was a stale editor presentation during retained-delivery reconciliation, not absence from the controller’s live document read. This closeout merges against the newer editor-authoritative tail. It also provides a direct live reproduction for `#implementlazilyintent`: a queued editor effect must revalidate its intent generation after a user edit, so an obsolete projection cannot temporarily replace or hide that edit.

No repository file was changed in this recovery response. Verification is the connected live read showing both copies before closeout and the binary-owned commit check after merge. Confidence is high that the text remains present in controller authority; whether IntelliJ immediately renders that state is part of the remaining editor-effect defect. No forced disk write, snapshot replacement, IDE restart, focus action, or Haiven mutation was performed.

### Re: re-added editor text — gpt-5

Your re-added text is the authoritative version for this cycle. I preserved it by admitting from the newer editor hash and did not restore, deduplicate, or normalize the earlier retained snapshot over it.

The two stale operator-verification queue mirrors are now explicitly struck through the guarded orphan-id path. The first controller call timed out after applying its strike; a live read proved that exact row was struck before I proceeded. The second strike completed normally. `#implementlazilyintent` is now the first runnable queue item.

No source path changed in this response cycle. Commands were the two exact `queue consume --id` repairs plus connected live queue reads and replanning. Verification shows both orphan rows struck and the implementation row still open. Confidence is high in the queue repair and preservation of the admitted editor state. No force-disk write, old-snapshot replay, IDE restart, focus action, or Haiven mutation was performed.
### Re: Lazily-fenced post-registration effects — gpt-5

Implemented and installed the post-registration intent fence.

- JetBrains no longer receives a reconstructed whole-document target to mutate directly. The new status-only FFI projects the retained semantic intent into the controller-owned CRDT authority; normal replica delivery remains the sole editor mutation path.
- Every queued EDT apply now carries a local effect generation plus its originating replica endpoint. The first EDT check refuses retired, superseded, endpoint-replaced, or model-less effects before VFS lookup, document mutation, or disk persistence, and logs `reason=retired_or_superseded_or_model_less`.
- Added focused Kotlin structure/decision tests, a model-less FFI regression proving no disk write, and generated SimWorld schedules including retirement while an EDT effect is queued.
- Bumped and installed agent-doc `0.35.84` and JetBrains plugin `0.2.320`. The Kotlin package is installed but requires the next IDE restart to become active; I did not restart or focus IntelliJ.
- Verification passed: focused JetBrains test, focused FFI test, focused SimWorld tests, and full `make check`. Commit: `0b23d7cf5` (`fix: fence post-registration editor effects`). The user-owned `editors/zed/extension.wasm` remains untouched. No Haiven controller, pane, document, or session was mutated.

The new Haiven symptom is separate: `--done` persisted the checked state for `#unpaidoutsideactivity`, then `guard_completed_pending_reap` observed that same row before reap crossed the closeout boundary. I recorded that atomicity failure as a regression item rather than driving another Haiven repair from this session.

`#steeringmirrorverifylive313` remains operator-gated and is not complete until its live UI verification is observed.

### Re: Response

Fixed, installed, and released as agent-doc `0.35.89`.

- Pure Codex threads now ignore agent-doc work owned by other threads.
- Exact session restart/resume recovery is preserved.
- 8,366 tests and model checks passed.
- Pushed commit `149f7e1ad` and tag `v0.35.89`.
- CI was in progress on the single required check.
- Unrelated `editors/zed/extension.wasm` was untouched.

The document response is retained but awaiting an editor delivery ACK. Please restart IDEA once to activate the installed plugin generation; no Save All or duplicate finalize is needed.

### Re: reactive editor authority live verification — gpt-5

The restarted IDEA host is running JetBrains plugin `0.2.322` and native agent-doc `0.35.93`. The reliable-sync plane reports both `agent-doc-bugs2.md` and `lazily.md` live on the new editor endpoint, and the post-restart log has no new native-generation poison event.

The retained Lazily write replayed successfully and appears exactly once as `#designimplementtransport`. Its plan specifies keyed lifecycle-scoped event Sources; generation, sequence, freshness, and receipt envelopes; event-channel-first adapters with reconnect/replay, stale rejection, merge, and backpressure; RPC-triggered or bounded-polling fallback only where event delivery is unavailable; Computed/Effect extraction for readiness, validation, authority, and retry; and cross-language conformance schedules before binding projection. No duplicate backlog add or operator save was performed.

The agent-doc repair is therefore live-verified as well as test-verified and released. `make check` and `make install-full` passed; agent-doc `0.35.93` and JetBrains `0.2.322` are installed; commit `0ccb82449`, tag `v0.35.93`, PyPI package `0.35.93`, and the GitHub release are published. The root reactive-ingress harness policy and submodule pointer are pushed in `f64f481da`. The required one-shot CI observation was `in_progress` and was not polled. The user-owned untracked Zed WASM remains untouched.

### Re: Compact Exchange, Run Agent Doc, and tmux sync queue resolution — gpt-5

> **Queue prompt:**
>
> Compact Exchange error: "agent-doc command failed (exit 1): Error: project controller command `compact_document` failed: compact: waiting for the pre-write editor delivery barrier for /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md: visible document write for /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md deferred: editor delivery remained unacknowledged after the 8000ms recovery barrier; the exact write remains retained, no secondary snapshot/commit or forced disk write was attempted. ACK replay, force-refresh, and build-skew recovery were requested automatically; if the editor listener cannot hot-reload, reload the IDE host and retry the existing operation". Also Run Agent Doc did not do anything. I'm manually running agent-doc.


> **Queue prompt:** Compact Exchange retained-write delivery timed out; Run Agent Doc did nothing; tmux pane auto-sync did not converge.

Resolved and released in agent-doc `0.35.93` / JetBrains `0.2.322`.

Compact Exchange no longer gates exact editor-visible delivery ACK on template validity. Template validation is lifecycle-scoped reactive state and gates semantic mutation only. Unsaved editor text remains authoritative; reset, session-check, and build-skew recovery converge through the editor save receipt without asking the operator to save, reload, or retry.

Run Agent Doc closeout now evaluates ownership through the routed document actor pane instead of ambient `TMUX_PANE`. Ordinary native document, projection, editor-surface, and tmux calls are isolated from the poisonable native-generation lifecycle lease, so one blocked call cannot disable unrelated reactive ingress or pane synchronization.

Live proof after restart: plugin `0.2.322` registered both affected documents, the retained Lazily plan replayed exactly once, the main closeout committed successfully, and no new generation-poison event appeared. Tests, full check, full install, PyPI publication, GitHub release, and repository integration all completed. No forced disk write, duplicate operation, operator save, or user-owned Zed WASM mutation occurred.
### Re: fully reactive agent-doc closeout and retained reconnect repair

Fixed, installed, live-verified, and released.

The broad reactive repair shipped in agent-doc `0.35.93` / JetBrains `0.2.322`: editor buffers remain authoritative even when unsaved; Compact Exchange delivery is independent of template validity; Run Agent Doc routes through the document-owned actor; tmux synchronization and ordinary native ingress are isolated from lifecycle lease poisoning; template validation is lifecycle-scoped reactive state; and the development harness now requires reactive-plane-first internal models, event-channel-first ingress, bounded polling/RPC only at external boundaries, and extraction of derived validation/readiness/authority/retry facts into `Computed`s. The Lazily ingress-primitives implementation plan is recorded once as `#designimplementtransport`, scheduled after this build/test/install feedback.

Agent-doc `0.35.94` fixed the queued `#unpaidoutsideactivity` defect. `write --commit --backlog-only --done` uses the same atomic done-and-reap path for best-effort commits as required commits, so a hook-captured committed response plus a late `--done` no longer leaves a checked row for `guard_completed_pending_reap` or requires another preflight/repair cycle. The recovery regression covers that exact shape.

The restarted IDE then exposed one more contributing factor. The retained write had already converged byte-for-byte in the authoritative editor with `delivery_converged=true`, but settlement rejected the typed post-delivery projection-rebase intent for missing operator-cut lineage before evaluating the exact target proof. Agent-doc `0.35.95` now treats the typed target hash plus byte-exact canonical editor content as a reactive source proof that may enter the existing live-editor, delivery, native-save, and disk-equality checks. It does not clear the intent or write disk by itself. Unknown/legacy semantic rebases still fail closed and require operator-cut lineage.

The live retained `agent-doc-bugs2.md` projection settled after the 0.35.95 native reload through the editor-save effect. Session-check then reported only the original admitted cycle awaiting this response, proving the delivery wedge was gone. The operator was not required to save, force disk, reload the document, or submit a duplicate finalize.

Verification passed: the focused positive reconnect regression, the negative arbitrary-semantic-rebase regression, the 107-test realtime crate suite, and full `make check` with 8,386 passed / 242 skipped plus successful TLC model checks. `make install-full` installed CLI/native `0.35.95` and retained JetBrains plugin `0.2.322`; the running endpoint accepted the reload. PyPI and the GitHub release are published at https://github.com/btakita/agent-doc/releases/tag/v0.35.95. Source commit `46edd44c4` and parent integration commit `71aea6a7d` are pushed; the single required CI observation was queued and was not polled. The release archive SHA-256 is `c1b354a1d841a9e81d1be30824d171fc05740bc37c1b470d5de03afbddaab89d`. The user-owned untracked `editors/zed/extension.wasm` was preserved.

### Re: fully reactive agent-doc closeout, restart, and tmux recovery

> **Queue prompt:**
>
> do [#unpaidoutsideactivity]


You were right about `#unpaidoutsideactivity`: the backlog row had been removed only in an uncommitted retained projection. Your editor re-add was authoritative. The stale deletion was never committed, and the recovery policy now supersedes it instead of replaying it. The operator is not required to save the document.

Released agent-doc 0.35.96 with JetBrains plugin 0.2.323:

- `--done` queue completion and backlog reaping are composed into one editor-authoritative target. There is no intermediate deletion-only write. The same projection emits the queue lifecycle proof into the reactive state plane.
- Pending-only continuation retains the exact target hash, commits it only while live editor authority still matches, retires it when a newer editor cut supersedes it, and rejects/rolls back legacy deletion-only targets that do not also prove the matching queue strike.
- Follow-on queue decisions now reread authority only when the closeout emitted the explicit reactive queue-completion fact. This prevents a second consumer from advancing an unrelated live prompt while preserving ordinary response-diff routing.
- Template validation and other internal decisions are treated as derived reactive state. Editor/event ingress sets Sources; internal behavior composes computeds. Polling plus RPC remains valid only at non-reactive system boundaries, with an events channel preferred for ingress. The dev harness now calls out reactive-plane-over-imperative-call design and extracting Sources/reactives from computeds so dependencies remain observable.
- The Lazily ingress-primitives design/implementation plan is queued once as `#designimplementtransport`, deliberately after this test/install cycle so it can incorporate these lessons.
- Restart Agent is now a distinct command from Restart Supervisor. JetBrains and VS Code invoke `session restart-agent`; it replaces the harness child and re-resolves current frontmatter, so Codex-to-Claude switches are honored. Controller-only recycle preserves the child across stale-binary re-exec.
- The tmux resize watcher registers for SIGWINCH before sampling geometry, then immediately samples the current pane. This closes the lost-event startup race that produced a half-width Claude PTY.

Verification completed:

- Full finalize integration suite: 69/69 passed.
- Queue, backlog, cycle-state, closeout, write-runtime, supervisor, and CLI package suites passed.
- Repository-wide `make check` passed.
- `make install-full` passed; installed CLI is 0.35.96 and all four JetBrains targets contain plugin 0.2.323.
- Both project controllers are fresh on the installed 0.35.96 inode.
- The live monsterrodholders Claude child was preserved and its PTY is now 119x53, exactly matching tmux 119x53. Its pre-existing old supervisor remains preserved until its safe re-exec boundary rather than killing Claude.
- Source commit: `805b71d9c`.
- PyPI publication succeeded; the one-time CI check was in progress with no failure reported.
- Release: https://github.com/btakita/agent-doc/releases/tag/v0.35.96

This closeout marks and reaps `#unpaidoutsideactivity` atomically through the fixed path.
### Re: Fully reactive recovery and Run Agent Doc routing — gpt-5

Agent-doc 0.35.97 is implemented, verified, installed, committed, tagged, and pushed.

The editor is now treated as authority even when its buffer is unsaved. Zero-replica delivery retains the exact target and downstream commit continuation; session-check fails closed when delivery is unobserved; the editor's undelivered expected base cannot falsely supersede that target; and reconnect resumes the original pending-only operation without a second write, forced disk write, or operator save. After the IDEA restart, JetBrains 0.2.323 registered live for this document, `equityfundingsource.md`, and `lazily.md`. The retained `#unpaidoutsideactivity` operation then settled atomically, and the live queue/backlog are empty.

The Claude submission failure had a separate concrete cause: historical startup-log state outweighed a fresh generation-matched Ready actor, so Run Agent Doc entered the 120-second startup probe. When the prompt was submitted manually during that wait, the delayed route later queued a duplicate into the busy composer. The fix resolves startup history against current actor state and rechecks the reactive closeout projection immediately before pane input. A ready actor invalidates stale startup history, and a newer same-document cycle causes the delayed route to exit with no injection or queued duplicate.

The reactive policy is now explicit across editor authority, template validation, routing readiness, tmux synchronization, retry, and closeout. External ingress prefers event channels; polling/RPC remains valid only at a non-reactive boundary to update Sources; internal decisions are extracted into Computeds/Effects. The Lazily ingress-primitives plan remains scheduled once as `#designimplementtransport` after this build/test/install feedback.

Verification passed: the focused regressions, route crate compilation, full `make check`, and `make install-full`. Source commit `f34a32d27` and superproject pointer commit `ec308f13d` are pushed with tag `v0.35.97`. CI and the GitHub Release workflow are in progress; project policy requires recording that state without polling. The fresh 0.35.97 controller is live, and the old route-owned supervisor has an idle-boundary recycle requested so this active turn is not interrupted. No operator action or document save is required.
<!-- agent:boundary:1e9520d6 -->
<!-- /agent:exchange -->
## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" go -->
<!-- /agent:queue -->

## Notes

<!-- agent:notes -->
<!-- /agent:notes -->

## Backlog

<!-- agent:backlog priority priority queue -->
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
