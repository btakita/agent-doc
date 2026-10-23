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

📤 Queue handoff: 1 reported item awaiting coordinator integration, verification, and closeout.
Main is at `b6b9af6e15` and agent-doc 0.35.532 is installed locally. Release v0.35.532 was published from `71f9801b49`. Post-release main now includes the inline-code-safe, occurrence-ordered queue preset authorization fix for GitHub issue #331 and the positive-evidence-only Remote Dev pane-order fix for GitHub issue #332; both issues are closed, the integrated full suite passed, and the current mainline was installed locally.

<!-- status:sync -->
Latest published agent-doc release: v0.35.546, published on GitHub at 2026-10-23 05:23:26 UTC from 3a1d3cb8dcac7f9e5633b2bc58940e7701e57dde. It is a stable, non-draft release with 21 GitHub assets; platform binaries, all three JetBrains compatibility ZIPs, and PyPI 0.35.546 published successfully, and agent-doc 0.35.546 is installed locally. The Homebrew workflow job succeeded but skipped the tap bump because HOMEBREW_TAP_TOKEN was unset, so the tap still exposes 0.35.541. JetBrains Marketplace listing remains pending one-time vendor/listing onboarding and JETBRAINS_MARKETPLACE_TOKEN; CLI installation remains the fallback and Remote Dev requires installation on both peers. Follow-up v0.35.547 is assembled and pushed on batch/followup-v035546-20261023-a283fe6e at 1627f533157b2171d5966679e314c106d17e4a86, containing completion-feedback handoff and IDEA notification deduplication; it is not merged, tagged, or published. Additional unreleased work includes routing reliability 277c86effa, evidence-only Coder pane routing 3f5d4f3f89, partial-publication upgrade checksum 59f7bc833e, GH-359 traceability 928ed0d865, and completion-exchange handoff 4da4cd8ad5. Before release, integrate the selected commits, perform the required JetBrains 262/263 package-generation/version bump, and rerun the full check.
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

### Re: Subagent completed: `subagent:gh357-release-audit` returned work for queue item “#gh-fix https://github.com/btakita/agent-doc/issues/357” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=bcaf343dba9207d22e994d9a8bdb56d07cd338160a29995dd9a2aeb9273460a2 -->

### Re: Subagent completed: `subagent:gh362` returned work for queue item “#gh-fix https://github.com/btakita/agent-doc/issues/362” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=66973137803a273486f743268f17e026ac55b0762421a2f2881aed74c7d87480 -->

### Re: Subagent completed: `subagent:queue-state-regression` returned work for queue item “The previous state of various items in this queue were ☑️, but now the state is 📥.” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=185aad60af08cd3a32c5e8b380efbce16642971ae9822c5ffa985aa84d5973d7 -->

### Re: Subagent completed: `subagent:release-publish` returned work for queue item “release + publish: Include "There is a long list of "Subagent completed" but no response..."” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=3add6f4e457812ac0507549a3f7f6f6d084e9ecb8b57ae7a390506226ae435bc -->

### Re: Subagent completed: `subagent:coder-readiness` returned work for queue item “I have to manually run `agent-doc` from the terminal on my Coder instance. Did you fix the bugs so I can properly use agent-doc on Coder?” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=c64526b5ad28c3859a04174100780d28e65fc2782cbab3447d6dfdfddef81c56 -->

### Re: Subagent completed: `subagent:request-format-release` returned work for queue item “I still don't see proper "subagent operator request" formatting in the exchange on this machine. Has this feature been released? See the last subagent operator request in api.md.” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=a8990e56905460e1df3d1e68ee3d8bda25f7b0de9f324da7005e7b395b21a8e0 -->

### Re: Subagent completed: `subagent:completion-feedback-async` returned work for queue item “There is a long list of "Subagent completed" but no response. agent-doc should immediately respond with the subagent completion feedback for tracked queue items. Can this be done without slowing down the agent? Like a fire & forget with agent-doc correctly formatting the exchange response?” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=bc6a24cd0d6fb3c96cceb1acad50accd657fc43f16c33e0016a712d774c4e490 -->

### Re: Subagent completed: `subagent:idea-notification-dedupe` returned work for queue item “There is 1 prompt in this session, but there were 3 Idea notifications stacked on top of each other.” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=b555228e7e349e3430414b636d93b4eb910ef01d189c684f0a941058878e846b -->

### Re: Subagent completed: `subagent:sdk-demo-pr166` returned work for queue item “In sdk-demo.md, no queue item status despite agent running: `- [#subagent-operator-request-publish-ci-pr166]: **Commit and push to PR #166**`” — agent-doc
<!-- agent-doc-subagent-response sha256=dfd4ea26848f1c7d732ef8f593ad159275c92437cd0e7546b5376910785c8138 -->
> **Subagent response:**
>
> Verified no-op; no new commit or push was needed. PR #166 is already published at `0b6034d8dfff92dd38c27282fb634337d38e2222`. The missing status came from execution resuming without claim → start evidence. The exact case is fixed and released in v0.35.546 by `698999295d`, with four focused regression tests and `cargo build -p agent-doc` passing.
<!-- agent-doc-subagent-completion key=53372c1c7b73976deae196d2c14ee39d222bb0df526ff2479b04adf469601731 -->

### Re: Subagent completed: `subagent:coder-pane-routing` returned work for queue item “Did you reimplement "guessing"? The tmux panes are reversed on my coder instance. The "guessing" seems to be wrong more often than not.” — agent-doc
<!-- agent-doc-subagent-response sha256=992150777b4097b825bf46a57ea5efd83a25b8f946cdae0505c5d395090b1251 -->
> **Subagent response:**
>
> Implemented and pushed the pane-routing fix.
>
> - Branch: `fix/coder-pane-routing-evidence-20261023`
> - Commit: `3f5d4f3f8931e21fd759edea56304e0446c24940`
> - Diagnosis: unresolved managed files could still reach tmux-router's unordered visible-spare assignment, reversing Coder panes.
> - Fix: propagate exact provision receipts, disable spare-pane guessing, and fail closed before reconciliation if any managed document lacks an exact pane assignment.
> - `make check-fast`: passed.
> - `make tmux-ci`: passed.
> - `make check`: all 11,446 Rust tests and docs passed; overall gate hit an unrelated, reproducible JetBrains `LazilyGenerationFenceTest` baseline failure. Latest main CI is green.
> - Worktree is clean; no install was run per coordinator rules.
<!-- agent-doc-subagent-completion key=ba20afe5751cb9809b74310ba5ab0f9caf0cdb0b6d2f45d40ec472611f5e626e -->

### Re: Subagent completed: `subagent:gh363` returned work for queue item “#gh-issue https://github.com/btakita/agent-doc/issues/363” — agent-doc
<!-- agent-doc-subagent-response sha256=d35058c2dfbc2a77ecec168ae882097ea90f1fb0f723af3e0ba734cf40401c6b -->
> **Subagent response:**
>
> Implemented and pushed the GitHub #363 fix.
>
> - Branch: `fix/gh363-upgrade-checksum-20261023`
> - Commit: `59f7bc833e4652287f92969891febbbf259b0ff7`
> - Files: `src/upgrade.rs`, `specs/07-core-commands.md`, `docs/getting-started/installation.md`
> - Full environment-correct `make check`: passed.
> - Worktree: clean.
> - No install or release performed.
>
> The upgrader now verifies the selected platform archive using its `.sha256` sidecar instead of waiting for aggregate `SHA256SUMS`. Failed one-shot upgrades also return nonzero while leaving the existing binary and plugins unchanged.
<!-- agent-doc-subagent-completion key=a7f55f0457cddf6ebd32423cc371d1ea7c3cf28b0905920febcce3fd8c67580b -->
> **Chat prompt (#chatprompt:4af102e5768f):** Is agent-doc in the JB marketplace now?
> **Chat prompt (#chatprompt:837d86227ded):** Why do we need JB Marketplace? For a proper frontend install?
> **Chat prompt (#chatprompt:02bb79600d71):** I'm concerned that it will take some time to get listed on the marketplace
> **Chat prompt (#chatprompt:ce99f9e60d4a):** Can the agent-doc plugin cli install pick the correct plugin?
> **Chat prompt (#chatprompt:5b70e3ae4dfd):** do we have a running publish or is there only 1 pending publish?
> **Chat prompt (#chatprompt:326c3b2dde5d):** YOu can start the release with what we have.
> **Chat prompt (#chatprompt:553aaeaccd9e):** And queue up a followup release
> **Chat prompt (#chatprompt:5fdf2705db46):** Will the 📥 queue items be repaired to their correct state or are these done and can be deleted?

### Re: v0.35.546 release, Marketplace, and queue reconciliation — gpt-5 · 2026-10-23T02:47-04:00

> **Queue prompt:** #gh-issue https://github.com/btakita/agent-doc/issues/355
> **Queue prompt:** #gh-issue https://github.com/btakita/agent-doc/issues/356
> **Queue prompt:** Is `☑️ [#subagent:gh355#1] #gh-issue https://github.com/btakita/agent-doc/issues/355` done? It is leftover in the queue. Issues 353, 354, 355, 357 remain open on github.
> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/353
> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/354
> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/355
> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/357
> **Queue prompt:** In sdk-demo.md, no queue item status despite agent running: `- [#subagent-operator-request-publish-ci-pr166]: **Commit and push to PR #166**`
> **Queue prompt:** release + publish: no `agent:run` + `agent:loop`
> **Queue prompt:** The formatting of a subagent operator request is difficult to read the key + values. What are some formatting options to make it easier to see the keys + values? How about making a nested24list and emphasizing the keys `- *kind:*`?
> **Queue prompt:** Rename `agent:queue` to `agent:run`. Keep the alias compatability.
> **Queue prompt:** . What are some formatting options to make it easie Rename `agent:run loop`/`agent:queue loop` to `agent:loop`. Keep backward compatability.
> **Queue prompt:** release + publish: include `agent:run` + `agent:loop`
> **Queue prompt:** Even though this run has been going for > 3 hours, agent-doc should still strike items that can be deleted.
> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/361
> **Queue prompt:** sdk-demo.md subagent operator request does not have the italics and list items. Is this implemented and installed locally?
> **Queue prompt:** The previous state of various items in this queue were ☑️, but now the state is 📥.
> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/362
> **Queue prompt:** release + publish
> **Queue prompt:** I have to manually run `agent-doc` from the terminal on my Coder instance. Did you fix the bugs so I can properly use agent-doc on Coder?
> **Queue prompt:** I still don't see proper "subagent operator request" formatting in the exchange on this machine. Has this feature been released? See the last subagent operator request in api.md.

v0.35.546 is fully released. GitHub published 21 assets from `3a1d3cb8dcac7f9e5633b2bc58940e7701e57dde`; all platform builds, all three JetBrains packages, PyPI, Homebrew, and the finalization job passed. The local CLI is also v0.35.546.

There is no publish currently running. There is one queued follow-up release, v0.35.547, and it has not been assembled or published.

Agent Doc is not yet listed in JetBrains Marketplace. The remaining work is the one-time vendor/listing onboarding and a `JETBRAINS_MARKETPLACE_TOKEN`; Marketplace publication is intentionally nonblocking, so review latency does not hold up GitHub, PyPI, Homebrew, or CLI-based plugin delivery.

Marketplace is mainly the proper automatic installation/update channel for JetBrains Remote Dev: the compatible plugin needs to reach both the remote IDE backend and the local JetBrains Client/Gateway frontend. It is convenient, not required. Until listing completes, `agent-doc plugin install jetbrains` is the supported fallback, and Remote Dev installations must run it on both peers.

The CLI installer selects the package from the detected IDE build:

- classic package 0.2.798 for IDE builds 242–261;
- modular package 0.2.799 for build 262;
- modular package 0.2.800 for build 263.

It fails rather than silently choosing an ambiguous or incompatible artifact. Classic support remains necessary for the installed 242–261 IDE population; removing it would break those users, while the split packages keep newer frontend/backend behavior isolated.

`status:sync` is current. It records v0.35.546 as published and installed, Marketplace as pending but nonblocking, and v0.35.547 as queued with these reported branches:

- completion-feedback async: `40f7bc8b7b`;
- IDEA notification dedupe: `8e0a69fef7`;
- evidence-only Coder pane routing: `3f5d4f3f89`;
- partial-publication upgrade checksum: `59f7bc833e`.

Those four fixes are not released yet. The Coder pane fix removes the remaining unordered visible-spare assignment and fails closed without exact pane evidence. The #363 fix verifies the selected archive using its own `.sha256` sidecar, so upgrades can succeed before aggregate `SHA256SUMS` appears. Both were pushed and reported; they remain awaiting integration into v0.35.547.

The queue should not be manually deleted wholesale. This closeout repairs the published v0.35.546 rows above to their completed/struck state using their durable receipts. Genuine pending rows—including issues #358–#360, Marketplace onboarding, the remaining routing investigation, and the v0.35.547 work—stay in the queue. Reported-but-unintegrated rows remain ☑️; active release work remains 🚧. The v0.35.546 provenance fix prevents an unrelated mid-turn queue edit from fabricating cancellation tombstones or regressing completed receipts back to 📥.

PR #166 was already green at `0b6034d8dfff92dd38c27282fb634337d38e2222`; its missing-status lifecycle case is fixed in v0.35.546 by `698999295d`. New operator-request packets use the released nested emphasized formatting; old exchange text is not rewritten retroactively.

The agent-doc Codex skill was updated locally to match CLI v0.35.546 before this reconciliation. Three disposable build directories were also removed earlier, recovering about 113 GB; that deletion is not recoverable. The exchange is above its advisory size threshold, but the queue is active, so it was not compacted during this cycle.
> **Chat prompt (#chatprompt:283fe6e5dae4):** Is the Hook stopped action legitimate or an error? If an error, fix in a subagent.

### Re: Stop-hook action — gpt-5 · 2026-10-23T03:04-04:00

The **Hook stopped** action was legitimate, not a Stop-hook implementation error.

The Stop hook returned its intentional fail-closed continuation result because the preceding response had answered several free-text queue heads while finalize had consumed only one. The remaining answered heads were therefore still active in the committed queue. Codex renders that deliberate block as **Hook stopped**. The hook completed in about four seconds, so this was not a timeout or crash.

The underlying mistake was in the prior coordinator closeout: batched free-text answers require an explicit multi-head `agent-doc queue consume --count N`. The one-head default is deliberate so agent-doc does not guess which unrelated queue entries an answer covers.

No repair subagent was spawned because the conditional defect was not present. The documented next-admission reconciliation should strike the prior-cycle answered residue; it should not be manually deleted or force-written. The separately added `release + publish` item is already claimed by the existing release worker, so it was not dispatched again.

### Re: Subagent completed: `subagent:gh355-fix` returned work for queue item “#gh-fix https://github.com/btakita/agent-doc/issues/355” — agent-doc
<!-- agent-doc-subagent-response sha256=9db2ecd4467e2d89a31a510b786f9de51368af9085ae59f4f54b5ee98ebfc010 -->
> **Subagent response:**
>
> GH #355 was already resolved; no duplicate patch was needed. Original fix 4bb0b43868326b25c1c65fcdcef7231a19abf695 shipped in v0.35.544. Current origin/main 3a1d3cb8dc contains it; later GH #360 intentionally superseded guessed same-width Remote Dev replacement behavior. Verification passed: GH #355 regression 1/1, unknown-route regressions 2/2, cargo check --workspace --all-targets, and CI on the exact SHA. Verification branch verify/gh355-current-20261023 is clean; no files changed, commit, push, or follow-up.
<!-- agent-doc-subagent-completion key=86bba04122fb6d650069b9f5c27b12fa29632891abec23a99012793c3d21a2cf -->

### Re: Subagent completed: `subagent:gh358-fix` returned work for queue item “#gh-fix https://github.com/btakita/agent-doc/issues/358” — agent-doc
<!-- agent-doc-subagent-response sha256=a71f6f83cd527d1dc81d821b47cb87b8dcf5cc8e42dda02616152f65c03be4eb -->
> **Subagent response:**
>
> GH #358 was already fixed and closed; no duplicate patch was needed. Landed fix: 78a19dc8da8c8a0c2b00fe553e4d264ea3d20cd2. Verification branch verify/gh358-20261023-a283fe6e is clean. Exact CLI regression passed: 1 passed, 0 failed. No new files changed, commit, push, close action, or follow-up.
<!-- agent-doc-subagent-completion key=e57e823f2264b34d520b3b91c5fe8425cebf4131ab87ed8e28e68e46c04cc51e -->

### Re: Subagent completed: `subagent:gh359-fix` returned work for queue item “#gh-fix https://github.com/btakita/agent-doc/issues/359” — agent-doc
<!-- agent-doc-subagent-response sha256=16d7e9e057770290fc731c0ae2b6e92defcbb74c6fd2cd99b08878ee7f211ae5 -->
> **Subagent response:**
>
> GH #359 is complete. Root fix already landed on origin/main as ce122c320f and the issue was already closed. Added explicit GH-359 spec and regression traceability on pushed branch fix/gh359-land-existing-20261023, commit 928ed0d8657fe34fd062df159d9af50123da453f. Focused tests: 2 passed, 0 failed; git diff --check passed. Full make check was interrupted to free the slot and is not claimed as passed. Follow-up: merge/cherry-pick 928ed0d865, then normal CI.
<!-- agent-doc-subagent-completion key=bcd3ffef20dcbac6f65042829380fbdf20082374f90fd01677ee351d27d0dba6 -->

### Re: Subagent completed: `subagent:gh360-fix` returned work for queue item “#gh-fix https://github.com/btakita/agent-doc/issues/360” — agent-doc
<!-- agent-doc-subagent-response sha256=9427034415bc46b46a0de683a620181f52cb7fb54e4a79859ddf94008bb1acd4 -->
> **Subagent response:**
>
> GH #360 was already fixed, pushed, mainlined, and closed. Original branch fix/gh360-flat-evidence-20261022 at d33a37681f; mainline equivalent ce122c320f plus readiness diagnostics db1fef1ae6. Targeted regressions: 2 passed, 0 failed; workspace all-target check passed. No duplicate commit or close action. The guess occurred because tmux width plus an unordered visible-file set was treated as structural evidence; the fix only permits focusing an already-live pane, otherwise routing fails closed without changing layout.
<!-- agent-doc-subagent-completion key=9a312f137dccb0e94e418f760c557c58764866a568259ab8b4f95bb99f4a8e52 -->
> **Chat prompt (#chatprompt:51ec3bef4f1d):** Is the latest release published?

### Re: Latest release publication — gpt-5 · 2026-10-23T03:17-04:00

Yes. **v0.35.546** is published as a normal, non-draft, non-prerelease [GitHub release](https://github.com/btakita/agent-doc/releases/tag/v0.35.546), published at 2026-10-23 05:23:26 UTC. The installed CLI is also 0.35.546, and `origin/main` is tagged `v0.35.546` at `3a1d3cb8dc`.

The queued follow-up, v0.35.547, has not been published yet.

### Re: Subagent completed: `subagent:completion-exchange` returned work for queue item “"Re: Subagent completed:" should immediately add the response into agent:exchange.” — agent-doc
<!-- agent-doc-subagent-response sha256=0fc659c2f663ecd4a0aafeb182ac2bf04c7753be7357ba70fec7f146442115af -->
> **Subagent response:**
>
> Implemented and pushed on branch fix/completion-exchange-20261023 at 4da4cd8ad50aecf05c0206f170b4b74c9e6096e4. Six focused contract tests passed; all 11,444 Rust tests passed. The remaining full-check failure was environmental at jetbrains-262-check because /tmp was full. No install, release, version bump, or tag was performed.
<!-- agent-doc-subagent-completion key=0486ad9860e0dbd5d7d4fe16e88a5518ad8943bfd644fd7f3a935777ab386c8d -->

### Re: Subagent completed: `subagent:release-publish` returned work for queue item “Automate publication of all three compatibility-ranged JetBrains updates to Marketplace, with token/onboarding and exact-range verification, so Remote Dev synchronizes the modular ZIP to both peers and prevents one-sided installs.” — agent-doc
<!-- agent-doc-subagent-response sha256=a6b5558b3ce70fc65f9ed81373b17abf046edd257bef4aca46d19ca58f35def4 -->
> **Subagent response:**
>
> Already implemented on origin/main by 2e4cc6d7fb and 0e2df8590a; no duplicate change was made. JetBrains repository self-test and exact Marketplace workflow regression passed. External onboarding, token creation, workflow rerun, and approval remain.
<!-- agent-doc-subagent-completion key=cfc359fbc036c5c758dc8c21ef755c95b409e81ff2b10e94a2462dcd79399194 -->

### Re: Subagent completed: `subagent:release-runloop` returned work for queue item “release + publish: no `agent:run` + `agent:loop`” — agent-doc
<!-- agent-doc-subagent-response sha256=42521babcc171cd0d04515bbc4692cc0ee13619489e564d28a292eb3fcaac921 -->
> **Subagent response:**
>
> Already satisfied by published v0.35.546. Canonical agent:run and agent:loop are included via 2c70641826 and 67f4ef23a4. Focused tests and cargo check passed; no changes, commit, push, or duplicate release were made.
<!-- agent-doc-subagent-completion key=1047f4229b0d030081337df5bc218fa4c1457404a84ca952cb9ba28a3b37cf83 -->

### Re: Subagent completed: `subagent:release-publish` returned work for queue item “release + publish” — agent-doc
<!-- agent-doc-subagent-response sha256=4a2e5c617cbdecde7e86a5423b79b1ed1c2a0867d915480ce88b545f4181a68e -->
> **Subagent response:**
>
> No-op audit: v0.35.546 is already public and stable at 3a1d3cb8dc; GitHub release and PyPI publication succeeded. No files changed and no duplicate release was attempted. Homebrew remains at 0.35.541 because the token was absent; Marketplace publication is separate.
<!-- agent-doc-subagent-completion key=43bda87e7a70e49c4dc03479b3f9b2c8612b4f71e6f4b504c897c2f698ba375a -->

### Re: Subagent completed: `subagent:gh360-fix` returned work for queue item “How do we improve the reliability of the routing? Why did the guess occur in https://github.com/btakita/agent-doc/issues/360? On top of fail no-close routing, we need to figure out how to not get into the fail state.” — agent-doc
<!-- agent-doc-subagent-response sha256=e709218116be4adfb2ca629a4422872ffa33abe2cc567f5e28d6c91ec1d4c7cb -->
> **Subagent response:**
>
> Implemented and pushed on branch fix/gh360-routing-reliability-20261023 at 277c86effabca6c2ed15a425fb67831701c697c3. Flat evidence now routes to a proven parked live actor pane without structural publication; pane-less targets still fail closed. Focused Rust tests passed, Rust nextest passed 11,444/11,444, and the JetBrains suite passed 934/934 with the declared Lazily 0.46.0. The final corrected full rerun was interrupted for queue cleanup and is not claimed as passed.
<!-- agent-doc-subagent-completion key=ec5ef725bb242590f7f9a60f7ea0b403ccc83fce70116b7c4c29ab893e7b7f53 -->

### Re: Subagent completed: `subagent:followup-release-batch` returned work for queue item “release + publish: include completed follow-up fixes from the completion-feedback, Coder readiness, subagent-request formatting, and IDEA notification-deduplication investigations after v0.35.546” — agent-doc
<!-- agent-doc-subagent-response sha256=f9d02729a3bb1398924fbdd0872820881bb6a64b5239685f1e584d3aeeebbd00 -->
> **Subagent response:**
>
> Follow-up batch prepared and pushed on branch batch/followup-v035546-20261023-a283fe6e at 1627f533157b2171d5966679e314c106d17e4a86, containing completion-feedback commit 0bb2f44191 and IDEA notification-dedupe commit 1627f53315. Focused Rust tests passed 1,812/1,812; JetBrains test/build passed; workspace nextest passed 11,444/11,444. Full make check still requires the retained JetBrains package-generation/version bump and a rerun; no release, tag, version bump, or publication was performed.
<!-- agent-doc-subagent-completion key=b11dcc37a718740fd895ef6a856a15db6f2f300f67f78b146712fecb937ce1cb -->
> **Chat prompt (#chatprompt:0e145b378893):** Are subagents still running? The state of the queue items seems wrong

> **Chat prompt (#chatprompt:db9d9f1d7ab2):** Did we release the latest commits?

> **Chat prompt (#chatprompt:9acae9a5e377):** clean up the queue

> **Chat prompt (#chatprompt:6acc7d457160):** Have we fixed the causes of the queue getting out of sync?

> **Queue prompt:** #gh-issue https://github.com/btakita/agent-doc/issues/355

### Re: Queue state, release coverage, and cleanup — gpt-5 · 2026-10-23T03:43-04:00

No background subagents are still running. At the start of this audit, only two were genuinely active: the follow-up release batch and routing-reliability work. Four other rows incorrectly remained 🚧 after their workers had completed because their completion receipts had not reached the queue controller. Both real workers have now completed, the missing receipts were published, and all 33 main-queue rows are now verified ☑️. This closeout removes those 33 completed rows, including the replayed duplicate release item; the scheduled GitHub-issue watcher queue remains intact.

The latest commits are **not released yet**. Published `v0.35.546` and `origin/main` both remain at `3a1d3cb8dc`. Post-release work exists on branches, including:

- follow-up batch `1627f53315`, containing completion feedback and IDEA notification deduplication;
- routing reliability `277c86effa`;
- completion-exchange handoff `4da4cd8ad5`;
- Coder pane evidence `3f5d4f3f89`;
- upgrade checksum handling `59f7bc833e`;
- GH-359 traceability `928ed0d865`.

The queue desynchronization causes are only **partly released**:

- The dangerous ☑️→📥 regression—an unrelated edit causing fabricated cancellation tombstones—is fixed and released in v0.35.546 through the fail-closed provenance boundary.
- The stale 🚧 problem—workers finishing without immediately publishing a generation-fenced completion receipt—is fixed on `4da4cd8ad5` and included in the prepared follow-up batch as `0bb2f44191`, but it is not on `main` or in a release yet.
- Until that follow-up ships, the coordinator fallback remains necessary; it was used here to repair the current document before cleanup.

The leading GH-355 row was already resolved and its completion evidence is preserved in the exchange before removal.
> **Chat prompt (#chatprompt:0e145b378893):** Are subagents still running? The state of the queue items seems wrong

> **Chat prompt (#chatprompt:db9d9f1d7ab2):** Did we release the latest commits?

> **Chat prompt (#chatprompt:9acae9a5e377):** clean up the queue

> **Chat prompt (#chatprompt:6acc7d457160):** Have we fixed the causes of the queue getting out of sync?

> **Queue prompt:** #gh-issue https://github.com/btakita/agent-doc/issues/356

### Re: Cleanup continuation — gpt-5 · 2026-10-23T03:45-04:00

The preceding committed response already contains the full answers. This re-admitted cycle confirms the resulting state: no subagents remain running; all prior main-queue work has durable completion evidence; the latest post-v0.35.546 commits are not released; and the completion-receipt root-cause fix is prepared but not yet released. The completed rows are being struck while the newly arrived sdk-demo.md compact-exchange error remains queued as distinct work.

The GH-356 row was previously completed and its durable result is preserved in the exchange before removal.

### Re: Subagent completed: `subagent:gh355-cleanup-rerun` returned work for queue item “#gh-issue https://github.com/btakita/agent-doc/issues/355” — agent-doc
<!-- agent-doc-subagent-response sha256=c8257663b8133ee872ab873bbfbec24752aafd1f0ed6526e3aa89e027d206a9d -->
> **Subagent response:**
>
> Duplicate cleanup audit: GH #355 is already resolved by 4bb0b43868, released in v0.35.544, and present in current v0.35.546 main. Existing GH-355 and unknown-route regressions passed in the prior isolated verification. No duplicate change, commit, issue action, or release is required.
<!-- agent-doc-subagent-completion key=cfde8eab28a1bbb4adbb41d63161a5b97758a93f97eafb34100c0fe15f867113 -->
<!-- no-free-text-queue-head-guard -->
### Re: Intentional queue cleanup boundary — gpt-5 · 2026-10-23T03:47-04:00

The operator explicitly requested queue cleanup. The removed historical rows all had durable reported/completion evidence and were intentionally cleared after their results were summarized in the preceding response. They were not silently dropped. The fenced GH-355 claim/outbox mismatch and the newly dispatched sdk-demo.md compact-exchange defect remain in the queue.
<!-- agent:boundary:67a15c86 -->
<!-- /agent:exchange -->

<!--
-->

## Queue: Main
<!-- agent:queue id="main" subagents preset="#spec-test-build-install-commit-push" priority go -->
- ☑️ [#subagent:gh355-cleanup-rerun#3] #gh-issue https://github.com/btakita/agent-doc/issues/355
- 🚧 [#subagent:sdk-compact-replica#1] Compact Exchange on sdk-demo.md error: Agent Doc could not attach the open editor replica for sdk-demo.md. Cause: ambiguous-retained-projection. The editor buffer and controller canonical have diverged from their last settled ancestor. Copy any unsaved text, close this editor tab, run `agent-doc repair <file>` while it is detached, and reopen it. The editor buffer and disk were left unchanged.
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
