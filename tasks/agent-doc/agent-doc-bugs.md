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

📤 Queue handoff: 20 reported items awaiting coordinator integration, verification, and closeout.
Main is at `b6b9af6e15` and agent-doc 0.35.532 is installed locally. Release v0.35.532 was published from `71f9801b49`. Post-release main now includes the inline-code-safe, occurrence-ordered queue preset authorization fix for GitHub issue #331 and the positive-evidence-only Remote Dev pane-order fix for GitHub issue #332; both issues are closed, the integrated full suite passed, and the current mainline was installed locally.

<!-- status:sync -->
✅ Release v0.35.547 was published from `9c2375a33c` and verified end to end. The stable, non-draft GitHub release has 21 assets; all 11 release jobs passed, the PyPI build/publish/clean-install verification passed, and issue #367 is closed. The local CLI reports 0.35.547; expected plugins are JetBrains 0.2.801, VS Code 0.2.83, and Zed 0.1.1. JetBrains Marketplace publication remains deferred because its token is not configured; the verified ZIPs and feeds are present in the GitHub release.

The pushed post-release integration branch `integrate/post-547-config-repair-20261023` is at `220aaa6b14`. Its combined authoritative `make check` passed and the coordinator-owned `make install` completed. It adds proof-gated automatic repair for torn queue owner/lifecycle projections (`0c2503e2ff`) and project-level agent selection (`220aaa6b14`). Set `.agent-doc/config.toml` to `default_agent = "claude"`; precedence is CLI `--agent`, document frontmatter `agent:`, project default, global default, then Claude fallback.

The live queue cleanup is committed through this closeout. The attached controller deferred its native-library reload while this cycle was active; it will adopt the installed generation at the idle boundary and ordinary projection will canonicalize the malformed queue row.
<!-- /status:sync -->
<!-- /agent:status -->
 

## Exchange

<!-- agent:exchange patch=append subagents -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20261023-152449.md`*

Compacted content:
- Archived 53 response topic(s): Release v0.35.544 published and release workflow hardened; Continuation after release closeout; recovered response; 50 more
- Prior summary/context: prior compacted content: Archived 15 response topic(s): recovered response; Subagent completed: `subagent:unique_requ...

### Re: Release v0.35.547, queue repair, and project agent configuration — gpt-5 · 2026-10-23T13:28:38-04:00

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/367 ⟦effect:1⟧

Issue #367 is fixed and closed. The ingress retry implementation and regression test were already in the integration line; the state-backbone invariant was added, `make jetbrains-262-check` passed, and the complete batch shipped in v0.35.547.

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/367 ⟦effect:2⟧

The duplicate follow-up was verified as a no-op. Issue #367 is closed, and both remote `main` and `v0.35.547` resolve to release commit `9c2375a33c`, which contains the fix, regression test, and spec.

> **Queue prompt:** Several of this queue's items were in a ❔ state. Now the incorrect 📥 state. They should be in the ☑️ state. Fix agent-doc causes.

Fixed the first queue-state cause in `495ef865fb`: structurally attested subagent completion cells are excluded from answer/deferral/preset matching, so nested worker queue-prompt echoes cannot strike a reported row or release its claim. Queue and preflight tests, the workspace build, and `make check` passed. The fix shipped in v0.35.547.

> **Queue prompt:** release + publish: include issue 367 + queue state fixes

Release v0.35.547 is complete. Commit `9c2375a33c` is on `main` and tagged; all 11 GitHub release jobs passed; all 21 platform, checksum, and editor assets are public; PyPI build, publication, and clean-install verification passed; and the local full install completed. The release is stable, non-draft, and non-prerelease: https://github.com/btakita/agent-doc/releases/tag/v0.35.547

> **Queue prompt:** do [#coderinstancedefaulting]

Implemented project-level agent selection in `a1448b7977`, integrated as `220aaa6b14`, and pushed. Configure the coder instance in `.agent-doc/config.toml`:

```toml
default_agent = "claude"
```

`.agent-doc/project.md` remains prompt-preset scope. Resolution order is CLI `--agent` → document frontmatter `agent:` → project `default_agent` → global `default_agent` → Claude fallback. Focused tests passed; the worker full check passed 11,453/11,453 nextest tests; the combined integration check and local install also passed.

> **Queue prompt:** agent-doc should have an auto-recover + repair to clean up the queue. Clean up this queue.

Implemented proof-gated queue auto-repair in `6c263383e9`, integrated as `0c2503e2ff`, and pushed. It removes a torn duplicate owner/lifecycle prefix only when valid observed or durable owner-generation evidence proves the repair; unproved lookalikes remain operator text. Targeted document/queue tests and both worker and combined full checks passed. The new CLI/library is installed; the attached controller will reload at this cycle's idle boundary and ordinary projection will canonicalize the malformed row.

> **Queue prompt:** What is `⟦effect:1⟧`? Why is `⟦` and `⟧` used?

`⟦effect:1⟧` is a stable binary-owned occurrence ID for duplicated free-text queue rows. It distinguishes the first identical occurrence from `effect:2`, so claims, completion receipts, and closeout affect the intended row. It is not priority, severity, or a side-effect count. The white square brackets visibly delimit reserved machine metadata; only the exact trailing ` ⟦effect:<positive integer>⟧` form is parsed. The visual-delimiter rationale is an implementation inference because the specific glyph choice is not separately documented.

### Re: Release availability and upgrades — gpt-5 · 2026-10-23T13:28:38-04:00

> **Chat prompt (#chatprompt:55ddfa643edb):** how is the release looking?

The release completed successfully. GitHub publication, every platform build, plugin packaging, local full installation, PyPI publication, and PyPI clean-install verification passed.

> **Chat prompt (#chatprompt:72dbe11cfcd7):** Is the linux + plugins available?

Yes. Both GNU Linux archives, the x86_64 musl archive, the VS Code VSIX, and all three JetBrains variants are available in the public v0.35.547 release.

> **Chat prompt (#chatprompt:f818f78968f8):** Is the coder instance using x86_64 or x86_64-unknown-linux-musl?

This coder instance is `x86_64-unknown-linux-gnu`: it reports x86_64, glibc 2.44, and a dynamically linked GNU/Linux executable. The GNU x86_64 archive is the correct artifact.

> **Chat prompt (#chatprompt:2fa5ba1255e5):** so I can upgrade the coder instance now?

Yes. The required GNU Linux artifact and plugins are public; there is no need to wait for musl on this glibc instance.

> **Chat prompt (#chatprompt:ff666cb7ea6f):** Can I set `agent: claude` in the project.md file?

No. Use `.agent-doc/config.toml` with `default_agent = "claude"`; `.agent-doc/project.md` is for project prompt presets.

> **Chat prompt (#chatprompt:f132a630e58b):** for the coder instance?

Yes—the `.agent-doc/config.toml` project default applies to the coder instance. A CLI flag or document frontmatter can override it for a particular run/document.

> **Chat prompt (#chatprompt:7550aebc76f2):** Is the cli on 0.35.546 and plugin on 0.2.800? Coder saw that

Those are stale remote-instance versions. This machine reports CLI 0.35.547, JetBrains classic 0.2.801, JetBrains 262/Rider 0.2.804, and VSCodium 0.2.83. A remote coder instance must run its own upgrade and reload/restart if its live plugin generation cannot hot-reload.

> **Chat prompt (#chatprompt:388af1582650):** Does agent-doc cli have an upgrade command?

Yes: `agent-doc upgrade` performs a one-shot stable-release upgrade. `agent-doc upgrade --auto` watches for stable releases, polling every 15 minutes by default.

> **Chat prompt (#chatprompt:5f53f2d52e36):** Would the upgrade work for the cli + any installed plugins?

Yes. The upgrade updates the CLI and reconciles every already-installed agent-doc editor plugin, even when the CLI is already current. It does not install plugins for editors that were never installed, and it fails visibly if an installed plugin cannot be updated.

### Re: Subagent completed: `subagent:queue-state-live-repair` returned work for queue item “Several of this queue's items were in a ❔ state. Now the incorrect 📥 state. They should be in the ☑️ state. Fix agent-doc causes.” — agent-doc
<!-- agent-doc-subagent-response sha256=f83e2adbceb900af1cea75a8987a778b76b877d7039b15cb3d8e312eb089b47d -->
> **Subagent response:**
>
> Queue-state live repair audit complete. No additional code change is needed beyond the already integrated commits `0c2503e2ff` (proof-gated row-body repair) and `ff0c322c8a` (projection-guard identity repair). The exact controller regression `claim_controller_repairs_torn_duplicate_owner_before_projection_guard` proves the actual torn `[#owner#3📥...` row is canonicalized and the next sdk-demo row can be claimed; unproved lookalikes remain operator text. Verification: `cargo test -p agent-doc-document -p agent-doc-queue -p agent-doc-queue-io` passed, exit 0/no failures; worktree clean. The coordinator combined `make check` after `ff0c322c8a` passed and install completed. No additional commit/push and no follow-up queue items.
<!-- agent-doc-subagent-completion key=0e31615a199d4231b84cad51354b2384a71993b0c1c968788957f169d962a632 -->

### Re: Subagent completed: `subagent:sdk-demo-compact-attach` returned work for queue item “Compact Exchange on sdk-demo.md error: Agent Doc could not attach the open editor replica for sdk-demo.md. Cause: ambiguous-retained-projection. The editor buffer and controller canonical have diverged from their last settled ancestor. Copy any unsaved text, close this editor tab, run `agent-doc repair <file>` while it is detached, and reopen it. The editor buffer and disk were left unchanged.” — agent-doc
<!-- agent-doc-subagent-response sha256=b6614c980a6329a6e2b16d4a493a45c44c4bfbaa5baf48eed8a7ecab0ea313c7 -->
> **Subagent response:**
>
> This queue item is a stale duplicate of work already integrated in current main/release. Exact fix: `d1424feed1 fix(jetbrains): pause ambiguous attach retries` is an ancestor of `origin/main` at `9c2375a33c` and shipped in v0.35.547. It includes spec/runbook updates plus Rust/Kotlin/parity regressions for `await_operator_evidence`, no timer retry, refresh suppression, and safe authority release on closing the refused unattached editor. Fresh validation at release head: `agent-doc-editor-surface` 160 tests passed (158 unit + 2 doc), focused JetBrains classes 41 passed (9 AttachFailureDiagnosis + 32 RetainedCanonicalOperatorText), zero failures/skips. Worktree clean; no duplicate edit/commit/push and no follow-up queue heads.
<!-- agent-doc-subagent-completion key=a09c440ba7ddd7583b549191dff03c3b9229c0f7a9eec00d4e333ad855c02569 -->

### Re: v0.35.548 release and current upgrade availability

> **Chat prompt (#chatprompt:64f960b49289):** Can I restart Idea?

Yes. Save unsaved editor text first; restarting IDEA is safe and is the correct way to load a plugin generation when dynamic reload is unavailable.

> **Chat prompt (#chatprompt:8758fc3e1d4c):** Can I install the new version in my Coder?

Yes. v0.35.548 is now fully stable and public. The Coder host is x86_64 GNU/glibc, so its correct artifact is x86_64-unknown-linux-gnu.

> **Chat prompt (#chatprompt:ed2288d8237e):** I thought we allowed per environments to be updated

The release pipeline published platform artifacts independently, but the installed v0.35.548 upgrader still selected GitHub's globally stable release. That was the gap: per-environment artifact readiness existed, but the CLI had no per-target stable pointer.

> **Chat prompt (#chatprompt:e76a3286629a):** does agent-doc upgrade not support this?

v0.35.548 does not target an independently ready prerelease artifact. Its upgrade command updates the CLI and reconciles installed plugins, but it resolves the globally stable release. The per-target resolver is now implemented on main for the next release.

> **Chat prompt (#chatprompt:bf3e495313af):** Can we make the stable demarcation be per target or would that be problematic?

Yes. It is safe when the target pointer is checksummed, coupled to the exact editor-package manifest, serialized, fail-closed during pointer replacement, rollback-fenced, and backward-compatible with the global stable channel for old clients.

> **Chat prompt (#chatprompt:9fe2fcecbe2e):** implement in a subagent. Make sure this works.

Implemented by a subagent, reviewed, integrated as `9e366edd73`, and pushed to `main`. The new rolling channel publishes a checksummed manifest per target, distinguishes Linux GNU from musl, promotes a target as soon as both its archive and all JetBrains compatibility packages are ready, revalidates the pointer and archive before installation, refuses unpinned fallback, and provides an explicit verified rollback workflow. Windows readiness metadata is published, but Windows CLI self-upgrade intentionally remains on the legacy/manual path until a detached executable-replacement helper exists. Existing clients retain the global latest plus aggregate SHA256SUMS behavior.

Validation passed on the exact implementation: publisher self-test, Python 3.8 compatibility, Actionlint/ShellCheck, 27 upgrade tests, release workflow contract test, `make check-fast`, and the complete `make check`. The integration checkout repeated the focused tests after cherry-pick. Remote `main` resolves to `9e366edd73a85b7e299eeeae95b8a90f7add5c69`.

This feature is on main but is not part of v0.35.548; installations begin using it after installing a future release that contains the resolver.

> **Queue prompt:** release + publish: include issues 368-371

Release v0.35.548 is complete. GitHub release, all six platform builds, aggregate and per-archive checksums, all editor packages, Homebrew, PyPI publication, and PyPI clean-install verification passed. The public release is stable, non-draft, and non-prerelease: https://github.com/btakita/agent-doc/releases/tag/v0.35.548

### Re: agent run/loop, claim scaffolds, and queue steering

> **Chat prompt (#chatprompt:464a02888085):** Does agent-doc support agent:run and agent:loop?

Yes. Both components are supported: `agent:run` is the one-shot run surface and `agent:loop` is the looping session surface.

> **Chat prompt (#chatprompt:055f31f59dcf):** subagent: The claim template should use agent:run instead of agent:queue.

Fixed and shipped in v0.35.548. Newly scaffolded claim templates emit `agent:run`; existing legacy `agent:queue` documents remain compatible.

> **Chat prompt (#chatprompt:16da2011a0cd):** subagent: agent-doc should treat queue mutations as steering. It should not unregister any items that are edited.

Fixed and shipped in v0.35.548. Editing a claimed queue row preserves its owner and generation and forwards the changed text as steering instead of unregistering or redispatching the item. Regression coverage includes full prose rewrites, tracked-ID changes, distant additions, and false-pair avoidance.

> **Queue prompt:** grok does not seem to run subagents in the queue. Make grok use subagents when `subagents` attribute is set. See the sdk-demo.md session. I had to manually prompt Grok to run subagents for each queue item.

Fixed and shipped in v0.35.548. Grok now uses native background subagents for queue dispatch when `subagents`/fan-out is configured, then generation-fenced starts the exact claims without requiring manual prompts.

### Re: Ambiguous retained projection repair verification

> **Queue prompt:** fix: Agent Doc could not attach the open editor replica for agent-doc-bugs.md. Cause: ambiguous-retained-projection. The editor buffer and controller canonical have diverged from their last settled ancestor. Copy any unsaved text, close this editor tab, run `agent-doc repair <file>` while it is detached, and reopen it. The editor buffer and disk were left unchanged.

No new code is needed; this was a stale duplicate incident class. Existing fix `d1424feed1 fix(jetbrains): pause ambiguous attach retries` is an ancestor of current `main` at `9e366edd73`. It converts ambiguous retained/reregister projection into an operator-evidence hold, suppresses unsafe retry/refresh behavior, and safely releases authority when the refused editor closes.

Fresh current-main validation passed: `agent-doc-editor-surface` completed with zero failures, `AttachFailureDiagnosisTest` passed 9/9, and `RetainedCanonicalOperatorTextTest` passed 32/32. The isolated worktree is clean; no duplicate commit or follow-up queue item was created.
<!-- agent:boundary:1d86f43f -->
<!-- /agent:exchange -->

<!--
-->

## Queue: Main
<!-- agent:queue id="main" subagents preset="#spec-test-build-install-commit-push" priority go -->
- ☑️ [#subagent:coder-instance-defaulting#1] do [#coderinstancedefaulting]
- ☑️ [#subagent:gh363-fix#1] do [#gh363]
- ☑️ [#subagent:gh355-cleanup-rerun#3] #gh-issue https://github.com/btakita/agent-doc/issues/355
- ☑️ [#subagent:sdk-demo-compact-attach#3] Compact Exchange on sdk-demo.md error: Agent Doc could not attach the open editor replica for sdk-demo.md. Cause: ambiguous-retained-projection. The editor buffer and controller canonical have diverged from their last settled ancestor. Copy any unsaved text, close this editor tab, run `agent-doc repair <file>` while it is detached, and reopen it. The editor buffer and disk were left unchanged.
- 📥 agent-doc process is thrashing in the background along with Idea.
- 📥 Investigate the pre-existing JetBrains LazilyGenerationFenceTest stale-disposal failure on untouched main after the lazily-kt 0.46.0 update.
- 📥 Compact Exchange in sdk-demo.md compacted but did not complete the cycle. There is uncommitted compact summary content in the exchange.
- 📥 #gh-fix https://github.com/btakita/agent-doc/issues/364
- 📥 #gh-fix https://github.com/btakita/agent-doc/issues/365
- 📥 #gh-fix https://github.com/btakita/agent-doc/issues/366
- 📥 Coder instance still has backend/frontend communication issues
- ☑️ [#subagent:gh367-fix#1] #gh-fix https://github.com/btakita/agent-doc/issues/367 ⟦effect:1⟧
- ☑️ [#subagent:gh367-followup#1] #gh-fix https://github.com/btakita/agent-doc/issues/367 ⟦effect:2⟧
- ☑️ [#subagent:queue-state-live-repair#4] Several of this queue's items were in a ❔ state. Now the incorrect 📥 state. They should be in the ☑️ state. Fix agent-doc causes.
- ☑️ [#subagent:release-367-queue-state#1] release + publish: include issue 367 + queue state fixes
- ☑️ [#subagent:queue-auto-repair#1] agent-doc should have an auto-recover + repair to clean up the queue. Clean up this queue.
- ☑️ [#subagent:effect-annotation-explain#1] What is `⟦effect:1⟧`? Why is `⟦` and `⟧` used?
- ☑️ [#subagent:project-md-frontmatter#1] Allow project.md to configure all frontmatter including `agent: ...`. This would have override precedence over config.toml. Allow changes while the project is open to both config.toml and project.md.
- ☑️ [#subagent:gh368-fix#1] #gh-fix https://github.com/btakita/agent-doc/issues/368
- ☑️ [#subagent:gh369-fix#1] #gh-fix https://github.com/btakita/agent-doc/issues/369
- ☑️ [#subagent:gh370-fix#1] #gh-fix https://github.com/btakita/agent-doc/issues/370
- ☑️ [#subagent:gh371-fix#1] #gh-fix https://github.com/btakita/agent-doc/issues/371
- ☑️ [#subagent:agent-doc-bugs-attach-repair#1] fix: Agent Doc could not attach the open editor replica for agent-doc-bugs.md. Cause: ambiguous-retained-projection. The editor buffer and controller canonical have diverged from their last settled ancestor. Copy any unsaved text, close this editor tab, run `agent-doc repair <file>` while it is detached, and reopen it. The editor buffer and disk were left unchanged.
- ☑️ [#subagent:sdk-demo-reroute-recovery#1] In sdk-demo.md, I manually interrupted the Grok turn, ran `/clear`, then attempted to Run Agent Doc from the editor: [route] active closeout for /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk/sdk-demo.md could not be drained before reroute; nothing was dispatched and existing queue head "Unbind the \"s\" key to toggle \"Network Stats\". We are typing into chat and everytime \"s\" is pressed, the Network Stats is toggled." remains queued behind the closeout (blocker: closeout recovery replay_safe [open_empty_preflight]: `agent-doc session cancel-turn /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk/sdk-demo.md` — first interrupt the owning harness run; the cancellation receipt may then abandon its empty diagnostic preflight without document drift) ui_outcome_contract=ui-outcome-v1 ui_outcome=blocked_with_exact_unblocker ui_outcome_class=blocked next_action=follow_unblocker unblocker=run_recovery_command recovery_command=agent-doc session cancel-turn /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk/sdk-demo.md
---
☑️ [#subagent:in-turn-upgrade-answer#1] - From sdk-demo.md: Can we make agent-doc allow in-turn upgrades with the new contract? Would that require the agent to restart? Answer only: ```
❯ agent-doc tasks/sdk/sdk-demo.md                                                                      2:09 PM


I'll open the agent-doc session for tasks/sdk/sdk-demo.md and follow the skill before answering.       2:09 PM

◈ Explored 4 files, 1 search, called 3 tools, ran 1 command

Admission is in. The binary is newer than the local skill, and the plan says this sealed turn still    2:09 PM
has a contract to run. I'll update the skill and pull the queue and backlog. 
- ```
---
- ☑️ [#subagent:release-368-370#1] release + publish: include issues 368-371
- ☑️ [#subagent:grok-queue-subagents#1] grok does not seem to run subagents in the queue. Make grok use subagents when `subagents` attribute is set. See the sdk-demo.md session. I had to manually prompt Grok to run subagents for each queue item.
<!-- /agent:queue -->

## Queue: Issues

<!-- agent:queue id=issues loop interval=1h -->
- ☑️ [#subagent:gh363-fix#1] do [#gh363]
<!-- /agent:queue -->

## Todo
<!-- agent:todo -->
- [ ] [#ahfe] Live-check GH #267 on the 263 IDE: after `agent-doc upgrade` WITHOUT restart, once idea.log shows `[native] hot-reloaded libagent_doc`, Run Agent Doc should complete (ops.log `editor_route_superseded_editor_admitted`, route output ends with the advisory line). 2026-10-15 operator: did not work (believed); watch for bug reports before reopening GH #267.
- [ ] [#ctg7] Under heavy load, switch the editor 2 panes -> 1 -> 2 and confirm tmux follows and stays at 2 (stale-controller fix).
- [ ] [#coder-remote-install-02805] Install agent-doc-jetbrains-263 0.2.805 on both the Coder Client or Gateway and backend, then restart both peers and verify paired ingress deferred or accepted and completed or connected diagnostics.
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
