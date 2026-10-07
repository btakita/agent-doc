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
  '#gh-fix': fix then closerelease
  '#release': release + publish
  '#self': agent-doc-bugs.md
  '#subagents': 'run the remaining items in subagents'
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
---

## Status

<!-- agent:status patch=replace -->

Released agent-doc v0.35.460; fixed and closed GH #139–#143; restored this document pane; cleared backlog cruft; captured two release-cycle follow-ups.
<!-- /agent:status -->
 

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20261007-061259.md`*

Compacted content:
- Archived 14 response topic(s): GH #148, GH #149, releases, and release speed; release fast path; continuation; 11 more
- Prior summary/context: prior compacted content: Archived 42 response topic(s): do #steerbaselineabsorb; preflight refused on its own repair ...

### Re: release and delegated fixes — gpt-6.1-sol · 2026-10-07T05:06:55-04:00

> **Queue prompt:**
>
> Run Agent Doc on this file; then interrupt the Codex turn; agent-doc should detect if the turn is interrupted. I ran Compact Exchange and got an error. Compact Exchange should work even if a turn is running/interrupted. Run Agent Doc again caused the enqueued Compact Exchange to run.
>
> #gh-fix https://github.com/btakita/agent-doc/issues/166
>
> #gh-fix https://github.com/btakita/agent-doc/issues/169
>
> #gh-fix https://github.com/btakita/agent-doc/issues/170
>
> #gh-fix https://github.com/btakita/agent-doc/issues/171


> **Chat prompt (#chatprompt):** In a subagent: Fix agent-doc background processes thrashing

> **Chat prompt (#chatprompt):** also fix in subagent: api.md session crashed

> **Chat prompt (#chatprompt):** fix in subagent: ~/work/btakita/agent-loop/src/haiven-dev docs/fpe-ser…emporal-plan ⇡802 +1 !7                                 04:02:15
>
> ❯ /home/brian/.cargo/bin/agent-doc start --route-owned --route-owned-reap-policy auto --resume=01a0eaeb-c65f-7241-adde-93f0934ccc02 -- /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/api.md
> Error: Unterminated frontmatter block

> **Queue prompt:** release + publish

Completed the delegated bug batch and published it in two patch releases:

- `v0.35.472` contains the Compact/interruption race fix, background-process thrash fix, and GH #166, #169, #170, and #171. The issues are closed and the release was installed locally.
- `v0.35.473` contains the `api.md` crash fix. Source and tag are pushed at `2b6c74ebb`; the generated JetBrains `0.2.503` install revision is recorded on `main` at `5d3187301`.

The fixes are:

- **Run/interruption + Compact Exchange:** a document advance during drain now refreshes the durable op-capture epoch, so Compact Exchange defers through the active-typing window instead of treating partial input as quiescent. The interrupted/rerun flow no longer strands the queued compact operation.
- **Background-process thrashing:** recycle attempts are monotonic per session, causal recycle markers are validated, and capture-resume occurs only for proven binary staleness. Repaired supervisors/controllers settle instead of repeatedly recycling.
- **GH #166:** fresh-controller focus escalation now requires positive live tmux layout evidence.
- **GH #169:** warm supervisors refresh the incremental liveness journal, distinguish no-route from dead-route state, and use bounded recovery.
- **GH #170:** optional `data: null` controller envelopes preserve the typed outer-success/inner-null result.
- **GH #171:** concurrent `Preparing` successors are protected by exact identity, and failed handoffs retain observable exit/signal evidence.
- **`api.md` crash:** route-owned hot re-exec was parsing malformed editor-authoritative text containing a literal `9---` closing fence even though disk was already valid. Startup now validates the live authority before exec, retries transient invalid authority at a bounded cadence, propagates cold-start failure synchronously, and repairs only a proven one-character-prefixed closing fence after both the YAML and repaired full document validate. The live `api.md` session converged to its already-valid disk content; the document itself was not edited by the worker.

Validation and installation completed:

- Full `make check` passed, including 9,615 Rust tests, editor suites, and formal-model checks.
- `make tmux-ci` passed.
- Post-install editor parity passed: 193 VS Code tests plus the native cross-editor simulation.
- `make install-full` installed CLI/library `0.35.473`, JetBrains `0.2.503`, and VS Code `0.2.80`. Runtime processes were marked for safe idle-boundary handoff; none were forcibly recycled.

The one-time remote observation found CI and the `v0.35.473` Release workflow still in progress; the GitHub release object was not yet available. Per release policy, I did not poll or wait for those asynchronous jobs.
<!-- agent:boundary:e0f2b885 -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:queue preset="#subagents #spec-test-build-install-commit-push" priority -->
<!-- /agent:queue -->


<!-- agent:backlog priority -->
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
