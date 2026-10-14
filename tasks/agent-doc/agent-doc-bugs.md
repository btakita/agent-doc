---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: ba846c4c-2e01-4e12-9539-096faec207ee
  codex: 01a13b46-0ff5-7cb3-9cad-48c782f68b6a
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
---

## Status

<!-- agent:status patch=replace -->
Review pass done. frontendlivedispatch closed on live evidence. Two review items remain, both genuinely gated: an operator decision (jb262dynlauncherrecommen) and an external PyPI reply (pypiquotarecheck).
<!-- /agent:status -->
 

## Exchange

<!-- agent:exchange patch=append subagents -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20261014-044955.md`*

Compacted content:
- Archived 30 response topic(s): #actionable-review; GitHub issue #245; Restart Agent retained projection and infra.md recovery; 27 more
- Prior summary/context: prior compacted content: Archived 59 response topic(s): dispatched GH #240 to a subagent; Clear Session Context on sd...
❯ What this mean? Is this a bug?
```
• Failed (exit 1) agent-doc respond /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk.md --template --done …
  └ This exchange is already above the legibility target, but the active queue takes precedence; no repository or prod…
    [write] template write requested for realtime document; routing through stream document-model write path
    Error: strict template closeout response must wrap its response in matching `<!-- patch:exchange -->` and `<!-- /p…
    + 33 lines (^t to expand)
```

### Re: strict-template failure and steering stall — gpt-5 · 2026-10-14T01:05-04:00

The shown `respond --template` failure is not an Agent Doc core bug. That call supplied plain Markdown even though preflight had explicitly required matching `<!-- patch:exchange -->` and `<!-- /patch:exchange -->` markers and disallowed plain stdin. Strict template mode rejected it before capture or mutation, as designed. The retry added both markers and succeeded with the same response, so no answer or `--done` mutation was lost. The realtime-routing line was informational, and `--done` was unrelated.

The later stall is a separate issue. The committed response was followed by new realtime steering, but the continuation stopped with an abandoned prior cycle instead of admitting the new prompt automatically. That behavior should continue without manual re-entry. The queue item describing it is now claimed by `subagent:steering_stall` for a spec, regression-test, and implementation fix; two isolated investigators are also checking why the pasted queue diagnostic was classified and split into extra exchange prompts. The existing GitHub issue workers remain active.
> **Chat prompt (#chatprompt):** subagent: fix: • Blocked by hook
>   └ agent-doc Stop hook retained the exact closeout for /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk.md
> under its durable keyed retry. The latest assistant text was captured into the pending/capture ledger before auto-
> close. The hook could not finish the required commit boundary: closeout operation is already in progress for cycle
> cycle-1791959659836 by captured-finalize-resume-3600622-1791956809940749262-t13 pid=3600622
> role=captured_finalize_resume until=1791962084 [closeout_owner_retry_at=1791962084]. The retained capture or projection
> is already durable and this exact retained intent commits itself once delivery converges — this is a deferral, not a
> lost response. Wait for its controller-owned terminal state edge; do not use the document's current-cycle status as
> proof because queue continuation can advance to a different cycle. Do NOT re-send the response, force disk, or `admin
> recycle`, which disturb the capture being awaited. Do not reach for `admin reload-lib` on your own either: `admin
> reload-lib` is the sanctioned recovery only when `agent-doc session-check` reports that the editor holding the document
> is not serving its replica, and then its remedy governs. The controller owns delivery, the editor-native save receipt,
> and the terminal commit. Do not reopen or recapture the cycle, rerun finalize/write/repair, force disk, recycle the
> supervisor, or resend the response. Wait for the existing controller state edge unless it explicitly reports
> `needs_operator`; do not send the final answer yet.

### Re: retained closeout and SDK queue continuation — gpt-5 · 2026-10-14T03:10-04:00

Dispatched the retained-closeout ownership failure to `subagent:retained_closeout_owner` in an isolated worktree. It will preserve the durable `sdk.md` capture, use only read-only evidence from that session, and fix the controller/Stop-hook behavior with specs and deterministic tests.

A second document-queue report showed a distinct in-pane queue-continuation failure for `do [#sdkreleasecontrolreset]`. I claimed that queue head and dispatched `subagent:sdk_queue_continuation` separately so it can fix continuation behavior without executing or modifying the live SDK session. Neither worker will resubmit, force-write, recycle, reload, or otherwise disturb `sdk.md`.

### Re: integrated queue fixes — gpt-5 · 2026-10-14T03:29-04:00

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/251

Fixed and integrated on `main` as `3f45b8e935`. The JetBrains stale-plugin diagnostic now selects the expected package generation from the live IDE platform line, including build 263, and reuses live mapped-JAR evidence. Issue #251 is closed.

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/252

Fixed and integrated on `main` as `b709dc7065`. Self-recycle handoff now preserves predecessor lifecycle authority through promotion/drain and prevents the predecessor listener from unlinking the promoted successor socket. Issue #252 is closed.

> **Queue prompt:** fix sdk.md issue
> ```
> › agent-doc /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk.md
>
> • Blocked by hook
>   └ Queue continuation, not an error: Codex labels it `Blocked by hook`. Next `agent:queue auto` prompt for /home/
> brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk.md: "do [#sdkreleasecontrolreset]". Continue THIS turn in-pane:
> answer it, then persist with `agent-doc finalize /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk.md`. Do
> NOT run `agent-doc /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk.md` (recursive self-invocation) and do
> not send the final answer yet.
> ```

The foreign-subagent inheritance defect was already fixed on `origin/main` by `3e21e1a24e`, which fences queue continuation by the tracked Codex turn ID. Read-only session evidence confirms `#sdkreleasecontrolreset` continued and committed successfully. The current integrated build, including that fix, is now installed; no live `sdk.md` response was resubmitted or modified.

The same verified batch also integrated realtime steering continuation after an abandoned empty Stop preflight as `5af7f40b1f`. Coordinator verification passed: 10,471 Rust tests, 202 VS Code tests, cross-editor SimWorld, JetBrains classic/262/263, bootstrap, docs/version audits, and TLA receipt. The batch was pushed to `main` and installed locally.
> **Chat prompt (#chatprompt):** subagent: fix: • Blocked by hook
>   └ agent-doc Stop hook retained the exact closeout for /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk.md
> under its durable keyed retry. The latest assistant text was captured into the pending/capture ledger before auto-
> close. The hook could not finish the required commit boundary: closeout operation is already in progress for cycle
> cycle-1791959659836 by captured-finalize-resume-3600622-1791956809940749262-t13 pid=3600622
> role=captured_finalize_resume until=1791962084 [closeout_owner_retry_at=1791962084]. The retained capture or projection
> is already durable and this exact retained intent commits itself once delivery converges — this is a deferral, not a
> lost response. Wait for its controller-owned terminal state edge; do not use the document's current-cycle status as
> proof because queue continuation can advance to a different cycle. Do NOT re-send the response, force disk, or `admin
> recycle`, which disturb the capture being awaited. Do not reach for `admin reload-lib` on your own either: `admin
> reload-lib` is the sanctioned recovery only when `agent-doc session-check` reports that the editor holding the document
> is not serving its replica, and then its remedy governs. The controller owns delivery, the editor-native save receipt,
> and the terminal commit. Do not reopen or recapture the cycle, rerun finalize/write/repair, force disk, recycle the
> supervisor, or resend the response. Wait for the existing controller state edge unless it explicitly reports
> `needs_operator`; do not send the final answer yet.

### Re: retained closeout fix remains in flight — gpt-5 · 2026-10-14T03:31-04:00

This is the same defect already assigned to `subagent:retained_closeout_owner`. I kept that single owner rather than dispatching duplicate repository work. The worker remains constrained to read-only evidence from the retained `sdk.md` operation and an isolated Agent Doc worktree; it will not resubmit, force-write, recycle, reload, or otherwise disturb the durable capture.

### Re: retained closeout owner wait fixed — gpt-5 · 2026-10-14T03:46-04:00

Implemented, integrated, installed, and pushed the retained-closeout fix to `main` as `01cedcd3ba` (worker source `01af085925`).

The root cause was premature Stop-adapter handling: a durable controller-owned `Deferred` repair receipt was immediately converted into `Blocked by hook`, even though the exact cycle was still progressing and reached write, commit, terminal proof, and owner release roughly twelve seconds later. Stop now subscribes to that exact cycle through the existing reactive closeout projection:

- terminal or superseded state invalidates the cached observation and re-inspects;
- owner release permits one exact keyed resume, then re-inspects;
- timeout, observation failure, or a missing key retains the bounded fail-closed response.

No polling loop, duplicate state owner, forced write, recycle, reload, or live `sdk.md` mutation was introduced. The historical SDK retained cycle had already completed normally and was inspected read-only.

Verification on the integrated commit passed: 10,472 Rust tests (167 skipped), 202 VS Code tests, native cross-editor SimWorld, JetBrains classic/262/263 builds, eight bootstrap tests, docs/version audits, and the matching TLA receipt. The release-local CLI/library and editor packages installed successfully. The preceding `main` CI run was still in progress when checked; local authoritative verification is complete.
> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/253

### Re: GH #253 one-column provisional layout regression — gpt-5 · 2026-10-14T04:09-04:00

Issue #253 is fixed on `main` and closed. The production ordering correction was already present in `a6690b86a1`; commit `55d1c4e9985e777b69ebd0807f49c68dca2c5372` adds the exact one-column regression sequence (`[a]` → provisional `[c]` → repeated structural `[a,c]`) and specifies that the pre-guess ancestry rule applies at width one.

Changed files:

- `agent-doc-editor-surface/src/remote_layout.rs` — focused GH #253 regression coverage.
- `editors/jetbrains/SPEC.md` — one-column provisional ancestry contract.

Verification passed: touched-file rustfmt, the focused regression, `make check-fast`, and the authoritative full `make check` (exit 0; zero detected failures). The single local `make install` completed successfully for the CLI/library, JetBrains packages, and VS Code extension. The worker branch was pushed, then fast-forwarded to `origin/main`; CI run `38533121984` for `55d1c4e9` is queued. The public package release remains for the next release cut; no release or version bump was performed in this cycle.

### Re: GitHub issue #243 — gpt-5 · 2026-10-14T13:34-04:00

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/243

Fixed, integrated, pushed, installed, and closed. Commit `10fb137c24` makes duplicate-supervisor retirement safe: a proven duplicate is retired immediately only when it is idle, input-free, and not client-visible; protected route duplicates receive a durable PID/pane/start-tick-targeted request that their existing idle watcher consumes only after revalidating authority and live protections. A protected `start --force` now fails closed for operator retry, and diagnostics name the duplicate pane, window, authoritative owner, reason, and eventual closure.

Verification passed on the integrated batch: the authoritative `make check` exited 0 with zero detected failures; the focused worker suites passed 5,240 nextest tests, 160 start-runtime tests, 26 start-IO tests, policy tests, clippy, and full `make tmux-ci`. The single coordinator `make install` completed for the CLI/library, JetBrains packages, and VS Code extension. The batch is on `origin/main`; CI run `38607719497` is in progress.

A separate `tools.md` Stop-hook regression is claimed by `subagent:tools_md_issue` in an isolated worktree and was deliberately kept outside this sealed batch.

### Re: GitHub issue #254 — gpt-5 · 2026-10-14T13:36-04:00

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/254

Fixed, integrated, pushed, installed, and closed. Integrated commit `73f94f2d32` repairs both causes of the four-hour editor convergence wedge: repeated refused native-save receipts now request an idempotent editor-replica re-registration every third refusal, independently of replica pull traffic, and a structurally valid complete live editor observation now outranks a partial stale op-epoch replay that could otherwise manufacture garbled operator text.

The realtime workflow specification and incident-shaped regressions cover both rules. Focused verification passed 688 controller tests, 185 realtime tests, controller clippy, the JetBrains local-splice suite, and `buildPlugin`; the worker's final `make check-fast` passed 3,808 nextest tests. After serial integration, the coordinator's authoritative `make check` exited 0 with zero detected failures, and the single batch `make install` completed for the CLI/library and editor packages. The fix is on `origin/main` as part of `10fb137c24`; issue #254 is closed.

### Re: project-wide prompt presets — gpt-5 · 2026-10-14T13:37-04:00

> **Queue prompt:** Is the project-wide agent-doc template (i.e. presets) released + published? How do I use it?

No—not in a public release yet. The latest published Agent Doc version is `0.35.498` on GitHub Releases, PyPI, and Homebrew; crates.io is historical at `0.34.65`. Project-wide preset inheritance landed on `main` in `d89a6db234`, 17 commits after tag `v0.35.498`, and no tag contains it. Therefore there is no first released version yet, and a stock `0.35.498` installation cannot use project-wide inheritance. The locally installed build from current `main` can.

Create `<project-root>/.agent-doc/project.md` manually with YAML frontmatter:

```markdown
---
presets:
  "#review": Review the change and run focused tests.
  "#release":
    prompt: Prepare and publish the release.
    runbook: runbooks/release.md
---

# Agent Doc project defaults
```

Then invoke `#review` in a session exchange or add `- #review` to an active `agent:queue`. Both `presets:` and `prompt_presets:` are accepted. The nearest `.agent-doc/project.md` supplies live defaults to sessions below it; edits apply without copying or syncing. An exact-name session-local declaration overrides the project value, while an unfinished or null local declaration masks it. Only presets inherit; other project frontmatter and body content do not.

User-facing setup and inheritance documentation is now on `main` in `48eeb06782` (`docs/guide/document-format.md` and `docs/reference/features.md`), included in integrated head `10fb137c24`. Verification passed three focused inheritance tests, `make check-fast`, and the authoritative full suite with 10,473 Rust tests plus the remaining checks.

### Re: multi-preset queue authorization — gpt-5 · 2026-10-14T14:43:02-04:00

> **Queue prompt:** do [#toolspresetmultiname]

Implemented and released in agent-doc `v0.35.499`.

- `preset="#push #auth"` now resolves each whitespace-separated preset independently, in source order.
- Whole-value and individually quoted references are supported; canonical duplicates collapse at first occurrence.
- Preflight, realtime steering, and subagent briefs share the behavior, preserve structured runbooks, and fail closed if any reference is unresolved.
- Integration commit: `8935621f29` (`fix: resolve multiple component presets`); release commit/tag: `32990a20ed` / `v0.35.499`.
- Focused regressions passed 17/17; the authoritative release gate passed 10,483/10,483 Rust tests, 202/202 VS Code tests, all JetBrains package lines, cross-editor simulation, and the live tmux suite.

The hosted Release workflow is still assembling platform artifacts; code, tag, and local install are complete. Exchange size is elevated, but the active auto-queue continues without pausing for compaction.

### Re: `admin issue` action (`#actionfileshuman`) — gpt-5 · 2026-10-14T14:57-04:00

The repository does not yet have the requested action contract, so I have claimed `#actionfileshuman` and dispatched a fresh isolated worker with the authorized `#spec-test-build-install-commit-push` scope. The implementation brief reserves a repository-specific `admin issue` action for filing human-only or agent-assisted issues in `btakita/agent-doc`, while keeping generic `issue` available for callers' own repositories unless the existing action architecture requires a more compatible spelling.

The worker owns specification, implementation, regression tests, build verification, commit, and branch push. I retain coordinator ownership of review, integration, the single local install/release batch, claim release, and final closeout. The item stays open until that evidence is integrated and verified.

The exchange is above the normal accretion warning threshold, but the active queue contract requires continued draining rather than pausing for compaction.

> **Chat prompt (#chatprompt):** Let me know when the release is published

### Re: hot-reload tmux auto-sync and release notification — gpt-5 · 2026-10-14T15:20-04:00

I claimed the new hot-reload/auto-sync report and dispatched a fresh isolated worker with the authorized specification, tests, build verification, commit, and push scope. It will reproduce the library/plugin reload gap, trace the tmux layout ownership transition, and add deterministic coverage; coordinator review, integration, installation, and claim release remain here. The earlier developer-prompt-choice item is likewise claimed and running in its own isolated worker.

`v0.35.500` is installed locally and its correct hosted Release run is active on `btakita/agent-doc`, but publication is not complete yet. I will notify the operator only after the Windows build is green and the GitHub release, PyPI package, and Homebrew formula are publicly visible. The release claim remains open until those checks pass.

The exchange remains above the normal accretion warning threshold; the active auto-clear queue requires yielding for its fresh-context boundary rather than pausing the queue for compaction.

### Re: surface subagent prompts and permission requests across harnesses — gpt-5 · 2026-10-14T15:22-04:00

Yes. Agent-doc can define a harness-neutral escalation envelope so a background subagent sends its blocking question or permission request—with the exact proposed action, why it is needed, affected scope, and safe alternatives—to the coordinator, which presents it to the operator in the owning session. Approval must remain operator-to-coordinator only: a worker may request authority, but it cannot grant itself authority, broaden its queue preset, or treat silence as consent.

The implementation must map that common contract onto each harness honestly: use native subagent messaging or permission surfaces where Claude Code, Codex, Grok Build, or OpenCode expose them, and use an agent-doc coordinator relay where they do not. It must preserve the request verbatim, identify the originating worker and queue item, deduplicate retries, keep the worker paused, and route the operator’s answer back to that worker without converting it into authorization for sibling work.

I have claimed this item and dispatched a dedicated isolated worker with the authorized specification, implementation, regression-test, build, commit, and push scope. It is explicitly covering all four named harnesses and distinguishing native capabilities from protocol fallbacks. The item remains open until the coordinator reviews, integrates, tests, installs, and releases the result.

### Re: published release and completed integration batch — gpt-5 · 2026-10-14T16:17-04:00

Agent-doc `v0.35.500` is published and independently verified on all intended public channels:

- GitHub release: https://github.com/btakita/agent-doc/releases/tag/v0.35.500
- PyPI: https://pypi.org/project/agent-doc/0.35.500/
- Homebrew: `btakita/homebrew-tap` now serves formula version `0.35.500`.

The hosted Release workflow completed successfully across every platform, including the corrected Windows build. The post-release integration batch is also complete on `btakita/agent-doc` main through `840625b9c0`: `make check` passed all 10,495 tests, `make tmux-ci` passed, `make install-full` installed the CLI/native library and editor packages, and the verified commits were pushed.

The `#actionfileshuman` backlog item is complete. Agent-doc now has the repository-locked `admin issue` action with explicit `human-only` and `agent-assisted` handling, while the generic issue path remains available for caller-owned repositories.

> **Queue prompt:** tools.md issue
> ```
> › agent-doc /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/tools.md
> queue or backlog line the live buffer has and the disk projection does not). It is either operator steering or an
> earlier `agent-doc write` whose own commit refused; this check compares components and cannot tell which, so it does
> not guess. Either way it is not a failed closeout. Answer it: run `agent-doc /home/brian/work/btakita/agent-loop/src/
> haiven-dev/tasks/tools.md` to open the next cycle, which reads that edit as its prompt and commits it. Do NOT run
> `agent-doc commit /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/tools.md` — every commit path refuses a
> fresh unanswered edit on purpose, because committing it would swallow the next turn's prompt — and do NOT force disk,
> which clobbers the live edits. Do not recapture the response or rerun finalize/write. Run `agent-doc session-check /
> home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/tools.md` and let the binary-owned keyed retry finish.
> ```

This cross-document closeout issue was already resolved in its owning session. This row is a stale completion echo; no recursive agent-doc invocation is performed from this owner pane.

> **Queue prompt:** Allow preset to recognize the entire text and support multiple presets in the text. Attribute was `preset="#push #auth"`. See tools.md:
> ```
> • The #push preset itself is now defined, but the queue attribute is preset="#push #auth", which the document engine
>   treats as one undefined preset name. I’ll follow the generated contract exactly and dispatch a fresh verifier; I
>   won’t reinterpret the combined preset string or perform an unauthorized push.
> ```

The multiple-preset parsing work was already resolved and verified earlier; this row is likewise a stale completion echo.

> **Queue prompt:** Add a `agent:todo` to store todo items for the operator to act on. Unpublished drafts are a common todo item. Add `agent:todo` into the agent document template for Claim for Tmux Pane command. The agents should be instructed to make a brief TODO item. It can refer to `agent:review` + `agent:backlog` items, draft files, etc.

Completed in `cac7c4a211` and included in `v0.35.500`: the authoring contract now defines brief operator TODO entries and permits references to review/backlog IDs and draft paths.

> **Queue prompt:** release + publish

Completed. Version `0.35.500` is public on GitHub, PyPI, and Homebrew, with the full hosted workflow green.

> **Queue prompt:** When the agent prompts the developer, show the prompt choices with context in the agent:exchange.

Completed in `ca95a2777a`. The bounded `choice-checkpoint` path mirrors the public context, question, and two-to-five choices through the existing salient-response CRDT path without treating a displayed choice as authorization or answer evidence.

> **Queue prompt:** During a hot-reload, tmux pane auto-sync is temporarily not operational.

Completed in `9fa63104ae`. Dynamic JetBrains replacement now preserves the controller-owned editor surface and focus across reload, while an actual terminal close still retires it.

> **Queue prompt:** Can we guide subagents to surface prompts + permission requests to the user across all supported agent harnesses (Claude Code + Codex + Grok Build + OpenCode)? If yes, add the functionality to agent-doc.

Yes; completed in `1953755617`. Agent-doc now defines a deterministic `#waypostrequest` packet carrying the exact request kind, request text, context, and choices, and routes it through the coordinator exchange on Claude Code, Codex, Grok Build, and OpenCode. Conversational approval remains distinct from a native tool permission grant.

> **Queue prompt:** Fix agent-doc causes of agent-doc-bugs.md Stop hook issue:
> ```
> • Blocked by hook
>   └ agent-doc Stop hook found fresh unresolved exchange work for /home/brian/work/btakita/agent-loop/tasks/agent-doc/
> agent-doc-bugs.md after the previous cycle was already committed. Continue THIS turn in-pane: answer "Can we guide
> subagents to surface prompts + permission requests to the user across all supported agent harnesses (Claude Code + Codex
> + Grok Build + OpenCode)? If yes, add the functionality to agent-doc." in /home/brian/work/btakita/agent-loop/tasks/
> agent-doc/agent-doc-bugs.md and persist with `agent-doc finalize /home/brian/work/btakita/agent-loop/tasks/agent-doc/
> agent-doc-bugs.md` (or `agent-doc write --commit /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-
> bugs.md`). Do NOT send the final answer yet.
>
> • The newly admitted prompt is already claimed by the dedicated cross-harness worker. I’m persisting that answer in its
>   own agent-doc cycle now: yes, the intended design is to surface subagent prompts and permission requests through the
>   coordinator with exact context, while preserving each harness’s native permission boundary.
> ```

Completed in `6f5149960f`. The Codex Stop path now recomputes authoritative pending steering: a diff fully owned by `forward_to_owner` continues with its existing worker, while mixed or genuinely unclaimed steering still blocks closeout.

All five isolated worker results were integrated serially, centrally verified, installed once, pushed, and their claims released.
> **Chat prompt (#chatprompt):** Let me know when the release is published.

### Re: release published — gpt-5 · 2026-10-14T16:17-04:00

Agent-doc `v0.35.500` is published:

- GitHub: https://github.com/btakita/agent-doc/releases/tag/v0.35.500
- PyPI: https://pypi.org/project/agent-doc/0.35.500/
- Homebrew: `btakita/homebrew-tap` formula version `0.35.500`

The hosted release workflow is green across all target platforms, including Windows. The verified post-release integration batch is installed locally and pushed to `btakita/agent-doc` main through `840625b9c0`.
<!-- agent:boundary:3599dd10 -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built
<!-- agent:queue subagents preset="#spec-test-build-install-commit-push" priority -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#jb262dynlauncherrecommen] [recommended] [operator-verify] [agent-doc] Operator decision: jb262dynupgrade (153c72916) chose the accurate-warning option (a 262 package reports declined_by=modular_package and falls back to file replacement + restart). Either keep that as the supported policy (then close this), or authorize a real restart-free launcher for the exact-262 split-mode plugin that swaps backend + frontend (the frontend may run in a separate JetBrains Client process), proven against a live 262 IDE.
- [/] [#pypiquotarecheck] Track pypi/support#12214 (agent-doc project limit request, 25 GiB) to a decision. Unblocked mechanically by a human reply on pypi/support#12214, or make pypi-quota-check reporting a ceiling in reach. Rechecked 2026-10-12: pre-cutover 131 releases 8.361 GiB (0.35.113..0.35.343), post-cutover 91 releases 3.19 MiB (mean 35.9 KiB), about 47,722 further releases fit; VERDICT headroom fine. The support issue is still open with one comment, unchanged since 2026-09-10. Nothing to act on or delete. If the grant lands, record it and drop this item; deletion stays reserved for a ceiling the authenticated check reports.
<!-- /agent:review -->

<!-- agent:icebox -->
- [ ] [#ghrecvpolicy] [agent-doc] Recipient-side [receive] auto_create_components policy for multi-user workspaces (GH #40, https://github.com/btakita/agent-doc/issues/40). Parked until multi-user workspace support exists.
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc/agent-doc-bugs.done.md -->

<!-- completed work archived in tasks/agent-doc/agent-doc-bugs.done.md -->
<!-- /agent:done -->
