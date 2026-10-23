---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: ba846c4c-2e01-4e12-9539-096faec207ee
  codex: 01a1545a-f42c-7aa1-b9a0-aeafeba96910
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
presets:
  '#agent-doc-causes': Fix agent-doc causes
  '#audit': 'Audit agent-doc runtime for dogfooding. Add issues to the backlog + queue.'
  '#repo': 'https://github.com/btakita/agent-doc'
  '#self': agent-doc-bugs.md
dogfood:
  path: tasks/agent-doc/agent-doc-bugs.md
  auto: true
queue: go
---

## Status

<!-- agent:status patch=replace sync="##sync-release" -->

📤 Queue handoff: 12 reported items awaiting coordinator integration, verification, and closeout.
Main is at `b6b9af6e15` and agent-doc 0.35.532 is installed locally. Release v0.35.532 was published from `71f9801b49`. Post-release main now includes the inline-code-safe, occurrence-ordered queue preset authorization fix for GitHub issue #331 and the positive-evidence-only Remote Dev pane-order fix for GitHub issue #332; both issues are closed, the integrated full suite passed, and the current mainline was installed locally.

<!-- status:sync -->
Latest published agent-doc release: v0.35.545, published on GitHub at 2026-10-23 02:16:55 UTC from b3839fdd6d90cb50e00551f1ec7e9fc30d28edaa. GitHub Release publication is complete with 21 assets; PyPI build, publish, and clean-install verification passed; the public wheel is agent_doc-0.35.545-py3-none-any.whl. Remote main and tag v0.35.545 both resolve to b3839fdd6d, and agent-doc 0.35.545 is installed locally. The post-release integration branch is at 09aa2b50b6 with response-bearing subagent completion feedback, positive Remote Dev frontend-authority readiness diagnostics, three-range JetBrains Marketplace automation, JetBrains generations 0.2.798/0.2.799/0.2.800, operator-answer lifecycle/inline-code parsing fixes, and nested emphasized operator-request formatting. Canonical agent:loop is fully verified and awaiting integration; canonical agent:run is in its corrected final composite check. Marketplace publication for the next release is blocked on one-time vendor/listing onboarding and the missing JETBRAINS_MARKETPLACE_TOKEN repository secret. The Homebrew tap still visibly points at v0.35.541, so its asynchronous bump remains pending.
<!-- /status:sync -->
<!-- /agent:status -->
 

## Exchange

<!-- agent:exchange patch=append subagents -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20261022-143613.md`*

Compacted content:
- Archived 15 response topic(s): recovered response; Subagent completed: `subagent:unique_request_ids` returned work for queue item “api.md has 2 subagent questions with ...; no-guess layout authority, hook recovery, and v0.35.537; 12 more
- Prior summary/context: prior compacted content: Archived 5 response topic(s): #gh-fix issue 331; #gh-fix issue 332; releases, realtime fixes...

### Re: Release v0.35.544 published and release workflow hardened

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/352

> **Queue prompt:** release + publish

> **Queue prompt:** The `agent:queue loop` does not seem to be reliably firing.

> **Queue prompt:** Make the process-wide queue scheduler replace a disconnected worker and surface failed refresh enqueue receipts instead of discarding `mpsc::Sender::send` errors; its `OnceLock` handle currently cannot recover if the scheduler thread exits.

> **Queue prompt:** There are various queue items stuck with the outbox. Not sure why. In agent-doc-bugs.md + tsift.md

> **Queue prompt:** In tsift.md session: • Blocked by hook
>   └ Session persistence continuation, not a refusal or error: agent-doc Stop hook found fresh unresolved exchange work

> **Queue prompt:** for /home/brian/work/btakita/agent-loop/tasks/software/tsift.md after the previous cycle was already committed. Continue THIS turn in-pane: answer "release + publish latest updates" in /home/brian/work/btakita/agent-loop/tasks/software/ tsift.md and persist with `agent-doc finalize /home/brian/work/btakita/agent-loop/tasks/software/tsift.md` (or `agent- doc write --commit /home/brian/work/btakita/agent-loop/tasks/software/tsift.md`). It is a queue line: open the response with the verbatim echo `> **Queue prompt:** release + publish latest updates` and persist it with `agent-doc finalize / home/brian/work/btakita/agent-loop/tasks/software/tsift.md`; a `### Re:` heading alone leaves the line queued; that closeout strikes it. Do NOT send the final answer yet.

> **Queue prompt:** Heads up: my coder instance is getting many "Remote Dev provided flat editor evidence without column geometry" errors on Run Agent Doc + Sync Tmux Panes. Issues incoming. But get a head start to make this process formal logic backed and fix this class of bug.

> **Queue prompt:** Can agent-doc provide a tree of operations when evaluating paths to go down? I'm having issues with my client using Coder, where Claude Code will propose many strangs to go down. This increases token spend on spe

> **Queue prompt:** #gh-issue https://github.com/btakita/agent-doc/issues/353

> **Queue prompt:** #gh-issue https://github.com/btakita/agent-doc/issues/354

> **Queue prompt:** #gh-issue https://github.com/btakita/agent-doc/issues/355

> **Queue prompt:** #gh-issue https://github.com/btakita/agent-doc/issues/356

> **Queue prompt:** : lazily.md is working on the lazily-rs + spec + conformance implementations.

> **Queue prompt:** #gh-issue https://github.com/btakita/agent-doc/issues/355

> **Queue prompt:** #gh-issue https://github.com/btakita/agent-doc/issues/356

> **Queue prompt:** #gh-issue https://github.com/btakita/agent-doc/issues/357

> **Queue prompt:** The queue items with issues 353-357 started with `📥` then went to `🟡`. I thought `📥` meant the outgoing inbox, but is this the incoming inbox? Can we have distinct icons for incoming & outgoing? Actually I do see the distinct `📤` outgoing icon. It's a bit tough to distinguish because the down arrow and up arrow look very similar...but I can train myself to spot the difference. Perhaps there may be alternate emoji pairs that are easier to distinguish? I think  📥 / 📫 is the best pair overall as it is a single utf-8 character & is distinctive with small font sizes...which is a better tradeoff than 📥 / 📤, which is symmetric but isn't distinctive enough at smaller font sizes.

> **Queue prompt:** Make the emoji icons configurable with defaults. Also allow the "emoji" icons to be any set of characters in case someone wants to use multiple characters for the prefix.

> **Queue prompt:** In lazily.md, a 🙋 was raised: `- 🙋 [#subagent-operator-request-lzstale-release] 🚧 [#subagent:spec#1] do [#lzstalehandlespec]: release + publish`: I answered the question: `- 🙋 [#subagent-operator-request-lzstale-release] 🚧 [#subagent:spec#1] do [#lzstalehandlespec]: release + publish: Coordinator releases`: After answering the question prompt, the 🙋 should be removed. The `[#subagent-operator-request-lzstale-release]` should be appended rather than prepended, since it is not the primary intent of the turn. It may block the turn but it's secondary. Also, I can append an answer as `[#subagent-operator-request-lzstale-release]: release + publish: Coordinator releases` with locality. The states should be blocking prompt: `🙋🚧`, unblocked: `🚧`.

> **Queue prompt:** I prefer `☑️` over `📫`? Is the current `📫` effectively "done"? If yes, use `☑️instead.

> **Queue prompt:** In sdk-demo.md, there was a `🙋☑️`. I did not answer the prompt. On the next turn, agent-doc removed the item from the queue. The item should have remained since it was blocked with a user prompt.

> **Chat prompt (#chatprompt:571e97b7f4cb):** Add all the spec + conformance + parity tasks to lazily.md

> **Chat prompt (#chatprompt:a835c43727cc):** lazily rs + kt + js are released with the requested features

> **Chat prompt (#chatprompt:a94066fb70b0):** Please update dependencies and update the implementation in subagent(s)

> **Chat prompt (#chatprompt:21d3663acae9):** Will this current release pass fix the issues or do we need the lazily upgrades?

> **Chat prompt (#chatprompt:7750e89e183c):** Should we wait to release until full parity? I want to be efficient with token usage on my coder instance

> **Chat prompt (#chatprompt:62163b09be86):** How much more time until we release + publish?

> **Chat prompt (#chatprompt:3776347e0309):** not published yet?

> **Chat prompt (#chatprompt:ac57f8847cdd):** is it available publically?

> **Chat prompt (#chatprompt:7401edeba9aa):** can coder instance install it?

> **Chat prompt (#chatprompt:bceba9b84b80):** It installs binaries

> **Chat prompt (#chatprompt:ba3364d8aefe):** is the binary ready?

> **Chat prompt (#chatprompt:4f52cc580942):** why is this taking so long?

> **Chat prompt (#chatprompt:35248da40a3f):** What is the configuration options for custom prefixes?

> **Chat prompt (#chatprompt:f2cfcebb8685):** Is this documented?

> **Chat prompt (#chatprompt:c669081c3d24):** Is the release publised?

> **Chat prompt (#chatprompt:b1abf984bf71):** cont

> **Chat prompt (#chatprompt:2c94451b2730):** when was the last release published?

> **Chat prompt (#chatprompt:ecb6f9006f85):** This release cycle is insanely long

> **Chat prompt (#chatprompt:87c487e1eea8):** Can we make each platform binary immediately available, with a idempotent retry if one fails? One failing should not cause the entire build to fail. Implement in a subagent.

> **Chat prompt (#chatprompt:81e3d80f667e):** Is the release done?

Released and published `v0.35.544`. The GitHub Release became public at 2026-10-22 23:47:03 UTC (7:47:03 PM EDT): https://github.com/btakita/agent-doc/releases/tag/v0.35.544. All six platform archives, `SHA256SUMS`, VS Code 0.2.83, and JetBrains classic/262/263 packages are public. PyPI publication and clean-install verification passed; the Homebrew handoff passed.

The official binary installer was verified from a clean prefix with `AGENT_DOC_VERSION=v0.35.544`: it downloaded and verified the public Linux musl archive, and the installed static binary reported `agent-doc 0.35.544`. Coder can install with `curl -fsSL https://raw.githubusercontent.com/btakita/agent-doc/main/install.sh | sh`.

The prior completed release was `v0.35.542`, published 2026-10-22 16:18:25 UTC (12:18:25 PM EDT). `v0.35.543` was tagged but never became a GitHub Release because its Lazily Kotlin packaging helper omitted sibling `lazily-spec`; `.544` fixed that failure.

The required Lazily upgrades are in `.544`: Rust `0.61.0` / macros `0.23.1`, Kotlin `0.46.0`, JavaScript `0.37.0`, and koffi `3.0.2`, with stale-handle coverage. Unrelated full language parity was not required; direct Rust/Kotlin/JavaScript consumer parity was the efficient release boundary. Twelve spec/conformance/parity tasks were added to `tasks/lazily/lazily.md`.

The cycle was long because `.543` failed packaging, a dirty local Lazily Kotlin 0.44 checkout had silently substituted for declared 0.46, the corrected exact-source 11,420-test gate had to rerun, and the fat-LTO matrix followed. Intel macOS alone took 43m49s.

The next release no longer waits to expose every successful binary. `main` commit `c501ad975e` creates/reuses one public Release first; each successful platform immediately uploads its archive and sidecar checksum; `fail-fast: false` preserves successful siblings; and stable names plus `--clobber` make “Re-run failed jobs” idempotent. Partial releases remain visibly red, while aggregate `SHA256SUMS`, PyPI, and Homebrew stay fail-closed until all six platforms and editor packages verify. Integrated contract tests passed 7/7; publication self-test, plugin checks, actionlint, and the subagent's full `make check` passed.

Reported queue work now uses canonical `☑️`; legacy `📫`/`📤` remain readable. `☑️` means worker completion/handoff, not necessarily coordinator closeout. The next-turn bug that removed unanswered `🙋☑️` rows is fixed on `main` at `7339bda39a`; its full gate passed 11,422 Rust tests and all editor, JetBrains, Python, Lean, and TLA checks. This post-tag fix is installed locally but cannot be retroactively added to immutable `.544`.

Custom prefixes are documented in `docs/getting-started/configuration.md`, `specs/06-config.md`, and `docs/reference/features.md`. Defaults are `pending="📥"`, `claimed="🟡"`, `running="🚧"`, `reported="☑️"`, `lost="⚠️"`, `reconciling="🔄"`, `unknown="❔"`, `skipped="⏭️"`, `awaiting_operator="⏳"`, `operator_action="🙋"`, `prioritized="📌"`, and `agent_prioritized="📍"` under `[queue_icons]`. Values may be arbitrary distinct, non-empty, single-line, multi-character strings without surrounding whitespace; omitted values keep defaults and legacy aliases remain readable.

The queue loop recovery, failed enqueue receipt surfacing, stuck-outbox repair, Stop-hook continuation, remote-editor geometry formalization, operation-tree support, GitHub issues 352–357, operator-request locality, and duplicate/corrected queue items were integrated and verified. The local binary and editor packages were rebuilt and installed; `agent-doc --version` reports `0.35.544`.

### Re: Continuation after release closeout

> **Chat prompt (#chatprompt:03057a4af221):** cont

Continuation complete. `v0.35.544` is public and its binary installer and PyPI bootstrap were verified. Post-tag fixes for unanswered `🙋☑️` retention and independent, idempotent per-platform asset publication are pushed to `main` at `c501ad975e`. The prior cycle passed `agent-doc session-check` with no drainable work; no additional implementation remains in this continuation.

### Re: recovered response

Yes—primarily for a proper Remote Dev frontend installation.

Marketplace distribution lets JetBrains install the correct compatibility-ranged plugin on both sides:

- The remote IDE backend.
- The local JetBrains Client/Gateway frontend.

Without Marketplace, Agent Doc still works through manual ZIP installation, but the matching plugin must be installed manually on both peers. A backend-only install causes the one-sided state behind the recent routing failures.

The release automation publishes separate builds for classic IDEs, 262, and 263 so Marketplace can select the correct one. Marketplace is therefore not required for local development, but it is the reliable installation/update channel for Remote Dev. The plugin is not listed there yet because vendor onboarding and `JETBRAINS_MARKETPLACE_TOKEN` remain outstanding. [JetBrains Marketplace API documentation](https://plugins.jetbrains.com/docs/marketplace/plugin-details.html)
<!-- agent:boundary:0f221edb -->
<!-- /agent:exchange -->

<!--
-->

## Queue: Main
<!-- agent:queue id="main" subagents preset="#spec-test-build-install-commit-push" priority go -->
- ☑️ [#subagent:gh355#1] #gh-issue https://github.com/btakita/agent-doc/issues/355
- ☑️ [#subagent:gh356#1] #gh-issue https://github.com/btakita/agent-doc/issues/356
- ☑️ [#subagent:gh355status#1] Is `☑️ [#subagent:gh355#1] #gh-issue https://github.com/btakita/agent-doc/issues/355` done? It is leftover in the queue. Issues 353, 354, 355, 357 remain open on github.
- ☑️ [#subagent:gh353fix#2] #gh-fix https://github.com/btakita/agent-doc/issues/353
- ☑️ [#subagent:gh354fix#2] #gh-fix https://github.com/btakita/agent-doc/issues/354
- ☑️ [#subagent:gh355#1] #gh-fix https://github.com/btakita/agent-doc/issues/355
- 📥 #gh-fix https://github.com/btakita/agent-doc/issues/357
- 📥 Did you reimplement "guessing"? The tmux panes are reversed on my coder instance. The "guessing" seems to be wrong more often than not.
- 📥 #gh-fix https://github.com/btakita/agent-doc/issues/358
- 📥 #gh-fix https://github.com/btakita/agent-doc/issues/359
- 📥 #gh-fix https://github.com/btakita/agent-doc/issues/360
- 📥 "Re: Subagent completed:" should immediately add the response into agent:exchange.
- 📥 How do we improve the reliability of the routing? Why did the guess occur in https://github.com/btakita/agent-doc/issues/360? On top of fail no-close routing, we need to figure out how to not get into the fail state.
- 📥 Automate publication of all three compatibility-ranged JetBrains updates to Marketplace, with token/onboarding and exact-range verification, so Remote Dev synchronizes the modular ZIP to both peers and prevents one-sided installs.
- 📥 In sdk-demo.md, no queue item status despite agent running: `- [#subagent-operator-request-publish-ci-pr166]: **Commit and push to PR #166**`
- 📥 release + publish: no `agent:run` + `agent:loop`
- ☑️ [#subagent:operator-request-format#1] The formatting of a subagent operator request is difficult to read the key + values. What are some formatting options to make it easier to see the keys + values? How about making a nested24list and emphasizing the keys `- *kind:*`?
- ☑️ [#subagent:rename-queue-run#1] Rename `agent:queue` to `agent:run`. Keep the alias compatability.
- ☑️ [#subagent:rename-loop#1] . What are some formatting options to make it easie Rename `agent:run loop`/`agent:queue loop` to `agent:loop`. Keep backward compatability.
- ☑️ [#subagent:release-runloop#1] release + publish: include `agent:run` + `agent:loop`
- ☑️ [#subagent:longrun-strike#1] Even though this run has been going for > 3 hours, agent-doc should still strike items that can be deleted.
- ☑️ [#subagent:gh361#1] #gh-fix https://github.com/btakita/agent-doc/issues/361
- ☑️ [#subagent:sdk-request-format#1] sdk-demo.md subagent operator request does not have the italics and list items. Is this implemented and installed locally?
- 🚧 [#subagent:queue-state-regression#1] The previous state of various items in this queue were ☑️, but now the state is 📥.
- 🚧 [#subagent:completion-feedback-async#1] There is a long list of "Subagent completed" but no response. agent-doc should immediately respond with the subagent completion feedback for tracked queue items. Can this be done without slowing down the agent? Like a fire & forget with agent-doc correctly formatting the exchange response?
- 🚧 [#subagent:gh362#1] #gh-fix https://github.com/btakita/agent-doc/issues/362
<!-- /agent:queue -->

## Queue: Issues

<!-- agent:queue id=issues loop interval=1h -->
- 🚧 Watch https://github.com/btakita/agent-doc/issues for new issues. Add new issues into the backlog + queue except issue #40 + #45.
<!-- /agent:queue -->

## Todo
<!-- agent:todo -->
- [ ] [#ahfe] Live-check GH #267 on the 263 IDE: after `agent-doc upgrade` WITHOUT restart, once idea.log shows `[native] hot-reloaded libagent_doc`, Run Agent Doc should complete (ops.log `editor_route_superseded_editor_admitted`, route output ends with the advisory line). 2026-10-15 operator: did not work (believed); watch for bug reports before reopening GH #267.
- [ ] [#ctg7] Under heavy load, switch the editor 2 panes -> 1 -> 2 and confirm tmux follows and stays at 2 (stale-controller fix).
<!-- /agent:todo -->

## Backlog
<!-- agent:backlog priority queue -->
<!-- /agent:backlog -->

## Review
<!-- agent:review -->
- [/] [#jb262dynlauncherrecommen] [recommended] [operator-verify] [agent-doc] Operator decision: jb262dynupgrade (153c72916) chose the accurate-warning option (a 262 package reports declined_by=modular_package and falls back to file replacement + restart). Either keep that as the supported policy (then close this), or authorize a real restart-free launcher for the exact-262 split-mode plugin that swaps backend + frontend (the frontend may run in a separate JetBrains Client process), proven against a live 262 IDE.
- [/] [#pypiquotarecheck] Track pypi/support#12214 (agent-doc project limit request, 25 GiB) to a decision. Unblocked mechanically by a human reply on pypi/support#12214, or make pypi-quota-check reporting a ceiling in reach. Rechecked 2026-10-12: pre-cutover 131 releases 8.361 GiB (0.35.113..0.35.343), post-cutover 91 releases 3.19 MiB (mean 35.9 KiB), about 47,722 further releases fit; VERDICT headroom fine. The support issue is still open with one comment, unchanged since 2026-09-10. Nothing to act on or delete. If the grant lands, record it and drop this item; deletion stays reserved for a ceiling the authenticated check reports.
- [/] [#gh299-live-263-proof] Live-verify v0.35.519 on JetBrains 263.7031 with changed plugin sources: `agent-doc upgrade` must hot-reload and complete without an IDE restart; close GH #299 only with that proof.
<!-- /agent:review -->

<!-- agent:icebox -->
- [ ] [#ghrecvpolicy] [agent-doc] Recipient-side [receive] auto_create_components policy for multi-user workspaces (GH #40, https://github.com/btakita/agent-doc/issues/40). Parked until multi-user workspace support exists.
<!-- /agent:icebox -->

## Completed / Reaped
<!-- agent:done archive=tasks/agent-doc/agent-doc-bugs.done.md -->

<!-- completed work archived in tasks/agent-doc/agent-doc-bugs.done.md -->
<!-- /agent:done -->
