---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: c47dae65-e36f-40cb-8ebb-b7c9573e3c41
  codex: 01a0db52-4c0d-72d3-b2c9-0ac89aff42a0
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#actionable-review': Add actionable review items into backlog + queue
  '#agent-doc-causes': Fix agent-doc causes
  '#do-backlog': '/goal do backlog items'
  '#gh-fix': 'fix then close'
  '#self': agent-doc-bugs.md
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: start
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->
 

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20260930-234705.md`*

Compacted content:
- Archived 10 response topic(s): #advance-review; Drive Idea for #drivecapturedsplice; #coder7; release + publish; 7 more
- Prior summary/context: prior compacted content: Archived 24 response topic(s): Compact Exchange needed two tries (+ fpe.md stuck closeout); ...

### Re: do [#refusalsteeringverbatim] + the lazily.md Stop-hook failure — opus-5.5 · 2026-09-30T20:14-04:00

> **Queue prompt:** do [#refusalsteeringverbatim]

**`#refusalsteeringverbatim` is done** in agent-doc `7f4e1f968` (pushed and installed).

- **Cause:** a refused preflight tells the agent that operator steering "may be waiting unanswered" and names `session-check` as the way to read it. But when the editor authority and disk diverge, both session-check divergence INTERRUPTs exit before the steering scan runs, so a prompt that exists only in the unmerged editor save showed up nowhere.
- **Fix:** a new `unmerged_editor_steering_note(authority, disk)` diffs disk → authority with the same `realtime_steering_all` policy. Both INTERRUPT messages now append that steering verbatim ("relay it, do not answer until the save settles"). The refusal payload wording says session-check covers this case.
- **Test:** an authority-only operator prompt is relayed verbatim, and there's no note when the two match.
- **Verification:** `make check` passed (exit 0; nextest 10263/10263 passed, 259 skipped). `make install` and `cargo build --release` succeeded.

**The `lazily.md` Stop hook failure (your mid-turn report).** It has recovered: session-check on `tasks/software/lazily.md` is OK, and the latest cycle is committed. From ops.log:

- The named intent `1790781617611427022` comes from the 11:20 turn. It settled at 15:20:24Z.
- At 00:08:04Z the controller recycled. My `make install` set that off: the fan-out recycles live controllers. The fresh controller re-seeded the editor replica with retained state still tagged `turn=cycle-1790781610027`.
- The Codex Stop replay then attempted a write whose target hash equals that old intent's target, `f42c0100…`. That was already the live authority at 00:08:13, so the replay was a no-op. It failed closed only because the document model was missing mid-reseed, and `ensure_deferred_document_write_intent` reused the old intent id.
- The capture resumed and committed at 00:09:17.

**The defects behind it are not fixed yet.** I captured them as `#stopreplaynoopretained` with the evidence:
- a no-op replay should not fail closed on a transient missing model;
- the settled intent should not still be eligible for reuse;
- `derive_settled_closeout_resume_signal` pairs the current capture with an unrelated `latest_converged_write`.

I stopped here instead of patching: whether the settled intent really survived in `pending_write` across the recycle needs a ledger trace I haven't done yet.

Next queue heads: `do [#supdrainyieldfalsifiable]`, then `#release`, which is still carried-skipped; see my previous question about fixing that.

### Re: #gh-fix GH #76 + fixing agent-doc's causes of the recycle churn — opus-5.5 · 2026-09-30T20:35-04:00

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/76

**Both are done** in agent-doc `6cb1ede86` (pushed, `make install` only). #76 is closed, with a comment explaining what I changed.

**The recycle churn: fixed (`#supdirstale`).**
- **Cause:** host-supervisor staleness compared the supervisor's running inode with the inode of the CLI's *own* binary. There are two launchable copies of one build: `~/.cargo/bin` from `make install`, and `target/release` behind the `.bin/agent-doc` newest-build shim. A CLI on one copy flagged every supervisor on the other as stale, *even a newer one*, and requested a recycle at every stage (`preflight_entry`, `commit_start`, `finalize_write_start`…). The re-exec went back through the shim onto the same newer copy, and the next command asked again. The live supervisors were measurably split between the two copies, and infra's supervisor re-exec'd three times in 2.5 minutes, each time refusing admission. That is your 7:27 `infra.md` refusal.
- **Fix:** staleness is now directional. A supervisor is stale only if its running bytes were unlinked or the CLI's binary is a *strictly newer build*. The new pieces:
  - the pure rule `host_supervisor_build_is_stale` in `agent-doc-supervisor`;
  - `BinaryBuild` probes in `agent-doc-fs`, which read the running binary's mtime through `/proc/<pid>/exe`;
  - one helper, `host_supervisor_binary_is_stale`, shared by all three recycle-driving sites;
  - `supervisor_stale_warning_message` now uses the existing `controller_binary_identity_is_newer` instead of an equality check that includes the path.
- **Workflow half:** the per-fix `cargo build --release` I had been running added a second recycle wave. I removed that from my workflow and corrected the memory note that prescribed it.

**#76: fixed.**
- `editor_route` now refuses *before dispatch*, with the pid, the mapped jar and the remedy, when the document's live JetBrains editor maps superseded plugin bytes. Previously it burned the full 125s.
- The probe moved from `agent-doc-preflight-io` to `agent-doc-fs` (re-exported), so the controller and preflight share one implementation.
- I kept the established remedy ordering (try the restart-free update first; restart only after an install records the IDE refused it) because existing tests pin it deliberately.
- One correction to the issue: the 125s wait is on the controller's own worker, not on an editor reply.

**Verification:** `make check` passed (exit 0; nextest 10265/10265, 259 skipped). Tests cover the directional rule across six cases and the route refusal.

**Not done (now in the backlog):**
- `#queueselectreversed`: this cycle's contract listed the queue in *reverse* order and selected the carried-skipped `#release`. I drained #76 by document order instead.
- `#releaseskipcarried`: removing a `⏭️` doesn't clear the carried skip.
- `#gh76secondary`.

Remaining queue: `do [#supdrainyieldfalsifiable]`, then `#release`, which will stay skipped until `#releaseskipcarried` is fixed or the line is rewritten.
<!-- agent:boundary:4df2c95c:agent-doc-bugs -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
- ~~#gh-fix https://github.com/btakita/agent-doc/issues/73~~
- ~~#gh-fix https://github.com/btakita/agent-doc/issues/74~~
- ~~#gh-fix https://github.com/btakita/agent-doc/issues/75~~
- ~~Do the "things you need from me"~~
- ~~do [#refusalsteeringverbatim]~~
- ~~#gh-fix https://github.com/btakita/agent-doc/issues/76~~ — auto-struck: answered this cycle (#ftstrike)
- do [#stopreplaynoopretained]
- do [#supdrainyieldfalsifiable]
- 🚧 #release
<!-- /agent:queue -->


<!-- agent:backlog -->
- [ ] [#queueselectreversed] [agent-doc] 2026-10-01 preflight selected '#release' (carried ⏭️) as head while queue_prompts listed the live heads in REVERSE of document order (doc: supdrainyieldfalsifiable, #gh-fix 76, #release → contract: #release, #76, supdrain). Operator-authored order is authoritative (#qauthorder).
- [ ] [#releaseskipcarried] [agent-doc] An operator removing a head's ⏭️ marker does not clear it from cycle-state skipped_queue_head_ids (#queueskip, agent-doc-preflight-io/src/lib.rs ~4840): '#release' (no backlog item/preset, so never 'consumed') is re-stamped every cycle. Treat a snapshot→document ⏭️ removal as an operator un-skip.
- [ ] [#gh76secondary] [agent-doc] GH #76 leftovers: stale_plugin expects 0.2.453 while the installer leaves 0.2.451 on disk (install vs expected-version disagree); full sync left window 0:1 named 'stash-' holding 19 panes instead of renaming to 'stash'.
- [ ] [#stopreplaynoopretained] [agent-doc] lazily.md Codex Stop hook failed closed 2026-10-01T00:08Z: serialized_atomic_write retained intent 1790781617611427022 (from 11:20, settled 15:20:24Z per retained_write_settled_from_derived_verdict) was re-used because the Stop replay's target hash == that intent's target f42c0100 == the authority already at 00:08:13 — a no-op replay that failed only because the controller had just recycled (make install) and the model was mid-reseed (crdt_replica_register_retained_reseed turn=cycle-1790781610027). Self-healed: captured_finalize_resume_committed 00:09:17. Fix: (1) a write whose target already equals live authority must not fail closed on a transient model-missing; (2) prove whether pending_write still held the settled 11:20 intent across the recycle (ensure_deferred_document_write_intent reuse at agent-doc-document-realtime-io/src/lib.rs:4732) and clear it at settlement; (3) derive_settled_closeout_resume_signal (project_controller.rs) pairs the current capture with an unrelated latest_converged_write — gate it to writes after the capture
- [ ] [#supdrainyieldfalsifiable] [agent-doc][recommended] GH #73 suggestion 3: record the supervisor-drain hand-off (session_check_supervisor_drain_handoff now logs head_sha256 + supervisor_drain_readiness) so the next preflight reports 'yielded N min ago, still undrained' instead of silently re-presenting the same heads
- [/] [#apimdrevert] [agent-doc][operator-verify] src/haiven-dev/tasks/api.md 'reverted by queue updates' (2026-09-29): forensics found NO lost operator text — disk/controller/editor converged at c9273a0b (HEAD 1b432940 + Codex 00:49:49 queue stop+strike, applied as a 2038-byte CRDT delta against the converged base). Only editor-side anomaly: idea.1.log 00:40:02
6local-splice-batch 3 splices 34346->34137 chars, then 00:40:03 exact inverse back to 25509a1e (undo/redo or plugin-internal flip; no VFS reload/projection logged). Blocked on operator: which text was lost + when (JetBrains Local History on api.md). Then trace the dropping path and fix with regression coverage.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#drivecapturedsplice] [operator-verify] Captured-splice live proof (#activateinstalledjetbrai). LIVE RUN 2026-09-30 20:33Z (operator go-ahead): after harness fixes 01b1e30b7, the recipe drove IDEA end to end — edit one captured (editor_op_capture_proof operator_ops=37 shadow_replay=agreed, 20:33:01Z), canonical advanced independently via write --commit in owner pane %93 (20:33:02Z, editor acknowledged the target), edit two captured (operator_ops=36 agreed, 20:33:12Z); the final text holds every operator edit plus the advance. No text lost. The verifier still FAILS: its proving triple needs two source=captured-local-splice-recovery reads, and the plugin only takes that path (CrdtReplicaManager rebootstrapCanonicalAndForwardCapturedLocalEdit) when its replica is STALE against the canonical text. Healthy delivery reached the editor before edit two, so the recovery never had to run. DECISION NEEDED: (a) accept the healthy-path trail as the proof, extending verify-captured-splice-recovery with a capture/advance/capture triple, or (b) require the stale-replica race, making the recipe type edit two while the advance is still in flight. Aside: the scratch doc is under gitignored tmp/, so every advance ends commit_refused_ignored_path (write lands on disk, commit refused). Plan: tasks/agent-doc/plan-captured-editor-splice-recovery.md.
- [/] [#pypiquotarecheck] Track pypi/support#12214 (agent-doc project limit request, 25 GiB) to a decision. Unblocked mechanically by a human reply on pypi/support#12214, or 'make pypi-quota-check' reporting a ceiling in reach. Rechecked 2026-09-30 16:25 EDT: 8.362 GiB of 10.0 GiB, 1.638 GiB headroom, 158 releases (0.35.113..0.35.424), post-cutover mean 34.7 KiB, ~49,485 further releases fit; VERDICT headroom fine. Nothing to act on or delete. pypi/support#12214 still open, 1 comment (github-actions bot), untouched since 2026-09-10T22:46:21Z. If the grant lands, record it and drop this item. Deletion stays reserved for a ceiling the check reports (next tranche 0.35.113..0.35.161, 2.491 GiB); verify any deletion from the authenticated /manage/project/agent-doc/ pages, never from pypi.org/pypi/agent-doc/json.
- [/] [#grokmcpverify] [operator-verify] Verify an authenticated Grok Build MCP admit/read/plan/finalize cycle and same-turn auto-queue continuation. NEW BLOCKER 2026-09-30: grok 1.0.41 is now SIGNED OUT ('grok -p' -> 'Error: Not signed in'); operator runs 'grok login --device-code' (or sets XAI_API_KEY), and the free usage quota or the planned subscription must be active. Grok CLI authorization, native JSON/backend commit fixtures, pane detection and quit behavior passed earlier; the last live MCP probe stopped at the usage limit before committed closeout. Heavy is not required. Plan: tasks/agent-doc/plan-grok-build-support.md.
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc/agent-doc-bugs.done.md -->

<!-- completed work archived in tasks/agent-doc/agent-doc-bugs.done.md -->
<!-- /agent:done -->
