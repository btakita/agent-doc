---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: c47dae65-e36f-40cb-8ebb-b7c9573e3c41
  codex: 01a0fb46-9936-7893-86be-7777f41da86e
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#actionable-review': Add actionable review items into backlog + queue
  '#agent-doc-causes': Fix agent-doc causes
  '#do-backlog': /goal do backlog items
  '#gh-fix': fix then closerelease
  '#release': release + publish
  '#self': agent-doc-bugs.md
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: go
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->
 

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20261002-062128.md`*

Compacted content:
- Archived 34 response topic(s): #gh-fix GH #77 (tmux shows 1 pane while the editor shows 2 documents); #gh-fix GH #78 (preflight overrun refusal had no measured cause); Can we make the "genuinely wedged run" never happen?; 31 more
- Prior summary/context: prior compacted content: Archived 9 response topic(s): do [#refusalsteeringverbatim] + the lazily.md Stop-hook failur...

### Re: #gh-fix issue 95 — gpt-5 · 2026-10-02T02:41-04:00

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/95

Fixed, installed, pushed, and closed [agent-doc issue #95](https://github.com/btakita/agent-doc/issues/95).

The root cause is in the newer JetBrains platform contract: its asynchronous classloader-retirement strategy returns success from dynamic reconfiguration before the outgoing classloader is collected, then waits in the background and may advise a restart later. `JetBrainsPluginUpgradeAction` now owns the invariant that a restart-free upgrade may proceed only when retirement is synchronously observable. On an asynchronous-retirement build it stages the package before any unload probe, preserving the live jars; synchronous builds retain the restart-free path. Refusal, failed unload, retained generation, and successful fresh-generation rows remain distinct.

The replica restart path now rediscovers open agent-doc documents across every live project instead of trusting only the pre-reload handoff map. A `0/0` observation is no longer convergence: it is emitted as `state=no-open-documents` with `live_projects=N`, while partial and complete attachment states remain explicit.

Architecture closeout: the policy owner is the one-shot JetBrains upgrade action; JetBrains platform state is the boundary ingress and the action emits one install receipt. This boundary has no long-lived agent-doc lifecycle graph to join, so the direct platform call remains the justified one-shot exception. The continuing replica lifecycle remains in the existing process-scoped coordinator; no new poller or parallel policy owner was introduced.

Changed surfaces: the JetBrains upgrade action, replica/reload reporting, focused JetBrains tests, and the cross-editor source-contract test. Commits `766d00b9b` and `9d4dcce69` are on `origin/main`; the latter projects JetBrains plugin `0.2.471`, matching the installed artifact.

Verification:

- Focused JetBrains regression suite: 19 tests passed.
- `make dev-harness-test`: passed after updating the cross-editor contract assertion.
- `make check`: passed from the top, exit 0, zero failure signals.
- `make install`: passed; binary/cdylib and editor packages installed, including JetBrains `0.2.471`.
- Latest pre-push CI observation was green for `v0.35.434`; the new push was not awaited.

No imperative derived-state calls were added or duplicated, and no operator-only verification is required for this safety policy: unsupported asynchronous platforms deterministically stage before touching the live generation.

### Re: #gh-fix issue 96 — gpt-5 · 2026-10-02 02:57 EDT

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/96

Fixed and closed [agent-doc issue #96](https://github.com/btakita/agent-doc/issues/96).

The queue policy now treats a missing optional `agent:queue` component as an empty answered-free-text projection. It still propagates every other AST/mutation error, so malformed queue state remains visible instead of being silently swallowed. A regression covers a normal exchange/backlog document with no queue component.

Verification passed:

- focused no-queue projection regression
- full `agent-doc-queue` test suite
- reactive controller projection regression
- full `make check`
- `make install`

Pushed `ff33358bc` (`fix(queue): ignore absent queue in free-text projection`). The repository gate also exposed generation debt from issue #95's VS Code test change, so `b2832879c` bumped and installed VS Code extension 0.2.76 before the full check.
<!-- agent:boundary:1956bca7:agent-doc-bugs -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
- ~~#gh-fix https://github.com/btakita/agent-doc/issues/95~~
- ~~#gh-fix https://github.com/btakita/agent-doc/issues/96~~
- release + publish
<!-- /agent:queue --> 


<!-- agent:backlog priority -->
- [ ] [#ghbrew] [agent-doc] Homebrew formula for agent-doc (GH #31, https://github.com/btakita/agent-doc/issues/31). The tap repo ~/work/btakita/homebrew-tap holds only chromium-bridge.rb today. Add Formula/agent-doc.rb pulling the per-platform GitHub Release archives (binary + cdylib together, GH #52) with SHA256SUMS, and add a release step that bumps the formula on each tag. Close #31 when brew install btakita/tap/agent-doc works.
- [ ] [#ghjbfolding] [agent-doc][jetbrains] JetBrains plugin UX phases 5-7 (GH #19, https://github.com/btakita/agent-doc/issues/19): fold agent:* component regions, add gutter icons on component markers, and a structure view of components and
- [ ] [#ghmultitmux] [agent-doc] One editor managing panes across several tmux sessions (GH #17, https://github.com/btakita/agent-doc/issues/17), e.g. a session per topic or project: an allowed-sessions list in config, with routing and sync choosing among them. Close #17 when shipped.
- [ ] [#preflightdeadline] [agent-doc] Make a preflight admission overrun impossible by construction (GH #78 follow-up). Today every wait inside preflight has its own local bound (3s settle waits, per-call controller timeouts), but nothing caps their SUM against the hook's 90s budget. The #78 session-check profile ran resolve_current_document_content 13x on a 518-byte document. Fix: (1) carry an admission deadline (the budget minus a margin) on the per-run PreflightProgress handle, and clamp every bounded wait in preflight to the time remaining; (2) check the deadline at each progress::enter phase boundary, so preflight returns a typed, named, retryable refusal at a step boundary instead of being abandoned mid-step; (3) once the first live overrun names its phase (now logged as preflight_admission_overrun), move that work off the admission path, e.g. to the supervisor's idle boundary. Code: agent-doc-preflight-command-io/src/progress.rs, src/preflight_hook.rs, agent-doc-preflight-io/src/debounce.rs.
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
- [ ] [#ghrecvpolicy] [agent-doc] Recipient-side [receive] auto_create_components policy for multi-user workspaces (GH #40, https://github.com/btakita/agent-doc/issues/40). Parked until multi-user workspace support exists.
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc/agent-doc-bugs.done.md -->

<!-- completed work archived in tasks/agent-doc/agent-doc-bugs.done.md -->
<!-- /agent:done -->
