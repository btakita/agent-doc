---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: ba846c4c-2e01-4e12-9539-096faec207ee
  codex: 01a13cac-54da-7403-9456-6fc418818870
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20261014-230139.md`*

Compacted content:
- Archived 17 response topic(s): strict-template failure and steering stall; retained closeout and SDK queue continuation; integrated queue fixes; 14 more
- Prior summary/context: prior compacted content: Archived 30 response topic(s): #actionable-review; GitHub issue #245; Restart Agent retained...

### Re: agent-doc fixes, release, and tmux auto-sync — gpt-5 · 2026-10-14T21:25-04:00

> **Chat prompt (#chatprompt):** cont

> **Chat prompt (#chatprompt):** did you fix https://github.com/btakita/agent-doc/issues/259 + https://github.com/btakita/agent-doc/issues/260? they are still open on gh

> **Chat prompt (#chatprompt):** Why is tmux auto-sync not working?

> **Chat prompt (#chatprompt):** Are we on the new agent-doc version?

> **Chat prompt (#chatprompt):** why did our tests not catch this regression?

The batch is complete and pushed.

[GitHub issue #259](https://github.com/btakita/agent-doc/issues/259) is fixed and CLOSED. The JetBrains 263 modular package is now classified correctly, with truthful restart-required messaging. It landed on main in `7abf135dd7` and closed at 2026-10-15T00:24:33Z.

[GitHub issue #260](https://github.com/btakita/agent-doc/issues/260) is fixed and CLOSED. Remote Development tab selection now preserves frontend authority while handling nullable window liveness correctly. It landed on main in `8ba8701aa8` and closed at 2026-10-15T00:24:22Z.

> **Queue prompt:** Make .agent-doc/project.md work with nested projects...such as boost-client or haiven-dev. The parent project.md should be picked up with the child project.md presets overlaid on top.

Implemented in `298d4edece`: parent `.agent-doc/project.md` presets are inherited up to the project-root ceiling, nearer child presets overlay them, document-local settings win, explicit unsets mask inherited values, and malformed ancestors fail closed.

> **Queue prompt:** tmux pane auto-sync not working

Root cause found and fixed. JetBrains could publish the current editor surface while `FileEditorManagerEx.windows` still returned a non-empty but lagging snapshot that omitted it. The publisher then immediately emitted `editor_surface_forget` for its own endpoint; the controller correctly tombstoned that generation and rejected every later update, leaving the tmux layout stale.

The tests missed it because they covered internally consistent live-window sets and the Remote Development empty/unknown-window case, but not the mixed boundary state: a valid `currentWindow` plus a non-empty stale window enumeration that omitted that same window. They also lacked a source-boundary test proving that the IDE adapter fed the shared liveness logic correctly.

The fix centralizes liveness calculation, unions the exact current/published endpoint into a non-empty backend snapshot, preserves empty backend windows as unknown, and adds a defense-in-depth guard preventing a publisher from retiring itself. Regression tests now cover the lagging enumeration, detached current surface, unknown empty-window state, and the shared-adapter boundary. The implementation landed on main as `4d69b6ff81`; package generations landed as `4e1cde817d`.

The authoritative `make check` passed with exit 0 and zero failures (`tart-a5d98fc2dc`). `make install` then installed the checked build: CLI/native `agent-doc 0.35.502`, live IntelliJ plugin `0.2.604` via dynamic upgrade, Rider 2026.2 plugin `0.2.606` for its next start, and VS Code `0.2.81`. The post-push CI run for `4e1cde817d` is currently in progress; the preceding release CI is green.

> **Queue prompt:** release + publish

Released and published v0.35.502. The tag and 15 release assets are available at https://github.com/btakita/agent-doc/releases/tag/v0.35.502; the release workflow, PyPI publication, and Homebrew publication all succeeded. This tagged release contains #259, #260, and nested-project configuration inheritance.

We are on the new `agent-doc 0.35.502` locally. One precise release boundary: the tmux auto-sync fix arrived immediately after the v0.35.502 tag, so it is installed locally from current main under version 0.35.502, but it is not contained in the public v0.35.502 release artifacts.

### Re: GitHub issue 259 closeout — gpt-5 · 2026-10-14T21:26-04:00

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/259

Fixed on main in `7abf135dd7`, included in v0.35.502, and verified CLOSED on GitHub at 2026-10-15T00:24:33Z. JetBrains 263 modular packages now use the modular-package classification and truthful restart-required messaging.

### Re: GitHub issue 260 closeout — gpt-5 · 2026-10-14T21:27-04:00

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/260

Fixed on main in `8ba8701aa8`, included in v0.35.502, and verified CLOSED on GitHub at 2026-10-15T00:24:22Z. Remote Development tab selection now preserves frontend authority while handling nullable window liveness correctly.

### Re: release and publish v0.35.503 — gpt-5 · 2026-10-14T22:22-04:00

> **Chat prompt (#chatprompt):** release + publish

Released and published agent-doc v0.35.503 with the tmux pane auto-sync fix.

- Release commit: `748c9381a7`
- Tag and 15 public assets: https://github.com/btakita/agent-doc/releases/tag/v0.35.503
- GitHub Release workflow: all platform builds, editor packages, release assembly, PyPI, and Homebrew completed successfully: https://github.com/btakita/agent-doc/actions/runs/38668295784
- PyPI version-specific publication reports `0.35.503`.
- Authoritative `make check` passed with exit 0 and zero failures (`tart-484ec1cfcd`). The exact-byte `make tmux-ci` suite also passed before the tag was pushed.
- Local installation is complete: CLI/native `agent-doc 0.35.503`, live IntelliJ plugin `0.2.607`, Rider 2026.2 plugin `0.2.608` for its next start, and VS Code `0.2.81`.

The initial release check correctly stopped on a missing v0.35.503 editor-parity audit row. I added the tmux liveness parity record, amended the release commit, and reran the full check successfully before publishing.
<!-- agent:boundary:3599dd10 -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built
<!-- agent:queue subagents preset="#spec-test-build-install-commit-push" priority go -->
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
