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
  '#self': agent-doc-bugs.md
dogfood:
  path: tasks/agent-doc/agent-doc-bugs.md
  auto: true
queue: go
---

## Status

<!-- agent:status patch=replace sync="Track the latest published agent-doc release + publication time and latest JetBrains plugin release version + publication time." -->

📥 Queue handoff: 8 reported items awaiting coordinator integration, verification, and closeout.
Main is at `b6b9af6e15` and agent-doc 0.35.532 is installed locally. Release v0.35.532 was published from `71f9801b49`. Post-release main now includes the inline-code-safe, occurrence-ordered queue preset authorization fix for GitHub issue #331 and the positive-evidence-only Remote Dev pane-order fix for GitHub issue #332; both issues are closed, the integrated full suite passed, and the current mainline was installed locally.

<!-- status:sync -->
Latest published release: [v0.35.535](https://github.com/btakita/agent-doc/releases/tag/v0.35.535), published on GitHub at 2026-10-21 16:47:10 UTC (15 assets); PyPI 0.35.535 followed at 2026-10-21 16:48:01 UTC. Release commit f514601d20. Published JetBrains packages: 0.2.762 (classic), 0.2.763 (IDE 262), and 0.2.764 (IDE 263), published with the release at 2026-10-21 16:47:10 UTC; VS Code 0.2.83. Post-release main is a0e4b0ecd4 with the five-fix integration batch; its JetBrains source/package generations are 0.2.765 (classic), 0.2.766 (IDE 262), and 0.2.767 (IDE 263). The integrated CLI/native library and available editor packages are installed locally; the active mid-cycle native reload is deferred to the next idle boundary.
<!-- /status:sync -->
<!-- /agent:status -->
 

## Exchange

<!-- agent:exchange patch=append subagents -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20261021-053012.md`*

Compacted content:
- Archived 7 response topic(s): Response; Subagent completed: `subagent:release531` returned work for queue item “release + publish”; Subagent completed: `subagent:sdklegalhistory` returned work for queue item “Run Agent Doc on sdk-legal.md issue: ```...; 4 more
- Prior summary/context: prior compacted content: Archived 29 response topic(s): nested status:sync, reported-item integration, four new subag...

### Re: #gh-fix issue 331 — gpt-5 · 2026-10-21T02:26-04:00

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/331

Fixed and closed [GH #331](https://github.com/btakita/agent-doc/issues/331). Queue preset authorization now ignores preset names inside balanced inline-code spans and preserves preset expansion order by textual occurrence instead of map order. The implementation, specification, and CLI/unit regressions landed on `main` as `bef5b08236`.

Verification passed: focused unit and queue-brief CLI regressions each 1/1; the worker's full suite passed 11,378 nextest tests, 1,982 batched tests, 202 VS Code tests, cross-editor simulation, Python bootstrap, all three JetBrains builds, and TLA validation.

### Re: #gh-fix issue 332 — gpt-5 · 2026-10-21T02:26-04:00

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/332

Fixed and closed [GH #332](https://github.com/btakita/agent-doc/issues/332). Cold Remote Dev layout observations may still provide a stable fold basis, but they no longer become positive exported column-order authority; order stays unknown until an editor/manual order-bearing signal arrives, and focus escalation carries only a genuinely positive basis. The implementation, JetBrains specification, and regressions landed on `main` as `b6b9af6e15`.

Verification passed: focused controller tests, 810/810 controller-library tests, and 27/27 editor-surface tests; the worker's full suite passed 11,376 nextest tests, 202 VS Code tests, cross-editor simulation, Python bootstrap, all JetBrains builds, and TLA validation.

The two commits were integrated serially from `0b5909927f` and pushed to `origin/main` at `b6b9af6e15`. A single authoritative combined `make check` then passed with exit 0 and zero detected failures. The single batch `make install` also passed, installing agent-doc 0.35.532, VS Code 0.2.83, and JetBrains generations 0.2.759/0.2.760/0.2.761. The latest pre-integration main CI run was green; the new push will receive its own CI run asynchronously.
> **Chat prompt (#chatprompt):** is v0.35.533 released?

> **Chat prompt (#chatprompt):** and published

> **Chat prompt (#chatprompt):** The status does not reflect 533 being released. Use a subagent to apply the correct hooks that would have made 533 visible in status.

> **Chat prompt (#chatprompt):** Also, tell me how to use the hooks or edit the document to use the hooks

> **Chat prompt (#chatprompt):** subagent: The sdk-demo.md queue items were duplicated again: - `you (sending)` is stale Unity acknowledgement state: fix this. The system should be realtime. Should we use 
> - `you (sending)` is stale Unity acknowledgement state: fix this. The system should be realtime. Should we use reactive logic or keep it callback based
> - `you (sending)` is stale Unity acknowledgement state: fix this. The system should be realtime. Should we use reactive logic?

> **Chat prompt (#chatprompt):** You need to account for Zscaler?

> **Chat prompt (#chatprompt):** Unity does not run with Zscaler. Is this for conformance?

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/333

> **Queue prompt:** release + publish: include issue 331-333

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/334: Can you simulation test the Coder environment?

> **Queue prompt:** release + publish: include issue 334

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/335

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/336

> **Queue prompt:** 📌 In sdk-demo.md, the queue item "- 📥 Why are you telling me to `cd /home/brian/work/btakita/agent-loop/worktrees/haiven-dev/haiven-cli/demo-open-options-20261021` if we are working in `~/work/btakita/worktrees/haiven-dev/haiven-cli/unity-hub-register-option`?" was never struck. Can this fix be a part of the 335-336 release or do we need a new release?

> **Queue prompt:** The queue items in sdk-dev.md do not have a prefix despite an active turn with subagents for each queue item. Include fix in 335-336 reease.

> **Queue prompt:** release + publish: include issue 335-336

> **Queue prompt:** sdk-demo.md queue items were duplicated as I edited the queue items after starting Run Agent Doc
>
> ```
> - ⚠️ I signed in the Boss Room demo. Are we using the real OAuth flow on localhost using the SBX credentials?
> - ⚠️ I signed in the Boss Room demo. Are we using the real OAuth flow on localhost using the SBX credentials?
> - ⚠️ I sent a test message to the Boss Room demo. It still says "you(sending)". Is the chat integrated and working? ![img_41.png](img_41.png). What room was created? Can I see it on my localhost chat rooms on the haiven website?
> - ⚠️ I signed in the Boss Room demo. Are we using the real OAuth flow on localhost using the SBX credentials?
> - 🟡 I sent a test message to the Boss Room demo. It still says "you(sending)". Is the chat integrated and working? ![img_41.png](img_41.png). What room was created? Can I see it on my localhost chat rooms on the haiven frontend?
> ```

> **Queue prompt:** do [#queueclaimprojectionrace]: This should relate to the "sdk-demo.md queue items were duplicated" item above.

> **Operator note:** This should relate to the "sdk-demo.md queue items were duplicated" item above.

Yes. They share the same authority race: an editor normalization/edit could win the visible merge after the durable queue transition, leaving duplicated free-text revisions or hiding the ledger-derived lifecycle marker. The integrated fixes address both sides of that causal family.

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/337

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/338: Why are we not able to detect the tmux pane state? Do we need agent-doc to use lazily state projection between the IDE frontend + backend? Ensure editor plugin parity. Do we need bi-directional state syncing so the controller does not need to "guess"?

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/339

### Re: releases, realtime fixes, and queue lifecycle — gpt-5 · 2026-10-21T13:20-04:00

Yes: v0.35.533 was released and published. GitHub published 15 assets at 2026-10-21 13:58:09 UTC, and PyPI published 0.35.533 at 13:59:00 UTC. The later v0.35.534 release was published at 15:09:44 UTC (PyPI 15:10:35), and [v0.35.535](https://github.com/btakita/agent-doc/releases/tag/v0.35.535) is now the latest public release: GitHub published 15 assets at 16:47:10 UTC and PyPI followed at 16:48:01 UTC. v0.35.535 contains #335 and #336; release commit `f514601d20`. Issues #333–#339 are closed.

The status drift was a hook-usage problem, not missing release state. I repaired the managed status during the cycle with:

```sh
agent-doc status set FILE --text 'Latest published release: ...'
```

For normal closeout, supply the same status payload atomically with the response:

```sh
agent-doc respond FILE --stream --origin skill --template \
  --status 'Latest published release: ...'
```

The document needs one managed region. Ordinary prose may remain before the synchronized result:

```markdown
## Status
<!-- agent:status patch=replace sync="Track the latest published agent-doc release + publication time and latest JetBrains plugin release version + publication time." -->
Optional human-maintained context.
<!-- status:sync -->
Hook-managed release result.
<!-- /status:sync -->
<!-- /agent:status -->
```

`status set` is the mid-cycle repair hook; `respond --status` is the preferred transactional closeout hook. Both update `status:sync` while preserving surrounding prose.

The SDK fix keeps the existing callback/event architecture. The socket is already push/realtime, and Unity already marshals host callbacks to its main thread; Rx would add a dependency without fixing the fault boundary. The receive loop now isolates malformed frames and throwing subscribers so a later correlated acknowledgement still reaches chat. Existing `1006` close/reconnect/resend behavior remains separate and preserves `clientMessageId`. Commit `2b1de59716` is pushed on `haiven-sdk` branch `agent/realtime-chat-ack-20261021`; C#, Unity, spec-link, and full SDK checks passed.

Zscaler is not a Unity runtime requirement. It belongs only to agent-doc's Coder/network conformance simulations. Unity is covered by SDK wire/reconnect/conformance tests against its real WebSocket behavior; no proxy-specific Unity logic was added.

The duplicate queue regression and the missing lifecycle prefixes had the same causal family. Post-apply race repair now enforces exact-copy multiplicity before narrowly collapsing causally proven interleaved free-text edit revisions. Queue claim/start/report projection now treats the durable ledger as authority, rereads the live editor/CRDT cut, reapplies markers by work identity, verifies convergence, and retries or refuses instead of reporting success after a merge chose stale editor text. This directly relates `#queueclaimprojectionrace` to the `sdk-demo.md` duplicates. Commits `a8e1e714c3` and `0f174e979e` are on main. Existing authored exchange history is intentionally preserved; the current `sdk-demo.md` queue contains no duplicate OAuth/chat rows.

The previously missed `sdk-demo.md` worktree-path row was lifecycle-safely closed by its owner with the exact response echo; it required no new release by itself.

Post-v0.35.535 main is now `a0e4b0ecd4` and also contains:

- #337: retained synchronous-delivery refusal is a safe deferred SessionStart record, not `record_failed` (`9c5e32e39c`).
- #338: manual unknown-order sync can replace only a basis with no positive authority. The controller already observes tmux locally, so bidirectional layout-authority echo is unnecessary; editor-bound projection is acknowledgement/presentation only. Positive geometry remains protected and editor parity is tested (`7c6c0ffeb8`).
- #339: exact-263 Kotlin/plugin-source hot upgrade remains honestly restart-bound. Restart-free acceptance requires full modular-graph replacement plus generation-fenced backend/frontend activation receipts for the same verified artifact; native-only derivations remain restart-free (`a0274220d6`).
- JetBrains source generations 0.2.765 (classic), 0.2.766 (IDE 262), and 0.2.767 (IDE 263) (`a0e4b0ecd4`). Published v0.35.535 packages remain 0.2.762/0.2.763/0.2.764 until the next release.

The integrated `make check` passed: 11,386 Rust tests, 202 VS Code tests, cross-editor simulation, all JetBrains generations, bootstrap, and TLA. One coordinator `make install` succeeded; the active editor's native reload is deferred to its next idle boundary. The six-commit batch is pushed to `origin/main`. Because these five post-release fixes landed after the v0.35.535 tag, they are queued for a subsequent release rather than being falsely attributed to v0.35.535.
> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/333

> **Queue prompt:** release + publish: include issue 331-333

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/334: Can you simulation test the Coder environment?

> **Queue prompt:** release + publish: include issue 334

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/335

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/336

> **Queue prompt:** 📌 In sdk-demo.md, the queue item "- 📥 Why are you telling me to `cd /home/brian/work/btakita/agent-loop/worktrees/haiven-dev/haiven-cli/demo-open-options-20261021` if we are working in `~/work/btakita/worktrees/haiven-dev/haiven-cli/unity-hub-register-option`?" was never struck. Can this fix be a part of the 335-336 release or do we need a new release?

> **Queue prompt:** The queue items in sdk-dev.md do not have a prefix despite an active turn with subagents for each queue item. Include fix in 335-336 reease.

> **Queue prompt:** release + publish: include issue 335-336

> **Queue prompt:** sdk-demo.md queue items were duplicated as I edited the queue items after starting Run Agent Doc
>
> ```
> - ⚠️ I signed in the Boss Room demo. Are we using the real OAuth flow on localhost using the SBX credentials?
> - ⚠️ I signed in the Boss Room demo. Are we using the real OAuth flow on localhost using the SBX credentials?
> - ⚠️ I sent a test message to the Boss Room demo. It still says "you(sending)". Is the chat integrated and working? ![img_41.png](img_41.png). What room was created? Can I see it on my localhost chat rooms on the haiven website?
> - ⚠️ I signed in the Boss Room demo. Are we using the real OAuth flow on localhost using the SBX credentials?
> - 🟡 I sent a test message to the Boss Room demo. It still says "you(sending)". Is the chat integrated and working? ![img_41.png](img_41.png). What room was created? Can I see it on my localhost chat rooms on the haiven frontend?
> ```

### Re: closeout queue-snapshot projection — gpt-5 · 2026-10-21T13:36-04:00

The residual rows above were already completed; they remained visible because the committed response documented the status-hook markup inside a fenced block. The response stripper incorrectly treated the example's `agent:status` closing comment as the end of the real exchange, so the live queue leaked into exchange steering and completion matching became inconsistent.

Fixed on `main` at `1820a9a0cf`. Response stripping now terminates only at a real `agent:boundary:*` marker or the exact exchange closing marker. The regression reproduces the fenced status example, malformed multiline queue item, and repeated ⚠️→🟡→🚧 projections. Red-before/green-after proof, 394 affected-crate tests, documentation audit, and the integrated full gate all pass; the full gate contains 11,387 Rust tests, 202 VS Code tests, cross-editor simulation, every JetBrains generation, bootstrap, and TLA. The corrected binary and editor packages are installed locally, and the commit is pushed to `origin/main`.

The `sdk-demo.md` live queue is clean: it contains only the unrelated SBX-login item and its `subagent:real_login` claim. The OAuth/chat repetitions now exist only in historical response and diagnostic prose, which should not be rewritten. Prevention remains on main in `a8e1e714c3` (interleaved edit deduplication) and `0f174e979e` (claim projection linearization), with their focused regressions green.

The older release rows resolve to their already-published artifacts: v0.35.533 for #331–#333, v0.35.534 for #334, and v0.35.535 for #335–#336. The worktree-path row was already lifecycle-safely closed. The post-v0.35.535 fixes, including this closeout projection correction, remain explicitly queued under `#post535release`; they are not falsely attributed to v0.35.535.
<!-- agent:boundary:4884d9a1 -->
<!-- /agent:exchange -->

<!--
-->

## Queue: Main
<!-- agent:queue id="main" subagents preset="#spec-test-build-install-commit-push" priority go -->
- do [#post535release]
- ~~#gh-fix https://github.com/btakita/agent-doc/issues/331~~
- ~~#gh-fix https://github.com/btakita/agent-doc/issues/332~~
- ~~#gh-fix https://github.com/btakita/agent-doc/issues/333~~ — auto-struck: answered this cycle (#ftstrike)
- ~~release + publish: include issue 331-333~~ — auto-struck: answered this cycle (#ftstrike)
- ~~#gh-fix https://github.com/btakita/agent-doc/issues/334: Can you simulation test the Coder environment?~~ — auto-struck: answered this cycle (#ftstrike)
- ~~release + publish: include issue 334~~ — auto-struck: answered this cycle (#ftstrike)
- ~~#gh-fix https://github.com/btakita/agent-doc/issues/335~~ — auto-struck: answered this cycle (#ftstrike)
- ~~#gh-fix https://github.com/btakita/agent-doc/issues/336~~ — auto-struck: answered this cycle (#ftstrike)
- ~~📌 In sdk-demo.md, the queue item "- 📥 Why are you telling me to `cd /home/brian/work/btakita/agent-loop/worktrees/haiven-dev/haiven-cli/demo-open-options-20261021` if we are working in `~/work/btakita/worktrees/haiven-dev/haiven-cli/unity-hub-register-option`?" was never struck. Can this fix be a part of the 335-336 release or do we need a new release?~~ — auto-struck: answered this cycle (#ftstrike)
- ~~The queue items in sdk-dev.md do not have a prefix despite an active turn with subagents for each queue item. Include fix in 335-336 reease.~~ — auto-struck: answered this cycle (#ftstrike)
- ~~release + publish: include issue 335-336~~ — auto-struck: answered this cycle (#ftstrike)
~~~done
sdk-demo.md queue items were duplicated as I edited the queue items after starting Run Agent Doc
```
- ⚠️ I signed in the Boss Room demo. Are we using the real OAuth flow on localhost using the SBX credentials?
- ⚠️ I signed in the Boss Room demo. Are we using the real OAuth flow on localhost using the SBX credentials?
- ⚠️ I sent a test message to the Boss Room demo. It still says "you(sending)". Is the chat integrated and working? ![img_41.png](img_41.png). What room was created? Can I see it on my localhost chat rooms on the haiven website?
- ⚠️ I signed in the Boss Room demo. Are we using the real OAuth flow on localhost using the SBX credentials?
- 🟡 I sent a test message to the Boss Room demo. It still says "you(sending)". Is the chat integrated and working? ![img_41.png](img_41.png). What room was created? Can I see it on my localhost chat rooms on the haiven frontend?
```
~~~
- ~~do [#queueclaimprojectionrace]: This should relate to the "sdk-demo.md queue items were duplicated" item above.~~
- ~~#gh-fix https://github.com/btakita/agent-doc/issues/337~~ — auto-struck: answered this cycle (#ftstrike)
- ~~#gh-fix https://github.com/btakita/agent-doc/issues/338: Why are we not able to detect the tmux pane state? Do we need agent-doc to use lazily state projection between the IDE frontend + backend? Ensure editor plugin parity. Do we need bi-directional state syncing so the controller does not need to "guess"?~~ — auto-struck: answered this cycle (#ftstrike)
- ~~#gh-fix https://github.com/btakita/agent-doc/issues/339~~ — auto-struck: answered this cycle (#ftstrike)
<!-- /agent:queue -->

## Queue: Issues
<!-- agent:queue id=issues loop interval=1h -->
- Watch https://github.com/btakita/agent-doc/issues for new issues. Add new issues into the backlog + queue except issue #40 + #45.
<!-- /agent:queue -->

## Todo
<!-- agent:todo -->
- [ ] [#ahfe] Live-check GH #267 on the 263 IDE: after `agent-doc upgrade` WITHOUT restart, once idea.log shows `[native] hot-reloaded libagent_doc`, Run Agent Doc should complete (ops.log `editor_route_superseded_editor_admitted`, route output ends with the advisory line). 2026-10-15 operator: did not work (believed); watch for bug reports before reopening GH #267.
- [ ] [#ctg7] Under heavy load, switch the editor 2 panes -> 1 -> 2 and confirm tmux follows and stays at 2 (stale-controller fix).
<!-- /agent:todo -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#post535release] Release + publish the post-v0.35.535 main batch containing the queue-claim projection, realtime edit deduplication, and issues #337-#339 fixes with JetBrains generations 0.2.765/0.2.766/0.2.767.
- [/] [#gh299-live-263-proof] Live-verify v0.35.519 on JetBrains 263.7031 with changed plugin sources: `agent-doc upgrade` must hot-reload and complete without an IDE restart; close GH #299 only with that proof.
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
