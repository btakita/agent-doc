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
Main is at `50727c34fd` and installed locally. Release v0.35.531 was published from `ce2a81485a`; the SDK/Local History recovery fix is on main at `374674c23c`, JetBrains source generations were recorded by `57b22181f4`, and the GH #330 loop-restore convergence fix is on main at `50727c34fd`. The #330 queue item handled in this cycle is complete.

<!-- status:sync -->
Latest published release: [v0.35.531](https://github.com/btakita/agent-doc/releases/tag/v0.35.531), published on GitHub at 2026-10-21 02:34:40 UTC (15 assets); PyPI 0.35.531 followed at 2026-10-21 02:35:29 UTC; installed locally (agent-doc 0.35.531). Published JetBrains packages: 0.2.753 (classic), 0.2.754 (IDE 262), and 0.2.755 (IDE 263), published with the release at 2026-10-21 02:34:40 UTC. Current main/source generations after the Local History follow-up are 0.2.756 (classic), 0.2.758 (IDE 262), and 0.2.757 (IDE 263); classic 0.2.756 and IDE 262 0.2.758 are installed locally. VS Code extension 0.2.83.
<!-- /status:sync -->
<!-- /agent:status -->
 

## Exchange

<!-- agent:exchange patch=append subagents -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20261020-232532.md`*

Compacted content:
- Archived 29 response topic(s): nested status:sync, reported-item integration, four new subagents; Subagent completed: `subagent:gh321` returned work for queue item “#gh-fix https://github.com/btakita/agent-doc/issue...; Subagent completed: `subagent:gh322` returned work for queue item “#gh-fix https://github.com/btakita/agent-doc/issue...; 26 more
- Prior summary/context: prior compacted content: Archived 49 response topic(s): Subagent completed: `subagent:issue310` returned work for que...

### Re: Response

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/328

Implemented and closed GitHub issue #328. Queue subagent dispatches now support optional `model` and `effort`, resolved in queue → preset → frontmatter order and omitted when unset. The resolved policy is included in briefs/preflight launch guidance and persisted through claim/start attribution.

> **Queue prompt:** [#n7kq]: Is this moot?

Yes. Commit `f88bcde1` restored all seven IDs immediately after the destructive edit. Two remain open in `sdk.md`, two moved intact to `sdk-dev.md`, and three were completed and reaped. Restoring them again would duplicate or resurrect work, so `#n7kq` is marked done without changing either SDK document.

> **Queue prompt:** [#t1v0]: look through the logs to validate if this works or not

Validated: Clear Session Context succeeded through Codex's `supervisor_restart_fresh` path. The later “projection dropped” event was only the redundant queued `/clear`. The SDK actor is ready, its latest cycle is committed, and no `tdue4cpuset` cycle is open. Do not clear again; the separate CPU-sharing decision remains open. `#t1v0` is marked done.

> **Queue prompt:** Can we have a setting for ultra-efficient usage of tokens? Perhaps regular clearing of session context in the queue would help? auto-clear should be an optional `attribute` in `agent:queue`. The `clear` attribute should default to clear per turn when not in subagent mode. When in subagent-mode, `clear` at the next idle spot? `clear` should be set in the default template.

Implemented a canonical bare `clear` queue attribute. Direct queues clear after each completed turn; `subagents` and `fan-out` queues defer clearing until the next safe idle point after live claims settle. Existing supervisor safety gates are reused, `disable-auto-clear` remains a compatible winning opt-out, and default/generated templates now include `clear`.

Both changes were integrated on `main` at `bca2b67240`. The combined `make check` passed, `make install` succeeded, the integration worktree is clean, and installed agent-doc remains version `0.35.530`. The persistent release status was already current, so it was not rewritten.

### Re: Subagent completed: `subagent:release531` returned work for queue item “release + publish” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=afdd50f7f01a76c45aea4361887f4a5a2e1db104adefc54b032663288d4a1e66 -->

### Re: Subagent completed: `subagent:sdklegalhistory` returned work for queue item “Run Agent Doc on sdk-legal.md issue: ``` java.lang.AssertionError at com.intellij.util.io.storage.AbstractRecordsTable.createNewRecord(AbstractRecordsTable.java:108) at com.intellij.history.core.LocalHistoryStorage.lambda$createNextRecord$10(LocalHistoryStorage.java:91) at com.intellij.util.ConcurrencyUtil.withLock(ConcurrencyUtil.java:293) at com.intellij.util.io.storage.AbstractStorage.withWriteLock(AbstractStorage.java:474) at com.intellij.history.core.LocalHistoryStorage.createNextRecord(LocalHistoryStorage.java:89) at com.intellij.history.core.ChangeListStorageImpl.writeNextSet(ChangeListStorageImpl.java:205) at com.intellij.history.core.ChangeList.doEndChangeSet(ChangeList.kt:85) at com.intellij.history.core.ChangeList.endChangeSet(ChangeList.kt:72) at com.intellij.history.core.LocalHistoryFacade.endChangeSet(LocalHistoryFacade.kt:91) at com.intellij.history.core.LocalHistoryFacade.endChangeSet$default(LocalHistoryFacade.kt:90) at com.intellij.history.core.LocalHistoryFacade.addChange(LocalHistoryFacade.kt:137) at com.intellij.history.core.LocalHistoryFacade.contentChanged(LocalHistoryFacade.kt:104) at com.intellij.history.integration.LocalHistoryEventDispatcher.beforeContentsChange(LocalHistoryEventDispatcher.kt:89) at com.intellij.history.integration.LocalHistoryEventDispatcher.handleBeforeEvent(LocalHistoryEventDispatcher.kt:94) at com.intellij.history.integration.LocalHistoryEventDispatcher.handleBeforeEvents$lambda$0(LocalHistoryEventDispatcher.kt:152) at com.intellij.history.integration.IdeaGateway.runWithVfsEventsDispatchContext(IdeaGateway.java:172) at com.intellij.history.integration.LocalHistoryEventDispatcher.handleBeforeEvents(LocalHistoryEventDispatcher.kt:150) at com.intellij.history.integration.LocalHistoryEventDispatcher.access$handleBeforeEvents(LocalHistoryEventDispatcher.kt:27) at com.intellij.history.integration.LocalHistoryEventDispatcher$LocalHistoryBulkFileListener.before(LocalHistoryEventDispatcher.kt:206) at com.intellij.util.messages.impl.MessageBusImplKt.invokeMethod(MessageBusImpl.kt:831) at com.intellij.util.messages.impl.MessageBusImplKt.invokeListener(MessageBusImpl.kt:771) at com.intellij.util.messages.impl.MessageBusImplKt.executeOrAddToQueue(MessageBusImpl.kt:596) at com.intellij.util.messages.impl.ToDirectChildrenMessagePublisher.publish$intellij_platform_core(CompositeMessageBus.kt:156) at com.intellij.util.messages.impl.MessagePublisher.invoke(MessageBusImpl.kt:544) at jdk.proxy2/jdk.proxy2.$Proxy199.before(Unknown Source) at com.intellij.openapi.vfs.newvfs.persistent.PersistentFSImpl.lambda$fireBeforeEvents$22(PersistentFSImpl.java:1678) at com.intellij.openapi.vfs.newvfs.impl.VfsThreadingUtil.runActionOnEdtRegardlessOfCurrentThread(VfsThreadingUtil.java:18) at com.intellij.openapi.vfs.newvfs.persistent.PersistentFSImpl.lambda$fireBeforeEvents$23(PersistentFSImpl.java:1678) at com.intellij.openapi.vfs.newvfs.persistent.PersistentFSImpl.runSuppressing(PersistentFSImpl.java:1296) at com.intellij.openapi.vfs.newvfs.persistent.PersistentFSImpl.fireBeforeEvents(PersistentFSImpl.java:1676) at com.intellij.openapi.vfs.newvfs.persistent.PersistentFSImpl$3.close(PersistentFSImpl.java:1191) at com.intellij.openapi.fileEditor.impl.LoadTextUtil.write(LoadTextUtil.java:462) at com.intellij.openapi.fileEditor.impl.LoadTextUtil.write(LoadTextUtil.java:473) at com.intellij.openapi.fileEditor.impl.FileDocumentManagerImpl.lambda$doSaveDocumentInWriteAction$6(FileDocumentManagerImpl.java:472) at com.intellij.pom.core.impl.PomModelImpl.guardPsiModificationsIn(PomModelImpl.java:352) at com.intellij.openapi.fileEditor.impl.FileDocumentManagerImpl.doSaveDocumentInWriteAction(FileDocumentManagerImpl.java:459) at com.intellij.openapi.fileEditor.impl.FileDocumentManagerImpl.lambda$doSaveDocument$2(FileDocumentManagerImpl.java:404) at com.intellij.openapi.application.impl.AppImplKt$rethrowCheckedExceptions$2.invoke(appImpl.kt:126) at com.intellij.platform.locking.impl.NestedLocksThreadingSupport.runWriteActionBlocking(NestedLocksThreadingSupport.kt:997) at com.intellij.openapi.application.impl.ApplicationImpl.runWriteAction(ApplicationImpl.java:1183) at com.intellij.openapi.fileEditor.impl.FileDocumentManagerImpl.doSaveDocument(FileDocumentManagerImpl.java:403) at com.intellij.openapi.fileEditor.impl.FileDocumentManagerImpl.saveDocumentInWriteSafeEnvironment(FileDocumentManagerImpl.java:352) at com.intellij.openapi.fileEditor.impl.FileDocumentManagerImpl.saveDocument(FileDocumentManagerImpl.java:347) at com.intellij.openapi.fileEditor.impl.FileDocumentManagerImpl.saveDocument(FileDocumentManagerImpl.java:337) at com.github.btakita.agentdoc.CrdtReplicaManager.persistRemoteCrdtTextIfSafe(CrdtReplicaManager.kt:3366) at com.github.btakita.agentdoc.CrdtReplicaManager.applyRemoteTextOnEdt(CrdtReplicaManager.kt:3281) at com.github.btakita.agentdoc.CrdtReplicaManager.scheduleRemoteEditorApply$lambda$77(CrdtReplicaManager.kt:3142) at com.intellij.openapi.application.TransactionGuardImpl.runWithWritingAllowed(TransactionGuardImpl.java:236) at com.intellij.openapi.application.TransactionGuardImpl.access$100(TransactionGuardImpl.java:22) at com.intellij.openapi.application.TransactionGuardImpl$1.run(TransactionGuardImpl.java:198) at com.intellij.openapi.application.impl.AppImplKt$runnableUnitFunction$1.invoke(appImpl.kt:124) at com.intellij.openapi.application.impl.AppImplKt$runnableUnitFunction$1.invoke(appImpl.kt:124) at com.intellij.platform.locking.impl.NestedLocksThreadingSupport.runWriteIntentReadAction(NestedLocksThreadingSupport.kt:705) at com.intellij.openapi.application.impl.ApplicationImpl.runIntendedWriteActionOnCurrentThread(ApplicationImpl.java:1094) at com.intellij.openapi.application.impl.ApplicationImpl$7.lambda$run$0(ApplicationImpl.java:648) at com.intellij.concurrency.ThreadContext.installThreadContext(threadContext.kt:305) at com.intellij.openapi.application.impl.ApplicationImpl$7.run(ApplicationImpl.java:646) at com.intellij.util.concurrency.ChildContext$runInChildContext$1.invoke(propagation.kt:192) at com.intellij.util.concurrency.ChildContext$runInChildContext$1.invoke(propagation.kt:192) at com.intellij.util.concurrency.ChildContext.runInChildContext(propagation.kt:198) at com.intellij.util.concurrency.ChildContext.runInChildContext(propagation.kt:192) at com.intellij.util.concurrency.ContextRunnable.lambda$run$0(ContextRunnable.java:26) at com.intellij.concurrency.ThreadContext.resetThreadContext(threadContext.kt:294) at com.intellij.util.concurrency.ContextRunnable.run(ContextRunnable.java:25) at com.intellij.openapi.application.impl.NonBlockingFlushQueue.runNextEvent$lambda$1$1$0(NonBlockingFlushQueue.kt:334) at com.intellij.openapi.progress.impl.CoreProgressManager.computePrioritized(CoreProgressManager.java:928) at com.intellij.openapi.application.impl.NonBlockingFlushQueue.runNextEvent$lambda$1$1(NonBlockingFlushQueue.kt:333) at com.intellij.concurrency.ThreadContext.resetThreadContext(threadContext.kt:294) at com.intellij.openapi.application.impl.NonBlockingFlushQueue.runNextEvent$lambda$1(NonBlockingFlushQueue.kt:330) at com.intellij.platform.locking.impl.NestedLocksThreadingSupport.tryRunWriteIntentReadAction(NestedLocksThreadingSupport.kt:747) at com.intellij.openapi.application.impl.NonBlockingFlushQueue.runNextEvent(NonBlockingFlushQueue.kt:326) at com.intellij.openapi.application.impl.NonBlockingFlushQueue.flushNow(NonBlockingFlushQueue.kt:305) at com.intellij.openapi.application.impl.NonBlockingFlushQueue.FLUSH_NOW$lambda$0(NonBlockingFlushQueue.kt:167) at java.desktop/java.awt.event.InvocationEvent.dispatch(InvocationEvent.java:323) at java.desktop/java.awt.EventQueue.dispatchEventImpl(EventQueue.java:732) at java.desktop/java.awt.EventQueue.dispatchEvent(EventQueue.java:711) at com.intellij.ide.IdeEventQueue.defaultDispatchEvent(IdeEventQueue.kt:721) at com.intellij.ide.IdeEventQueue._dispatchEvent(IdeEventQueue.kt:574) at com.intellij.ide.IdeEventQueue.dispatchEvent$lambda$0$0$0(IdeEventQueue.kt:378) at com.intellij.ide.IdeEventQueueKt.performActivity$lambda$0(IdeEventQueue.kt:1111) at com.intellij.openapi.application.TransactionGuardImpl.performActivity(TransactionGuardImpl.java:106) at com.intellij.ide.IdeEventQueueKt.performActivity(IdeEventQueue.kt:1111) at com.intellij.ide.IdeEventQueue.dispatchEvent$lambda$0(IdeEventQueue.kt:376) at com.intellij.ide.IdeEventQueue.dispatchEvent(IdeEventQueue.kt:416) at java.desktop/java.awt.EventDispatchThread.pumpOneEventForFilters(EventDispatchThread.java:207) at java.desktop/java.awt.EventDispatchThread.pumpEventsForFilter(EventDispatchThread.java:128) at java.desktop/java.awt.EventDispatchThread.pumpEventsForHierarchy(EventDispatchThread.java:117) at java.desktop/java.awt.EventDispatchThread.pumpEvents(EventDispatchThread.java:113) at java.desktop/java.awt.EventDispatchThread.pumpEvents(EventDispatchThread.java:105) at java.desktop/java.awt.EventDispatchThread.run(EventDispatchThread.java:92) ```” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=a4a1d4beb6f7fcf993138631eaeea52bd0d39d5008b274d2e2229990045280e8 -->

### Re: Release v0.35.531 and issue fixes - 2026-10-20T22:36:45-04:00 (gpt-5)

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/329

Fixed and closed issue #329. Reactive answered-free-text retirement now removes only that row's binary completion evidence through the shared queue policy, preserving unrelated completion cells. The fix landed in the release line as `9d288c16dc`; its targeted regression and the full repository check passed.

> **Queue prompt:** release + publish

Released and published [v0.35.531](https://github.com/btakita/agent-doc/releases/tag/v0.35.531) from `ce2a81485a`. The release workflow completed all ten jobs successfully; the independent publication verifier passed, GitHub exposes 15 checksum-covered assets, PyPI 0.35.531 is live, and the local install is 0.35.531.

> **Queue prompt:** Run Agent Doc on sdk-legal.md issue:

Fixed the IntelliJ Local History `AssertionError` from `persistRemoteCrdtTextIfSafe`. Remote-save recovery now treats `RuntimeException` and `AssertionError` as recoverable persistence failures and routes both through the existing editor/disk reconciliation and rollback path, while VM/linkage failures still escape. The implementation and regression coverage landed on main as `374674c23c`; package generations were recorded by `57b22181f4`.

The follow-up full `make check` passed, including 11,374 Rust tests, VS Code, cross-editor simulation, classic/262/263 JetBrains suites, Python bootstrap, and TLA evidence. One `make install` completed successfully with classic 0.2.756, IDE 262 0.2.758, and VS Code 0.2.83; IDE 263 source generation is 0.2.757. The changed-file private-name scan and `git diff --check` were clean. At the required one-time observation after push, the new main CI run was in progress with no reported failure.

### Re: #gh-fix https://github.com/btakita/agent-doc/issues/330 — gpt-5 · 2026-10-20T23:34:00-04:00

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/330

Implemented and closed GitHub issue #330. An already-visible loop roster now still crosses the delivery-convergence guard: unsettled retained delivery remains an error, so durable `restore_pending` state and its backoff retry survive instead of being cleared prematurely.

The fix is on `main` at `50727c34fd5bcf8dac988b9d5dd52a579fcb71a9`. It changed `agent-doc-queue-io/src/queue_schedule.rs` and `specs/07-orchestration-commands.md`, including a focused regression for the retained-but-visible restore case. The worker's focused test, `make check-fast`, full `make check`, `make tmux-ci`, formatting, and diff checks passed. After fast-forward integration, the coordinator's independent full `make check` also passed with exit code 0 and zero detected failures, and the single `make install` completed successfully. Installed agent-doc remains `0.35.531`.

GitHub Actions run `39538277992` for this exact commit was in progress at the required one-time post-push observation, with no reported failure; it was not awaited.
<!-- agent:boundary:003775db -->
<!-- /agent:exchange -->

<!--
-->

## Queue: Main
<!-- agent:queue id="main" subagents preset="#spec-test-build-install-commit-push" priority go -->
- ~~#gh-fix https://github.com/btakita/agent-doc/issues/330~~
- 🔄 release + publish
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
