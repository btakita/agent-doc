---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: c47dae65-e36f-40cb-8ebb-b7c9573e3c41
  codex: 01a10c4e-9d09-7702-964c-9375947d5ea2
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
  '#gh-fix': fix then closerelease
  '#release': release + publish
  '#self': agent-doc-bugs.md
  '#subagents': 'run the remaining items in subagents'
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: go
---

## Status

<!-- agent:status patch=replace -->

Released agent-doc v0.35.460; fixed and closed GH #139–#143; restored this document pane; cleared backlog cruft; captured two release-cycle follow-ups.
<!-- /agent:status -->
 

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20261006-014438.md`*

Compacted content:
- Archived 42 response topic(s): do #steerbaselineabsorb; preflight refused on its own repair (haiven-dev sdk.md); queue: GH #127, #128, #129 + release; 39 more
- Prior summary/context: prior compacted content: Archived 15 response topic(s): release + publish; release cleanup; queue dispatch; 12 more
> **Chat prompt (#chatprompt):** Can we reduce the time to release?

### Re: GH #148, GH #149, releases, and release speed — gpt-6.1-sol · 2026-10-05T23:22:27-04:00

Yes. The largest avoidable delay in this cycle was running two complete release trains: GH #149 arrived while v0.35.463 was already underway, so it required a second version projection, full check, tmux CI, release, and install. The recommended fast path is to coalesce all ready claimed fixes before the release boundary, then distinguish “tag pushed and publication jobs handed off” from “local install-full finished.” I added that work as the recommended `releasefastpath` backlog item, including timing and regression coverage.

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/148

Preset expansion satisfied: fix then closerelease.

GH #148 is fixed, closed, and released in v0.35.463. Retained compact projections now have a bounded 30-second deadline with identity-fenced cleanup, a nonzero pending CLI outcome, rerun guidance, and continuation-aware completion logs. The final tree passed the complete check suite and tmux CI; a timing-race regression was found during verification and fixed before release.

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/149

Preset expansion satisfied: fix then closerelease.

GH #149 is fixed, closed, and released in v0.35.464. Restarted editor replicas now handle missing registration shadows safely, diagnose the exact retained-reseed hold with an actionable remedy, clean up provisional registrations deterministically, and expose replica health through `admin inspect`. The JetBrains plugin generation advanced to 0.2.499. The final tree again passed the complete check suite and tmux CI.

> **Queue prompt:** release + publish

The v0.35.464 tag and main branch are pushed, the clean release checkout matches `origin/main`, and the installed CLI reports `agent-doc 0.35.464`. GitHub Release and PyPI workflows were both in progress at the required single observation; the GitHub release artifact was not yet published at that point. Per the repository release policy, I did not wait on or poll those asynchronous workflows.

The preflight semantic-completion matches were advisory only and were not used as evidence to close this batch.

### Re: release fast path — gpt-5 · 2026-10-06T00:23:20-04:00

Implemented and pushed `#releasefastpath` to `agent-doc` main as `bbb2489da` (`perf: batch and time release phases`).

The release workflow now seals all completed claimed fixes that are ready before version projection into one version/check/install batch. `make release` delegates its irreversible tail to a new release driver that separately times and reports `tag-publish-handoff` and `local-install-full`, runs `install-full` exactly once after the push, skips installation when the push fails, and preserves the successful publication outcome when only the later local install fails. The batch boundary and outcome semantics are documented across AGENTS, the functional/core-command specs, the shared skill, and every harness runbook mirror.

Regression coverage includes behavioral self-tests for success, push failure, and post-publish install failure, plus Rust structural coverage that pins the Makefile wiring and independent timing/status output.

Verification passed on the exact integrated tree: the release-driver self-test, targeted Rust regression, `make check-fast`, `make build`, one authoritative `make check`, diff/whitespace validation, and `make install`. The checked worker tree and the integrated commit had the same Git tree hash, so only commit metadata changed during integration. The install updated the binary, native library, skills, and JetBrains plugin; VS Code package sync used its supported skip path because `vsce` is not installed. Main was fast-forwarded to `bbb2489da`. CI run `37413416775` is in progress; the immediately preceding main CI run `37407933239` completed successfully.
<!-- agent:boundary:93bdac10 -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:queue preset="#subagents #spec-test-build-install-commit-push" priority -->
- do [#releasefastpath]
<!-- /agent:queue -->


<!-- agent:backlog priority -->
- [ ] [#releasefastpath] [recommended] Shorten the release critical path: coalesce all ready claimed queue fixes into one version/check/install batch, and report tag/publish completion independently from local install-full completion; add timing and regression coverage.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#qsubattr5] [agent-doc] Queue subagents attribute, phase 5 (plan: tasks/agent-doc/plan-queue-subagents-attribute.md): dogfood in tasks/software/tsift.md (replace per-line #subagent: prefixes with subagents=3); verify via queue claims + ops.log that each head is dispatched exactly once and none drain inline.
- [/] [#steergatemodelrerun] [agent-doc] Rerun the steergatenamo model evaluation on real pause points. Gate: at least 200 labelled steering_gate_log rows with a text_tail (0 as of 2026-10-03). Then run src/agent-doc/scripts/steergate-model-eval --dataset from the project root, which exits insufficient_data until the floor is met. Integrate turnsense locally (ONNX via ort, lazily downloaded, opt-in) only if rules+perceptron+turnsense beats rules+perceptron on held-out log-loss; if close but short, fine-tune on the logged rows. Proxy results and decision: tasks/agent-doc/eval-steergatenamo.md
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
