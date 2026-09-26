---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: 3d56a2a6-95ce-42c4-b71c-66c102e54fb4
  codex: 01a0db52-4c0d-72d3-b2c9-0ac89aff42a0
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#actionable-review': Add actionable review items into backlog + queue
  '#agent-doc-causes': Fix agent-doc causes
  '#self': agent-doc-bugs.md
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: go
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->
 

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20260926-012634.md`*

Compacted content:
- Archived 28 response topic(s): actionable operator verification belongs in Backlog; cross-document backlog item moves; fpe.md terminal commit convergence; 25 more
- Prior summary/context: prior compacted content: Archived 8 response topic(s): recovered response; frontend recovery; Anonymous queue identit...

### Re: #advance-review, tmux pane auto-focus, and queue edit convergence — gpt-5 · 2026-09-25T21:55:57-04:00

> **Queue prompt:** #advance-review

Both regressions are fixed, tested, installed, committed, and pushed.

- **Tmux pane auto-focus:** IntelliJ was checking `currentWindow.selectedFile` on a generic next EDT turn, before real split/tab focus transfer had necessarily completed. Both tab-selection and editor-tree click ingress now wait for `IdeFocusManager.doWhenFocusSettlesDown`, then re-check the active split under the existing generation fence before publishing focus. The controller remains the sole pane-selection policy owner.
- **Queue edit duplication:** the progressive edits `Update 113` → `Rebase +` → `Rebase + update 113` were being assigned three text-derived identities while the merge base lagged. Queue reconciliation now treats the live operator free-text run as authoritative only within the same shared-anchor gap. Durable `#id` items and id-less additions in different gaps remain independent. The causally gated raced-projection repair also recognizes the non-prefix replace-then-combine shape only when the final spelling contains every earlier draft.
- **Live document safety:** I did not overwrite `infra.md`; its owning session advanced independently, and its current queue no longer contains the three intermediate drafts.

Architecture contract: the invariant is “the newest active IntelliJ session selection focuses its exact route-owned pane after IDE focus settles; stale, inactive, disposed, and non-session observations emit no focus.” The transition rows are generation-newer vs stale, active-split vs background-split, visible-owner vs stashed/missing-owner, and final operator wording vs stale id-less drafts. The reactive topology remains editor event Source → focus-settled, generation-fenced observation → controller Computed policy → tmux Effect → receipt Source. The only one-shot action was the diagnostic direct-focus probe; no polling loop, retry owner, or second pane policy was added.

Verification:

- Targeted JetBrains focus tests passed.
- Targeted CRDT, document-cell, and progressive queue-repair tests passed.
- Full `make check` passed.
- `make install` installed agent-doc 0.35.418 and dynamically upgraded the live JetBrains process to plugin 0.2.420; `/proc/3129637/map_files` confirms the 0.2.420 jar is mapped. No IDE restart is required.
- agent-doc commit `c4b62e733` and superproject pointer commit `44a8ae1df` were created; the agent-doc commit is pushed.

The remaining `#tmuxfocussyncverify` item is narrowed to one operator-visible check: click between the two live IntelliJ session tabs/splits and confirm tmux follows each exact document.

### Re: recovered response

Fixed and deployed.

- Right editor focus now selects tmux `%218`; `%215` is inactive.
- Passive layout publications log `focus_not_requested`, so they can no longer steal focus.
- Stop-hook retained-delivery and `write_applied` states remain on binary-owned recovery instead of falling through to “already continued once.”
- The original FPE retained cycle committed successfully; its current interruption is a newer `preflight_started` cycle.
- Full `make check`: 9,758 tests passed.
- JetBrains plugin 0.2.421 is loaded live.
- Pushed:
  - `agent-doc`: `22479d31f`
  - `agent-loop`: `853abdb79`
<!-- agent:boundary:fe1c0161:agent-doc-bugs -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
- 🚧 #advance-review
<!-- /agent:queue -->


<!-- agent:backlog -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#cargovodtrim] [operator-verify] [P2][security] Trim the credential exposure from the livestream VOD once the operator supplies the VOD URL or platform/start-time locator. Known exposed interval: 2026-09-12T02:57:27.170Z through approximately 02:58:14Z.
- [/] [#pypiquotafix] [operator-auth] [P1] AUTHORIZED cleanup: after 0.35.383 thin wheel is live, sign into https://pypi.org/manage/project/agent-doc/releases/ in Chromium tab 0; then permanently delete the 34 oldest existing releases, 0.35.0 through 0.35.112 (all pre-2026-08-02), freeing about 1.75 GiB. Pinned PyPI installs of deleted versions will stop working. Browser is currently logged out; support request pypi/support#12214 remains open.
- [/] [#tmuxfocussyncverify] [operator-verify] JetBrains plugin 0.2.420 is dynamically loaded in live IDEA PID 3129637. Select agent-doc-bugs.md and a second visible cross-root session document in IntelliJ and confirm each selection focuses its exact tmux pane. Automated fix c4b62e733 waits for IntelliJ focus-manager settlement before publishing the generation-fenced selected document; direct controller focus to pane %215 succeeded. No restart is required; only the live tab/split click eyeball remains. Plan: tasks/agent-doc/plan-tmux-auto-sync-reactive.md.
- [/] [#grokmcpverify] [operator-verify] Verify an authenticated Grok Build MCP admit/read/plan/finalize cycle and same-turn auto-queue continuation once the free usage quota renews or the planned subscription is active. Grok CLI authorization, native JSON/backend commit fixtures, pane detection and quit behavior passed; the live MCP probe stopped at the usage limit before committed closeout. Installed v0.35.359; Heavy is not required. Plan: tasks/agent-doc/plan-grok-build-support.md.
- [/] [#activateinstalledjetbrai] [operator-verify] One IntelliJ restart activates the staged JetBrains plugin. PREMISE RE-MEASURED 2026-09-20T17:05-04:00: live IDEA is PID 1023909 started 2026-09-20 15:44:32; staged jar written 2026-09-20 16:42:24. The recorded PID 1645748 and the 2026-09-18 16:39:05 write are both wrong, and the PID 306200 asserted earlier in this document does not exist at all. Proof of non-activation is byte identity, not version: /proc/1023909/map_files shows agent-doc-jetbrains-0.2.388.jar (deleted) against on-disk inode 76585058, with the same 0.2.388 version string on both. Remaining splice proof unchanged: captured local edits recover across an independently advanced canonical response in a disposable live editor document. Original retained capture committed as 1b35dcb028 and all automated regressions passed. Plan: tasks/agent-doc/plan-captured-editor-splice-recovery.md.
- [/] [#harnessswitchverify] [operator-verify] Remaining proof: in a disposable live IntelliJ session, make an unsaved codex -> claude -> codex frontmatter switch while observing supervisor receipts; prove one spawn per authoritative switch, deferred restart for non-authoritative evidence, and no refusal or respawn storm. PREMISE RE-MEASURED 2026-09-20T17:05-04:00: live IDEA is PID 1023909 started 2026-09-20 15:44:32, so the previously recorded live PID 1645748 is gone as well as the older 1035669; the staged plugin jar 0.2.388 written 16:42:24 is not activated in that process; installed CLI is 0.35.400. Unblocks: provide the disposable document path and perform the real unsaved editor edit, since persisted CLI writes cannot substitute for this proof. Plan: tasks/agent-doc/plan-stop-gated-queue-head.md.
- [/] [#coder7] [operator-verify] Phase 7 — run the five-case live Coder workspace validation matrix after a reachable Coder workspace and authenticated access are available, with JetBrains Gateway remote backend, VS Code Remote-SSH, simultaneous dual-IDE attachment, workspace stop/start, and a no-tmux image variant. Plan: tasks/agent-doc/plan-coder-ide-hosted-tmux.md. Unblocks: provide the workspace URL or SSH target plus authentication and enable the current agent-doc plugin/extension in both remote IDE hosts.
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc/agent-doc-bugs.done.md -->

<!-- completed work archived in tasks/agent-doc/agent-doc-bugs.done.md -->
<!-- /agent:done -->
