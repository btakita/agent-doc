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
queue: go
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
<!-- agent:boundary:b66994af -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built
<!-- agent:queue subagents preset="#spec-test-build-install-commit-push" priority go -->
- ~~#gh-fix https://github.com/btakita/agent-doc/issues/243~~
- #gh-fix https://github.com/btakita/agent-doc/issues/254
- Is the project-wide agent-doc template (i.e. presets) released + published? How do I use it?
---
tools.md issue
```
› agent-doc /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/tools.md
queue or backlog line the live buffer has and the disk projection does not). It is either operator steering or an
earlier `agent-doc write` whose own commit refused; this check compares components and cannot tell which, so it does
not guess. Either way it is not a failed closeout. Answer it: run `agent-doc /home/brian/work/btakita/agent-loop/src/
haiven-dev/tasks/tools.md` to open the next cycle, which reads that edit as its prompt and commits it. Do NOT run
`agent-doc commit /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/tools.md` — every commit path refuses a
fresh unanswered edit on purpose, because committing it would swallow the next turn's prompt — and do NOT force disk,
which clobbers the live edits. Do not recapture the response or rerun finalize/write. Run `agent-doc session-check /
home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/tools.md` and let the binary-owned keyed retry finish.
```
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
