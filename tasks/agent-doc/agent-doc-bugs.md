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

Main is at `b6b9af6e15` and agent-doc 0.35.532 is installed locally. Release v0.35.532 was published from `71f9801b49`. Post-release main now includes the inline-code-safe, occurrence-ordered queue preset authorization fix for GitHub issue #331 and the positive-evidence-only Remote Dev pane-order fix for GitHub issue #332; both issues are closed, the integrated full suite passed, and the current mainline was installed locally.

<!-- status:sync -->
✅ Release v0.35.548 was published from `56046b3880` and verified end to end. The stable, non-draft GitHub release has 21 assets; all 11 release jobs passed, including six platform builds, plugins, Homebrew, PyPI, and finalization. Issues #368–#371 are closed.

Remote `main` is at `9e366edd73`. Its post-release per-target stable upgrade-channel implementation is pushed, and CI, Book, and Pages workflows all passed. The feature is not part of v0.35.548; it will activate after a future release containing the resolver.

This host reports agent-doc CLI 0.35.548. Installed editor packages are IntelliJ classic 0.2.806, Rider/262 0.2.807, and VSCodium 0.2.83. The v0.35.548 release also publishes JetBrains 263 package 0.2.808; it is not installed on this host.
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

### Re: recovered response

Implemented and pushed to `main` at `9e366edd73`.

It adds:

- Independent stable channels for GNU Linux, musl Linux, and macOS targets.
- Early promotion once the target archive and compatible plugins are ready.
- Checksummed manifests, editor-package coupling, TOCTOU protection, and explicit rollback.
- Backward-compatible global stable behavior for older clients.
- Windows readiness metadata; Windows CLI self-upgrade remains manual pending safe executable replacement.

Validation passed: full `make check`, `make check-fast`, Actionlint/ShellCheck, publisher self-test, 27 upgrade tests, and integration tests.

v0.35.548 is fully released, but this feature landed afterward. Your Coder can upgrade to 0.35.548 now; per-target upgrading will activate after the next release containing this resolver. Main CI is still running at handoff.
> **Queue prompt:** Update sync:status

### Re: Update sync:status — gpt-5 · 2026-10-23T18:12-04:00

The synchronized status now reflects the current verified state:

- v0.35.548 is the stable public release from `56046b3880`; all 11 release jobs and 21 assets passed, and issues #368–#371 are closed.
- Remote `main` is `9e366edd73`; its post-release per-target upgrade-channel work passed CI, Book, and Pages, but is not included in v0.35.548.
- This host runs CLI 0.35.548 with IntelliJ classic 0.2.806, Rider/262 0.2.807, and VSCodium 0.2.83. JetBrains 263 package 0.2.808 is published but not installed here.

This was a status-only audit: no repository files changed, so no code tests, build, install, commit, or push were required. Release and main CI were green. The exchange remains above its accretion warning threshold; it was not compacted because the queue was active and auto-compaction was not enabled.

### Re: do [#upgradeideatest] — gpt-5 · 2026-10-23T18:14-04:00

> **Operator question:** Should we upgrade IDEA to test the frontend + backend install?

Yes. Upgrade the test environment to IDEA 263 and install the matching agent-doc JetBrains 263 package, v0.2.808, on both the frontend Client/Gateway and the backend. Restart both peers before testing so the result proves the paired install path rather than a mixed or stale plugin generation.

Then run Agent Doc and verify that paired ingress is deferred or accepted as expected and reaches a completed or connected diagnostic state. This is a manual environment verification, so no repository code, build, or test changes were appropriate in this answer-only cycle. The existing operator TODO now names v0.2.808.

The exchange remains above its accretion warning threshold; it was not compacted because the queue is active and auto-compaction is not enabled.

### Re: do [#latestbetadownload] — gpt-5 · 2026-10-23T18:18-04:00

> **Operator question:** What is the latest beta download URL?

The latest IntelliJ IDEA Ultimate beta is 2026.3 Beta, build `263.7307.44` (2026-10-23).

For this Linux x86_64 environment, download it directly from JetBrains:

https://download.jetbrains.com/idea/idea-263.7307.44.tar.gz

SHA-256 file:

https://download.jetbrains.com/idea/idea-263.7307.44.tar.gz.sha256

JetBrains' current release metadata is available at:

https://data.services.jetbrains.com/products/releases?code=IIU&latest=true&type=eap

The canonical artifact URL was checked and currently redirects successfully to JetBrains' download CDN. The exchange remains above its accretion warning threshold; it was not compacted because the queue is active and auto-compaction is not enabled.

### Re: do [#upgradedidealatest] — gpt-5 · 2026-10-23T18:43-04:00

> **Operator question:** I upgraded IDEA to the latest release version. Is this architecture different from the Beta? Does this version have the frontend + backend? Or is that something to do with Toolbox + Coder?

The stable and beta builds use the same fundamental Remote Development architecture, but they are different IntelliJ platform branches:

- Latest stable: IntelliJ IDEA 2026.2.3, build `262.10968.63`.
- Latest beta: IntelliJ IDEA 2026.3 Beta, build `263.7307.44`.

Installing stable IDEA locally does not itself create a frontend/backend pair. That split appears when using JetBrains Remote Development:

- Coder provisions or exposes the remote development environment.
- A remote IDEA backend runs beside the project and source code.
- JetBrains Client is the local thin frontend.
- JetBrains Gateway establishes and manages the remote connection.
- Toolbox can install or launch the JetBrains products, but Toolbox alone is not what creates the split architecture.

Plugin compatibility follows the platform build branch. A plugin package targeting build 263 is expected to be rejected by stable IDEA 2026.2.x on build 262. For the stable release, use the agent-doc 262-compatible package; reserve the 263 package for the 2026.3 beta. That version mismatch likely explains the incompatible-plugin message, while the separate queued diagnostic can verify the exact package currently installed on each peer.

Official references:

- JetBrains stable release metadata: https://data.services.jetbrains.com/products/releases?code=IIU&latest=true&type=release
- JetBrains beta release metadata: https://data.services.jetbrains.com/products/releases?code=IIU&latest=true&type=eap
- JetBrains Remote Development overview: https://www.jetbrains.com/help/idea/remote-development-overview.html

The exchange remains above its accretion warning threshold; it was not compacted because the queue is active and auto-compaction is not enabled.
<!-- agent:boundary:f9beebe9:agent-doc-bugs -->
<!-- /agent:exchange -->

<!--
-->

## Queue: Main
<!-- agent:queue id="main" subagents preset="#spec-test-build-install-commit-push" priority go -->
<!-- /agent:queue -->

## Queue: Issues

<!-- agent:queue id=issues loop interval=1h priority -->
- ~~do [#upgradedidealatest]~~
- 📥 do [#helpregisterjetbrains]
- 📥 After the idea plugin, the agent-doc Plugin formatting stopped working and the commands don't show up. The plugin manager says the Agent Doc plugin is incompatible.
<!-- /agent:queue -->

## Todo
<!-- agent:todo -->
- [ ] [#ahfe] Live-check GH #267 on the 263 IDE: after `agent-doc upgrade` WITHOUT restart, once idea.log shows `[native] hot-reloaded libagent_doc`, Run Agent Doc should complete (ops.log `editor_route_superseded_editor_admitted`, route output ends with the advisory line). 2026-10-15 operator: did not work (believed); watch for bug reports before reopening GH #267.
- [ ] [#ctg7] Under heavy load, switch the editor 2 panes -> 1 -> 2 and confirm tmux follows and stays at 2 (stale-controller fix).
- [ ] [#coder-remote-install-02805] Install agent-doc-jetbrains-263 0.2.808 on both the Coder Client or Gateway and backend, then restart both peers and verify paired ingress deferred or accepted and completed or connected diagnostics.
<!-- /agent:todo -->

## Backlog
<!-- agent:backlog priority queue -->
- [ ] [#helpregisterjetbrains] Help me register the Jetbrains plugin?
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
