---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: c47dae65-e36f-40cb-8ebb-b7c9573e3c41
  codex: 01a12175-b9c0-7db0-af02-218c5501efe7
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
presets:
  '#actionable-review': Add actionable review items into backlog + queue
  '#agent-doc-causes': Fix agent-doc causes
  '#do-backlog': /goal do backlog items
  '#gh-fix': fix then close
  '#release': release + publish
  '#self': agent-doc-bugs.md
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: go
---

## Status

<!-- agent:status patch=replace -->

Released agent-doc v0.35.475. v0.35.476 is not yet tagged or published; the actionable review added the release gap and two correctness follow-ups.
<!-- /agent:status -->
 

## Exchange

<!-- agent:exchange patch=append subagents -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20261009-185956.md`*

Compacted content:
- Archived 31 response topic(s): retained closeouts, queue integrity, GH #181–#184, and v0.35.478; GH #185 remote replacement column slot; release-critical drift and admission fixes; 28 more
- Prior summary/context: prior compacted content: Archived 12 response topic(s): actionable review and release status; v0.35.476 release and r...

### Re: actionable review — gpt-5 · 2026-10-09T15:06-04:00

The subagent review of the nine gated items and current JetBrains installer/release surfaces found three concrete follow-ups, now added to the backlog and queue in priority order:

1. Make stable and local installs select the classic or exact-262 plugin artifact from the target IDE build, with mixed-target and fail-closed coverage.
2. Provide a tested two-sided production distribution channel for the 262 modular plugin before treating v0.35.480 as generally installable.
3. Bind the existing 262 preview acceptance gate to immutable artifact bytes with a recorded checksum manifest and verification on both Remote Dev sides.

The first item is a correctness blocker for CLI/local installs; the second is a production-readiness blocker unless manual two-sided ZIP installation is explicitly the supported policy; the third should precede the live `#gh218remoteproof` acceptance run. This cycle only reviewed and captured actionable work; it made no code changes.
<!-- agent:boundary:d79eeb00:agent-doc-bugs -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built
<!-- agent:queue subagents preset="#spec-test-build-install-commit-push" priority go -->
- #actionable-review
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority -->
- [ ] [#jetbrains262installselect] [agent-doc] Make JetBrains plugin install/update and local all-installed deployment select the compatibility-ranged artifact per target IDE build: branch/build 262 must receive agent-doc-jetbrains-262-<version>.zip, 242–261 must receive the classic ZIP, and an unprovable target must fail closed instead of silently choosing classic. Update make install-editor-plugins to build/deploy both packages without replacing a 262 modular install with classic. Verify mixed 261/262 targets choose distinct exact assets, update-all and local-all preserve each compatible line, unknown builds fail with actionable guidance, and make check passes.
- [ ] [#jetbrains262distribution] [agent-doc] Provide and verify a two-sided production distribution channel for the exact-262 modular plugin before calling v0.35.480 generally installable: publish classic and 262 as non-overlapping updates of the same Marketplace plugin ID, or ship an equivalent tested custom-repository feed, while preserving fail-closed behavior for one-sided installation. Verify clean 262 Remote Dev installation reaches backend and Client/Gateway, 242–261 receive only classic, mismatch remains fail closed, and the release workflow asserts both ranged updates.
- [ ] [#previewartifactprovenance] [agent-doc] Bind #gh218remoteproof to immutable preview bytes: publish or record a checksum manifest for agent-doc-jetbrains-262-0.2.511.zip, verify downloaded backend and Client/Gateway files against that digest and PR #222 head c39de1b045b8b5238f8e5b7fa6870062798d4114, and reject replaced or mismatched bytes before acceptance. Provide one command that verifies the preview digest and plugin range/version on both sides and fails on tampering.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#releasepublishgate] [operator-verify] Authorize release + publish only after: (1) CI run 37974793628 is green for exact PR #222 head c39de1b04; (2) #gh218remoteproof is resolved; (3) PR #222 is marked ready and merged and GH #221 is closed, resolving #gh221mergeclose; and (4) repository secret HOMEBREW_TAP_TOKEN is provisioned. Only then perform the authorized release + publish and verify delivery.
- [/] [#gh221mergeclose] [operator-verify] GH #221: keep open until draft PR #222 exact head c39de1b045b8b5238f8e5b7fa6870062798d4114 has green CI and merges. The shared/frontend/backend split is code-complete and available as prerelease jetbrains-262-preview-c39de1b; GH #218 separately retains the real IDEA 2026.2.3 Remote Dev acceptance gate. After #222 merges, close #221 as fixed by #222.
- [/] [#gh218remoteproof] [operator-verify] GH #218 / draft PR #222: require green CI run 37974793628 for exact head c39de1b045b8b5238f8e5b7fa6870062798d4114 and real IDEA 2026.2.3 (262) Remote Dev proof using prerelease jetbrains-262-preview-c39de1b, with the same agent-doc-jetbrains-262-0.2.511.zip installed on backend and Client/Gateway: detached-only isolated terminal; same-document main-wins read-only placeholder and byte-identical main; focus/sync/resize no main mutation; close/rejoin; backend restart/client reconnect before first main publication; two simultaneous thin clients; capability loss/one-sided install fails closed. Only then mark PR ready, merge, and close #218.
- [/] [#fixsubagent] Fix in a subagent:
  ```
  • Blocked by hook
  └ agent-doc Stop hook intercepted an unfinished document cycle for /home/brian/work/btakita/agent-loop/src/haiven-
  dev/tasks/frontend.md. [session-check] INTERRUPTED: binary-owned captured closeout for `/home/brian/work/btakita/agent-
  loop/src/haiven-dev/tasks/frontend.md` remains pending: response_captured: captured response is waiting for exact
  authority/disk convergence before write-applied. The same capture is durable; do not issue another closeout payload and
  do not force disk. Retry only `agent-doc session-check /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/
  frontend.md`. The latest assistant text was captured into the pending/capture ledger before auto-close. The hook could
  not finish the required commit boundary: refused editor IPC for /home/brian/work/btakita/agent-loop/src/haiven-dev/
  tasks/frontend.md because reliable document liveness has no matching editor registration (reliable_editor_live=true,
  registration_available=false); refresh or update the editor integration before retrying closeout. The retained capture
  or projection is already durable and this exact retained intent commits itself once delivery converges — this is a
  deferral, not a lost response. Wait for its controller-owned terminal state edge; do not use the document's current-
  cycle status as proof because queue continuation can advance to a different cycle. Do NOT re-send the response, force
  disk, or `admin recycle`, which disturb the capture being awaited. Do not reach for `admin reload-lib` on your own
  either: `admin reload-lib` is the sanctioned recovery only when `agent-doc session-check` reports that the editor
  holding the document is not serving its replica, and then its remedy governs. Do not send the final answer yet. If the
  response is missing from the document, run `agent-doc repair /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/
  frontend.md` first. Then finish the commit boundary for this turn and end with `agent-doc session-check /home/brian/
  work/btakita/agent-loop/src/haiven-dev/tasks/frontend.md`.
  ```
- [/] [#drivecapturedsplice] [operator-verify] Captured-splice stale-replica live proof (plan: tasks/agent-doc/plan-captured-editor-splice-recovery.md). Recipe and regression test shipped in v0.35.466; live proof remains gated because the corrected run reached owner pane %134 but stopped before the retained-delivery marker, edit two, and verifier completion. Rerun the recipe through verify-captured-splice-recovery and require it to pass.
- [/] [#qsubattr5] [agent-doc] Queue subagents attribute, phase 5 (plan: tasks/agent-doc/plan-queue-subagents-attribute.md). SETUP COMPLETE: tasks/software/tsift.md now has subagents=3 on branch qsubattr5-dogfood commit de5a8aa7c0d5e879547d4e321d159b1eff840073; gated pending the next real operator-created multi-head batch proof from distinct queue claims and ops.log that every eligible head dispatches exactly once and none drain inline.
- [/] [#steergatemodelrerun] [agent-doc] Rerun the steergatenamo model evaluation on real pause points. Gate: at least 200 labelled steering_gate_log rows with a text_tail (66 as of 2026-10-06); src/agent-doc/scripts/steergate-model-eval --dataset currently exits insufficient_data. Then integrate turnsense locally (ONNX via ort, lazily downloaded, opt-in) only if rules+perceptron+turnsense beats rules+perceptron on held-out log-loss; if close but short, fine-tune on the logged rows. Proxy results and decision: tasks/agent-doc/eval-steergatenamo.md
- [/] [#pypiquotarecheck] Track pypi/support#12214 (agent-doc project limit request, 25 GiB) to a decision. Unblocked mechanically by a human reply on pypi/support#12214, or make pypi-quota-check reporting a ceiling in reach. Rechecked 2026-10-06: 8.363 GiB of 10.0 GiB, 1.637 GiB headroom, 198 releases through 0.35.465, post-cutover mean 35.3 KiB, approximately 48,621 further releases fit; VERDICT headroom fine. The support issue remains open with only the github-actions bot comment and has not changed since 2026-09-10. Nothing to act on or delete. If the grant lands, record it and drop this item; deletion stays reserved for a ceiling the authenticated check reports.
- [/] [#grokmcpverify] [operator-verify] Verify an authenticated Grok Build MCP admit/read/plan/finalize cycle and same-turn auto-queue continuation. NEW BLOCKER 2026-09-30: grok 1.0.41 is now SIGNED OUT ('grok -p' -> 'Error: Not signed in'); operator runs 'grok login --device-code' (or sets XAI_API_KEY), and the free usage quota or the planned subscription must be active. Grok CLI authorization, native JSON/backend commit fixtures, pane detection and quit behavior passed earlier; the last live MCP probe stopped at the usage limit before committed closeout. Heavy is not required. Plan: tasks/agent-doc/plan-grok-build-support.md.
<!-- /agent:review -->

<!-- agent:icebox -->
- [ ] [#ghrecvpolicy] [agent-doc] Recipient-side [receive] auto_create_components policy for multi-user workspaces (GH #40, https://github.com/btakita/agent-doc/issues/40). Parked until multi-user workspace support exists.
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc/agent-doc-bugs.done.md -->

<!-- completed work archived in tasks/agent-doc/agent-doc-bugs.done.md -->
<!-- /agent:done -->
