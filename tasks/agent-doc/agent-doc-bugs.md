---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: c47dae65-e36f-40cb-8ebb-b7c9573e3c41
  codex: 01a1116f-44d5-79d2-b6f2-0ae06ed0dccd
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
> **Chat prompt (#chatprompt):** cont

### Re: continuation — gpt-5 · 2026-10-06T00:32:14-04:00

> **Queue prompt:**
>
> Watch the agent-doc gh issues. Some issues may be coming in over the next 2-3 hours. After you get the runbooks over the next 3 hours, release + publish. This will be unattended, so please do not stall.
>
> I'd like to associate `presets` with runbooks. Can we have a good way for agent-doc to author runbooks? An agent can easily author runbooks, but it would be nice to be able to navigate available runbooks in the harness.


Continued and verified the interrupted console handoff. The preceding `#releasefastpath` response is committed, its backlog item is complete, the worker claim is released, and the queue has no drainable work. No response recapture or duplicate repository mutation was needed.

### Re: runbook presets and the 0.35.465 release — gpt-5 · 2026-10-06T06:34:05-04:00

> **Queue prompt:** Watch the agent-doc gh issues. Some issues may be coming in over the next 2-3 hours. After you get the runbooks over the next 3 hours, release + publish. This will be unattended, so please do not stall.

Completed the full unattended watch and release cycle. The three-hour issue watch found GH #150; the fix now treats unknown, unowned bare `#id` queue heads as free text while preserving explicit and owned directives. I also fixed PyPI propagation verification so it retries a clean exact-version install from the simple index instead of trusting the earlier JSON signal.

Released `v0.35.465` from main commit `15dfe8366`. The [GitHub release](https://github.com/btakita/agent-doc/releases/tag/v0.35.465) is published with all four required Windows/Linux archives, `SHA256SUMS`, and both editor packages. The Release workflow and the PyPI build, publish, and clean-install verification jobs all completed successfully. The one-time local full install also completed and installed `agent-doc 0.35.465` plus its matching native library.

Verification included the authoritative workspace suite (9,582 tests passed, 141 skipped), 193 VS Code tests, the cross-editor native harness, formal model checks, the release parity audit, `tmux-ci`, private-name hygiene, and PyPI quota headroom. The separate main-branch CI run was still in progress at closeout; the release-specific workflows were green.

> **Queue prompt:** I'd like to associate `presets` with runbooks. Can we have a good way for agent-doc to author runbooks? An agent can easily author runbooks, but it would be nice to be able to navigate available runbooks in the harness.

Implemented structured prompt presets with optional runbook metadata while preserving scalar preset compatibility. A preset can now declare both its prompt and a project-relative runbook:

```yaml
presets:
  "#release":
    prompt: release + publish
    runbook: runbooks/release.md
```

Preflight exposes a required runbook-load instruction for the selected preset, so the harness receives an explicit path to read rather than relying on convention. Paths are restricted to safe project-relative files; traversal, symlink escape, absolute paths, and create collisions fail closed.

Runbook authoring and navigation are available directly in the harness through:

- `agent-doc runbook list`
- `agent-doc runbook show <name-or-path>`
- `agent-doc runbook create <name-or-path>`

The generated Claude Code, Codex, OpenCode, Cursor, and Grok skill surfaces, documentation, specifications, and regression tests were updated together. `[operator-verify]` A live visual pass through any preferred editor remains optional and non-blocking; no plugin-specific behavior changed in this release.
> **Chat prompt (#chatprompt):** also fix in subagent: • Blocked by hook
>   └ agent-doc Stop hook found a retained closeout that requires operator action for /home/brian/work/btakita/agent-
> loop/src/haiven-dev/tasks/roseshield.md. [session-check] INTERRUPTED: binary-owned document projection
> `1791293469309545516-1-e35c1c8d956053e614dcb51dc1aedf77f7788176ea50ed9c42274a2f7dba5fc7` is retained for `/home/brian/
> work/btakita/agent-loop/src/haiven-dev/tasks/roseshield.md` (reason=editor_projection_pending,
> source=post_commit_reposition, target_hash=e35c1c8d956053e614dcb51dc1aedf77f7788176ea50ed9c42274a2f7dba5fc7);
> retained_pending_needs_operator=true. The exact intent remains durable and must not be recaptured or forced to disk.
> The registered editor endpoint REJECTED the delivery receipt (`IPC receipt rejected`) — it answered NO, so the delivery
> projection this write is waiting on will never converge and waiting will not commit it. The response is durable and NOT
> lost: do NOT re-send it or force disk. Recover by removing the rejecting endpoint: restart or reload the editor that
> has /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/roseshield.md open (a backend still running deleted plugin
> jars after an update is the observed cause). Once that endpoint is gone or re-registered, the retained intent converges
> through document authority on its own; then run `agent-doc session-check /home/brian/work/btakita/agent-loop/src/
> haiven-dev/tasks/roseshield.md` and follow the recovery it names Report this exact blocker and its recovery to the
> operator; do not resend or recapture the response, force disk, or keep retrying session-check. After reporting it, the
> recursive Stop may finish this turn while the durable capture remains retained.
>
> **Chat prompt (#chatprompt):** hot-updates should not cause the error. every update should automatically recycle existing supervisors + controllers
>
> **Chat prompt (#chatprompt):** Can I restart Idea now?

### Re: hot-update recovery and automatic runtime recycling — gpt-5 · 2026-10-06T10:30-04:00

Yes—the IDEA restart was the correct recovery. It removed the rejecting JetBrains process, which had deleted plugin JARs mapped, and registered a clean plugin 0.2.500 endpoint. The retained RoseShield projection then converged without resending the response or forcing disk. The remaining RoseShield interruption is a separate normal turn containing two new operator prompts, so it was left for that document's owning session.

The recurrence is fixed and pushed to `agent-doc` main at `36451380f`. Every successful binary, native-library, and editor-plugin update now schedules an ordered safe handoff across all existing runtimes: supervisors first, then project controllers, without the former idle-root or same-binary exclusions. Multi-target plugin updates also recycle after partial success when any target changed. Fresh controllers can retire the specific post-restart orphan seen here only under the narrow guarded condition: local `PayloadAbsentFromConvergedContent`, authoritative controller absence, equal non-null authority/disk hashes, and an exact intent-id race check. Divergent or in-flight states remain blocked.

Verification passed: focused regressions, package-wide Clippy, `make check-fast`, and one authoritative `make check`. The latest observed main CI run was green. The one local `make install` succeeded and exercised the new behavior live: 17 supervisors and 15 controllers were marked in order with zero skipped at the binary/library handoff, followed by safe package-update recycle waves for JetBrains and VS Code. JetBrains dynamically upgraded without requiring another IDE restart; two unrelated open documents were still completing their bounded replica re-registration retries.

> **Queue prompt:** #advance-reviews

The five review gates were audited in a subagent. Two were stale gates and are now actionable backlog work: `#qsubattr5` still needs the real `subagents=3` dogfood proof, and `#drivecapturedsplice` still needs the stronger stale-replica race proof rather than weakening the verifier to accept a healthy path. Three remain genuinely gated: `#steergatemodelrerun` has 66 of the required 200 labelled rows, PyPI still has ample headroom while its support request awaits a human response, and Grok remains signed out. No review item was incorrectly marked complete. The semantic completion match for `#steergatemodelrerun` is a known false positive: it is the intentionally renamed successor to the accidentally reaped prior id.
<!-- agent:boundary:38ce3b77 -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:queue preset="#subagents #spec-test-build-install-commit-push" priority go -->
<!-- /agent:queue -->


<!-- agent:backlog priority -->
- [ ] [#drivecapturedsplice] [operator-verify] Captured-splice stale-replica live proof (plan: tasks/agent-doc/plan-captured-editor-splice-recovery.md): keep the existing branch-specific verifier contract; update the xdotool captured-splice recipe to hold canonical delivery after the independent advance until edit two is typed, avoid the gitignored-scratch commit_refused path for the advance, rerun the live proof, and require verify-captured-splice-recovery to pass.
- [ ] [#qsubattr5] [agent-doc] Queue subagents attribute, phase 5 (plan: tasks/agent-doc/plan-queue-subagents-attribute.md): add subagents=3 to tasks/software/tsift.md before its next real multi-head batch; then verify from queue claims and ops.log that every eligible head is dispatched exactly once and none drain inline.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
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
