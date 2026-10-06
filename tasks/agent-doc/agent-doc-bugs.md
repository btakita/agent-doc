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

### Re: Response — v0.35.466 hot-update and recovery release

> **Queue prompt:**
>
> do [#qsubattr5]
>
> do [#drivecapturedsplice]


Released agent-doc v0.35.466 from commit `0c66a41cc`. The GitHub release, all four platform archives, both editor packages, checksum manifests, Homebrew bump, and PyPI publish completed successfully. GitHub issues #151 through #157 are fixed and closed.

### Hot updates and retained recovery

- Every install/update now schedules existing supervisors to re-exec at the next safe idle boundary and controllers to recycle after supervisor handoff. The full install exercised this path: 18 supervisors and the eligible controllers were marked automatically; a mid-cycle editor endpoint was deferred safely instead of being rejected.
- Orphaned retained intents are retired after recycle. The earlier `roseshield.md` projection converged through document authority and committed as cycle `cycle-1791299889951`; nothing was resent, recaptured, or forced to disk.
- IDEA was not deadlocked or out of memory. Its event thread was blocked by a hidden **File Cache Conflict** modal. Bringing that dialog forward and choosing **Keep Memory Changes** released the UI while preserving editor authority. The JetBrains plugin was then dynamically upgraded and verified live, so no restart was required. Restarting IDEA is safe after this response commits, though it is unnecessary.
- The `sdk.md` harness could not resume the missing saved session ID, so its healthy supervisor started a fresh Codex turn. Its prior response was already committed; the session is now healthy and idle on actor generation 56.

### Resolved GitHub queue heads

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/151

Preset expansion satisfied: fix then closerelease.

Implemented typed usage errors and valid choices for invalid read components in integrated commit `7015e2850`; the dogfood defect banner is suppressed for this user-error path. Regression and full release tests passed, v0.35.466 was published, and issue #151 was closed.

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/152

Preset expansion satisfied: fix then closerelease.

Implemented glibc-over-stray-musl detection plus authoritative JetBrains/PATH install provenance in integrated commit `e1a1a2303`. Bootstrap and full release tests passed, v0.35.466 was published, and issue #152 was closed.

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/153

Preset expansion satisfied: fix then closerelease.

Implemented live-supervisor-pane layout derivation for fresh controllers in integrated commit `c03f337be`, preventing restart-time one-column collapse. Focused and full release tests passed, v0.35.466 was published, and issue #153 was closed.

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/154

Preset expansion satisfied: fix then closerelease.

Implemented retained-column protection for unknown manual layouts, Remote Dev client-session precedence, and fresh outside-window escalation generations in integrated commit `4c3fa0922`. JetBrains/controller and full release tests passed, v0.35.466 was published, and issue #154 was closed.

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/155

Preset expansion satisfied: fix then closerelease.

Implemented controller-local asynchronous Run Agent Doc execution in integrated commit `0d92d4fab`, removing the self-RPC five-second deadline failure. Focused and full release tests passed, v0.35.466 was published, and issue #155 was closed.

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/156

Preset expansion satisfied: fix then closerelease.

Implemented distinct retained-compact guidance in integrated commit `b780a7947`: delivery is underway, commits automatically, and must not be rerun; foreign pending retains its separate rerun guidance. Typed-outcome and full release tests passed, v0.35.466 was published, and issue #156 was closed.

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/157

Preset expansion satisfied: fix then closerelease.

Implemented immutable Remote Dev layout snapshot capture on the EDT and native folding off the EDT in integrated commit `b818399ed`, reconciled with the #154 authority rules. Merged JetBrains and full release tests passed, v0.35.466 was published, and issue #157 was closed.

> **Queue prompt:** release + publish

The v0.35.466 GitHub Release workflow completed successfully with all platform and editor assets plus checksums. The dependent PyPI build, publish, and verify jobs also completed successfully.

### Verification and publication

- `make check` passed: 9,591 Rust tests, 193 VS Code tests, JetBrains tests, cross-editor simulation, PyPI bootstrap tests, documentation/release audits, and all formal-model plus non-vacuity checks.
- The required tmux integration suite passed.
- `make install-full` installed agent-doc 0.35.466, native library 0.35.466, JetBrains plugin 0.2.501, and VS Code extension 0.2.80.
- Release: https://github.com/btakita/agent-doc/releases/tag/v0.35.466

### Gated follow-ups

- `#qsubattr5`: setup is complete on branch `qsubattr5-dogfood` at `de5a8aa7c0d5e879547d4e321d159b1eff840073`. It remains gated only for the next real operator-created multi-head batch, where distinct claims and `ops.log` must prove exactly-once dispatch and no inline drain.
- `#drivecapturedsplice`: the safer recipe and regression coverage shipped, but the corrected live run stopped after reaching owner pane `%134` and before the retained marker/edit-two/verifier sequence. It remains gated until `verify-captured-splice-recovery` passes.

> **Chat prompt (#chatprompt):** Idea seems to be stuck in with [Image #1]

> **Chat prompt (#chatprompt):** Idea is unresponsive

> **Chat prompt (#chatprompt):** sdk.md session crashed

### Re: recovered response

Completed and committed the [agent-doc session](/home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs.md).

- Released [agent-doc v0.35.466](https://github.com/btakita/agent-doc/releases/tag/v0.35.466); GitHub assets and PyPI publication succeeded.
- Fixed and closed GitHub issues #151–#157.
- Hot updates now automatically recycle existing supervisors and controllers at safe boundaries.
- `roseshield.md` retained intent converged and committed without resending or forcing disk.
- `sdk.md` recovered with no lost work.
- IDEA’s apparent hang was a hidden File Cache Conflict dialog. The plugin is upgraded and live.
- Full checks passed, including 9,591 Rust tests, editor suites, tmux integration, and formal models.
- Queue drained; `#qsubattr5` and `#drivecapturedsplice` remain correctly gated for live proof.
- Document commits: `539d7f0604`, queue closeout `4002c605ab`.

Yes—you can safely restart IDEA now, although it is no longer required.

### Re: GH #158 resolved

The selected #gh-fix expansion is complete: fix then closerelease.

The JetBrains native-library reload repair was verified, released in agent-doc v0.35.467, and https://github.com/btakita/agent-doc/issues/158 is closed.

- Reload handoff preserves every deregistered document path.
- Backend, Remote Dev, and active-editor discovery are unified.
- Incomplete replica rebuilds retry and report per-document outcomes.
- The release passed the full agent-doc and cross-editor checks and was installed successfully.

No actionable follow-up items remain for #158.
> **Chat prompt (#chatprompt):** Also fix in subagent: Compact Exchange in fpe.md: Agent Doc couldn't compact this document (exit 1). Error: [compact] pending: compact continuation compact-projection-9feecb5622f1e4c5968ca4b87853d78bd70fdb95999a27d26bdfa227e2d8ef98-95e5d1f4-c0e2-44c1-91b2-ad5278b56afb is in editor delivery and will commit automatically after delivery settles; do not re-run Compact Exchange (commit_requested=true)

> **Chat prompt (#chatprompt):** Also fix in subagent: inotify errors with Idea. Should we use a separate process for the bindings?

> **Chat prompt (#chatprompt):** In a subagent, fix: • Blocked by hook
>   └ agent-doc Stop hook intercepted an unfinished document cycle for /home/brian/work/btakita/agent-loop/src/haiven-
> dev/tasks/sdk.md. [session-check] INTERRUPTED: cycle `cycle-1791317771796` is still `preflight_started`
> (preflight_started) — cycle started but no write/commit followed. The latest assistant text was captured into the
> pending/capture ledger before auto-close. The hook could not finish the required commit boundary: closeout operation is
> already in progress for cycle cycle-1791317771796 by foreground-finalize-3518627-1791317845283876891-t1 pid=3518627
> role=foreground_finalize until=1791318145 [closeout_owner_retry_at=1791318145]. The retained capture or projection is
> already durable and the same intent commits itself once delivery converges — this is a deferral, not a lost response.
> Run `agent-doc session-check /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk.md` once to observe the
> terminal state; do NOT re-send the response, force disk, or `admin recycle`, which disturb the capture being awaited.
> Do not reach for `admin reload-lib` on your own either: `admin reload-lib` is the sanctioned recovery only when `agent-
> doc session-check` reports that the editor holding the document is not serving its replica, and then its remedy
> governs. Do not send the final answer yet. If the response is missing from the document, run `agent-doc repair /home/
> brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk.md` first. Then finish the commit boundary for this turn and end
> with `agent-doc session-check /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk.md`.

> **Chat prompt (#chatprompt):** also • The agent-doc closeout was still converging when the chat answer returned. I’m checking the retained write once, as
>   directed, without resubmitting or forcing the document.
>
> • Failed (exit 1) agent-doc session-check tasks/sdk.md
>   └ [perf] session_check.operations file=tasks/sdk.md total_ms=3666 attributed_ms=3504 nested=true inspect_with_warnin…
>     [session-check] INTERRUPTED: cycle `cycle-1791317771796` is still `response_captured` (response_captured) — respon…
>     + 2 lines (ctrl+t to expand)
>
> • Failed (exit 1) agent-doc repair tasks/sdk.md
>   └ [perf] repair.cycle_projection file=/home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk.md elapsed_ms=250
>     [repair] Found orphaned response for tasks/sdk.md (825 bytes). Applying...
>     Error: closeout operation is already in progress for cycle cycle-1791317771796 by foreground-finalize-3518627-1791…
>     + 4 lines (ctrl+t to expand)

> **Chat prompt (#chatprompt):** This may be related • Hook stopped
>   └ agent-doc Stop hook exceeded its 45s internal budget and failed closed before the harness timeout. The route-owned
> supervisor retains any captured closeout and continues recovery; do not rerun finalize or recapture the response.
> Phases: load_bound_session=23058ms session_check_inspect=3445ms auto_queue_continuation=1067ms
> active_session_prompt_writeback=0ms active_session_prompt_or_queue_head=RUNNING_17427ms

### Re: compact, IDEA inotify, and retained closeout recovery — gpt-6.1-sol · 2026-10-06T17:14:41-04:00

All reported defects are fixed locally, and the earlier SDK response is committed without replay or forced recovery.

- Compact Exchange initiating retained editor delivery now returns success while clearly reporting that the retained continuation will commit automatically; repeated or foreign pending operations remain nonzero. This shipped in agent-doc v0.35.468.
- A separate bindings process would not solve IDEA's inotify failure because the watch quota is per user. Since v0.35.468, `MaxFilesWatch` switches the daemon and steering follow paths to 500 ms content-aware polling. Native events still require reducing watched roots or raising the host per-user inotify limit.
- For `sdk.md`, the foreground owner exited and the one directed terminal `session-check` returned `commit_success`. The response was present, so no repair, resend, force-disk, recycle, or reload-lib action was taken.
- The stranded SDK lease was caused by a promoted v0.35.469 controller rejecting the older foreground finalizer's exact `closeout_owner_release` as `controller_binary_stale`. The fix permits only an exact capability-scoped owner release across version skew; every other stale mutation remains refused.
- The 45-second Stop trace exposed two additional serial budget sinks. Stop binding lookup is now bounded to five seconds, and exchange/queue selection reuse one authoritative document materialization instead of two realtime reads.
- The fixes are integrated and pushed through `cbeeac4bf` on `integrate/gh160-161-20261006`, installed locally as agent-doc v0.35.469, and verified by `make check-fast` plus a fresh authoritative `make check`: 9,598 Rust tests, 193 VS Code tests, JetBrains/native cross-editor coverage, release audits, and all formal/non-vacuity models passed.
- GH #160 and #161 are integrated in the same candidate. The queue-preemption persistence regression found during dogfooding is also fixed.

> **Queue prompt:** release + publish

**Deferred:** public v0.35.469 publication is blocked because crates.io rejects both the GitHub Actions and local credentials for the required `tmux-router` 0.3.22 publish. Refresh the `CARGO_REGISTRY_TOKEN` secret for `btakita/tmux-router` (and the local token only if local publishing is desired); then this queue head can publish tmux-router first and complete the agent-doc release. No issue or release is being claimed complete before that succeeds.
<!-- agent:boundary:4146e13a -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:queue preset="#subagents #spec-test-build-install-commit-push" priority go -->
<!-- /agent:queue -->


<!-- agent:backlog priority -->
- [ ] [#chatpromptmulti] [agent-doc] Preserve multi-paragraph #chatprompt recordings as one prompt when quoted blank lines separate paragraphs; add regression coverage so committed chat prompt bodies are not reclassified as fresh exchange steering.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#release469] [release] Publish tmux-router 0.3.22, then release agent-doc v0.35.469 from integrate/gh160-161-20261006. Blocked until the btakita/tmux-router CARGO_REGISTRY_TOKEN is refreshed; current GitHub Actions and local credentials are rejected by crates.io.
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
