---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: c47dae65-e36f-40cb-8ebb-b7c9573e3c41
  codex: 01a11229-37c1-78e2-9044-2a7ef441955c
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

Released agent-doc v0.35.460; fixed and closed GH #139–#143; restored this document pane; cleared backlog cruft; captured two release-cycle follow-ups.
<!-- /agent:status -->
 

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20261007-201000.md`*

Compacted content:
- Archived 7 response topic(s): release and delegated fixes; GH #172, GH #173, SDK Stop-hook recovery, and release; recovered response; 4 more
- Prior summary/context: prior compacted content: Archived 14 response topic(s): GH #148, GH #149, releases, and release speed; release fast p...
> **Chat prompt (#chatprompt):** Is the release done yet?
>
> **Chat prompt (#chatprompt):** what about 0.35.476?
>
> **Chat prompt (#chatprompt):** cont

### Re: actionable review and release status — gpt-5 · 2026-10-07T16:20-04:00

`v0.35.475` is released and installed. `v0.35.476` is not released yet: there is no remote tag or GitHub release, upstream still declares `0.35.475`, and the installed CLI is `0.35.475`. Public `main` is ahead at `8d8b7480a`; its latest CI run was still in progress during this review.

The delegated actionable review added three non-duplicate recommended items to the backlog and mirrored queue, in priority order:

1. `#releasepost475fixes` — publish the post-`v0.35.475` fixes as the next release and verify GitHub/PyPI artifacts.
2. `#annotatehistorynoop` — implement `annotate --history` as advertised, or reject/remove the currently ignored flag, with regression coverage.
3. `#semanticcontinuationfalsepositive` — prevent semantic-completion warnings from treating a newer re-gated continuation as resolved by an obsolete predecessor.

The preflight warning about `#steergatemodelrerun` was therefore not treated as proof of completion: that live gate has advanced evidence but remains below its required dataset threshold.
<!-- agent:boundary:2577cced -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built
<!-- agent:queue subagents preset="#spec-test-build-install-commit-push" priority go -->
- #actionable-review
<!-- /agent:queue -->


<!-- agent:backlog priority -->
- [ ] [#releasepost475fixes] [recommended] [P1][release] Release and publish the fixes landed after v0.35.475 through public main 8d8b7480a, including GH #174–#177, Run Agent Doc delayed-submit verification, operator backlog deletion, layout/reopen recovery, bare-subagents lint/replay, and late selected-preset closeout. Run the authoritative full check/install/release path and verify GitHub assets and PyPI.
- [ ] [#annotatehistorynoop] [recommended] [P2][correctness] Implement git-history attribution for `agent-doc annotate --history`, or reject/remove the flag until implemented; add CLI and sidecar regression tests proving history mode differs from snapshot-only attribution and invalidates its cache when relevant Git history changes.
- [ ] [#semanticcontinuationfalsepositive] [recommended] [P2][diagnostics] Stop semantic-completion warnings from treating a newer re-gated continuation under a different item ID as already resolved by its obsolete predecessor; add the #steergatemodelrerun/#steergatenamodata pair as a regression while preserving warnings for true duplicate work.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
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
