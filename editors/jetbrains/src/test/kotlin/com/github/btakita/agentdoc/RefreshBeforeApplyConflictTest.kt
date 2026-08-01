package com.github.btakita.agentdoc

import java.nio.file.Files
import java.nio.file.Paths
import org.junit.Assert.*
import org.junit.Test

/**
 * `#p2j4` / `#jbcfdiag` — pin the invariant that a content-bearing `VirtualFile.refresh` never runs
 * against an UNSAVED document on an agent write/apply path. A refresh against an unsaved buffer
 * whose disk bytes diverged is what arms IntelliJ's memory↔disk "File Cache Conflict" dialog behind
 * a live editor (the remaining trigger after IPC-first writes removed the Rust-side behind-editor
 * disk writes).
 */
class RefreshBeforeApplyConflictTest {

    @Test
    fun `refresh decision skips unsaved buffers and allows clean buffers`() {
        // Clean (saved) buffer: refreshing from disk is safe — no memory↔disk
        // divergence the platform can turn into a dialog.
        assertTrue(shouldRefreshVfsBeforeApplyUtil(false))
        // Unsaved buffer: skip the refresh — the apply path reconciles via the
        // Document API; a bare refresh here would arm the File Cache Conflict dialog.
        assertFalse(shouldRefreshVfsBeforeApplyUtil(true))
    }

    @Test
    fun `apply-time refresh sites are gated on document save state`() {
        val patchWatcherPath =
            listOf(
                    Paths.get("src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt"),
                    Paths.get(
                        "editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt"
                    ),
                )
                .first { Files.exists(it) }
        val patchWatcher = Files.readString(patchWatcherPath)

        // Every content-bearing apply-time refresh must be wrapped by the
        // shouldRefreshVfsBeforeApplyUtil gate. Count bare refresh calls that are
        // NOT immediately preceded (within the same guard line above) by the gate.
        val refreshToken = "targetFile.refresh(false, false)"
        val gateToken = "shouldRefreshVfsBeforeApplyUtil"

        // There must be at least as many gate uses as apply-time refresh sites.
        val refreshCount = patchWatcher.split(refreshToken).size - 1
        val gateCount = patchWatcher.split(gateToken).size - 1
        assertTrue(
            "expected at least 3 apply-time refresh sites, found $refreshCount",
            refreshCount >= 3,
        )
        // 3 call-sites gated + 1 function definition reference => >= 4 occurrences.
        assertTrue(
            "expected the refresh gate to wrap apply-time refresh sites (found $gateCount uses)",
            gateCount >= refreshCount + 1,
        )

        // Each apply-time refresh must be immediately preceded by the gate in an
        // `if (...)` guard, never a bare unconditional call before an apply.
        assertEveryRefreshGated(patchWatcher, refreshToken, gateToken)
    }

    @Test
    fun `vcs refresh signal never recursively refreshes project content`() {
        val patchWatcherPath =
            listOf(
                    Paths.get("src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt"),
                    Paths.get(
                        "editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt"
                    ),
                )
                .first { Files.exists(it) }
        val patchWatcher = Files.readString(patchWatcherPath)
        val refreshVcs = functionBody(patchWatcher, "private fun refreshVcs(filePath: String?)")

        assertFalse(
            "a VCS signal must not refresh every open content file behind unsaved editors",
            refreshVcs.contains("LocalFileSystem.getInstance().refresh"),
        )
        assertFalse(
            "a VCS signal must not dirty every repository in a monorepo",
            refreshVcs.contains("markEverythingDirty"),
        )
        assertTrue(refreshVcs.contains("dirtyScope.fileDirty(file)"))
        assertTrue(refreshVcs.contains("pendingVcsRefreshFiles"))
        assertTrue(refreshVcs.contains("without content VFS refresh"))
    }

    @Test
    fun `remote crdt apply refreshes only a clean target before save`() {
        val crdtReplicaPath =
            listOf(
                    Paths.get("src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt"),
                    Paths.get(
                        "editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt"
                    ),
                )
                .first { Files.exists(it) }
        val crdtReplica = Files.readString(crdtReplicaPath)
        val helper = functionBody(crdtReplica, "private fun refreshCleanDocumentBeforeRemoteApply(")

        assertTrue(helper.contains("shouldRefreshVfsBeforeApplyUtil"))
        assertTrue(helper.contains("targetFile.refresh(false, false)"))
        assertTrue(helper.contains("isDocumentUnsaved(document)"))
        assertTrue(
            "delta and REPLACE delivery must refresh the clean target before mutation",
            crdtReplica.split("refreshCleanDocumentBeforeRemoteApply(").size - 1 >= 3,
        )
    }

    @Test
    fun `failed persistent remote projections are rolled back transactionally`() {
        val crdtReplicaPath =
            listOf(
                    Paths.get("src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt"),
                    Paths.get(
                        "editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt"
                    ),
                )
                .first { Files.exists(it) }
        val crdtReplica = Files.readString(crdtReplicaPath)
        val persist =
            functionBody(crdtReplica, "private fun persistRemoteCrdtTextIfSafe(")
        val reconcile =
            functionBody(crdtReplica, "private fun reconcileRemotePersistence(")
        val replace =
            functionBody(crdtReplica, "private fun applyReplaceDelivery(")
        val delta =
            functionBody(crdtReplica, "private fun applyRemoteTextOnEdt(")

        assertTrue(persist.contains("readRawDiskText(filePath)"))
        assertTrue(persist.contains("reconcileRemotePersistence("))
        assertTrue(reconcile.contains("RemotePersistReconciliation.RollbackToBefore"))
        assertTrue(reconcile.contains("applyMinimalDocumentEditUtil(document, targetText, beforeText)"))
        assertTrue(reconcile.contains("reloadFromDisk(document)"))
        assertTrue(reconcile.contains("RemotePersistReconciliation.PreserveAdvancedEditor"))
        assertTrue(reconcile.contains("RemotePersistOutcome(false, null)"))
        assertTrue(replace.contains("before,"))
        assertTrue(replace.contains("replace-delivery-persist-rollback"))
        assertTrue(delta.contains("persisted.editorTextForAck"))
        assertTrue(
            "memory-only projection must remain visible without entering disk persistence",
            delta.contains("RemoteEditorApplyOutcome(false, pending.targetText)"),
        )
    }

    @Test
    fun `forced reconnect registers from the live editor without preinstalling a retained target`() {
        val crdtReplicaPath =
            listOf(
                    Paths.get("src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt"),
                    Paths.get(
                        "editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt"
                    ),
                )
                .first { Files.exists(it) }
        val crdtReplica = Files.readString(crdtReplicaPath)
        val ensureOpenReplica =
            functionBody(
                crdtReplica,
                "fun ensureOpenDocumentReplica(",
            )

        assertFalse(ensureOpenReplica.contains("deferredWriteReconnectContent("))
        assertFalse(ensureOpenReplica.contains("deferredWritePostRegisterContent("))
        assertFalse(ensureOpenReplica.contains("applyMinimalDocumentEditUtil("))
        assertFalse(ensureOpenReplica.contains("persistRemoteCrdtTextIfSafe("))
        assertTrue(ensureOpenReplica.contains("registrationText = text"))
        assertTrue(ensureOpenReplica.contains("scheduleDeferredWriteReplayAfterRegistration("))
        assertTrue(ensureOpenReplica.contains("replaceCached = forceRefresh"))
        assertTrue(
            ensureOpenReplica.contains(
                "expectedEditorTextAtSwap = if (forceRefresh) registrationText else null"
            )
        )
        val forwarderFor = functionBody(crdtReplica, "private fun forwarderFor(")
        assertTrue(
            "a raced editor cut must be rejected before the controller bootstrap is projected",
            forwarderFor.contains("editorBufferText(filePath) != expectedEditorTextAtSwap") &&
                forwarderFor.contains("retainCanonicalProjectionAfterRegistration(filePath, initialEditorText, forwarder)") &&
                !forwarderFor.contains("forwarder.ensureEditorText(initialEditorText)"),
        )
        assertTrue(
            "deferred replay scheduling must follow exact editor registration",
            ensureOpenReplica.indexOf("val forwarder = forwarderFor(") <
                ensureOpenReplica.indexOf("scheduleDeferredWriteReplayAfterRegistration("),
        )
    }

    @Test
    fun `forced reconnect queues retained intent replay behind registration`() {
        val crdtReplicaPath =
            listOf(
                    Paths.get("src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt"),
                    Paths.get(
                        "editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt"
                    ),
                )
                .first { Files.exists(it) }
        val crdtReplica = Files.readString(crdtReplicaPath)
        val ensureOpenReplica =
            functionBody(
                crdtReplica,
                "fun ensureOpenDocumentReplica(",
            )
        val scheduleReplay =
            functionBody(
                crdtReplica,
                "private fun scheduleDeferredWriteReplayAfterRegistration(",
            )
        val replay =
            functionBody(
                crdtReplica,
                "private fun replayDeferredWriteAfterRegistration(",
            )
        val queueRemoteApply =
            functionBody(
                crdtReplica,
                "private fun queueRemoteTextApply(",
            )
        val applyRemoteOnEdt =
            functionBody(
                crdtReplica,
                "private fun applyRemoteTextOnEdt(",
            )

        assertFalse(ensureOpenReplica.contains("deferredWritePostRegisterContent("))
        assertFalse(ensureOpenReplica.contains("applyMinimalDocumentEditUtil("))
        assertFalse(ensureOpenReplica.contains("persistRemoteCrdtTextIfSafe("))
        assertTrue(
            "retained replay must be scheduled downstream of registration before ordinary delivery drains",
            ensureOpenReplica.indexOf("val forwarder = forwarderFor(") <
                ensureOpenReplica.indexOf("scheduleDeferredWriteReplayAfterRegistration(") &&
                ensureOpenReplica.indexOf("scheduleDeferredWriteReplayAfterRegistration(") <
                    ensureOpenReplica.indexOf("requestRemoteDrain(filePath, \"open-document\")"),
        )
        assertTrue(
            "replay must run as a second task on the same per-document lane",
            scheduleReplay.contains("documentWorkers.forDocument(filePath).execute") &&
                scheduleReplay.contains("replayDeferredWriteAfterRegistration("),
        )
        assertTrue(
            "the queued replay must project only after exact editor and local-edit fences",
            replay.indexOf("tryReadDocumentText(document) != registrationText") <
                replay.indexOf("NativePatching.projectDeferredWritePostRegister") &&
                replay.contains(
                    "requestUrgentRemoteDrain(filePath, \"post-register-projected-intent\")"
                ),
        )
        assertFalse(replay.contains("applyMinimalDocumentEditUtil("))
        assertFalse(replay.contains("persistRemoteCrdtTextIfSafe("))
        assertFalse(replay.contains("invokeAndWait"))
        assertTrue(queueRemoteApply.contains("RemoteEditorEffectToken("))
        assertTrue(
            "the standard EDT delivery path must validate its Lazily effect token before resolving or mutating a document",
            applyRemoteOnEdt.indexOf("remoteEditorEffectTokenCurrentUtil(") <
                applyRemoteOnEdt.indexOf("LocalFileSystem.getInstance()") &&
                applyRemoteOnEdt.indexOf("remoteEditorEffectTokenCurrentUtil(") <
                    applyRemoteOnEdt.indexOf("applyMinimalDocumentEditUtil("),
        )
    }

    @Test
    fun `queued editor effect is refused after intent retirement or endpoint loss`() {
        assertTrue(
            remoteEditorEffectTokenCurrentUtil(
                tokenGeneration = 7L,
                liveGeneration = 7L,
                endpointMatches = true,
                endpointBacked = true,
            )
        )
        assertFalse(
            remoteEditorEffectTokenCurrentUtil(
                tokenGeneration = 7L,
                liveGeneration = 8L,
                endpointMatches = true,
                endpointBacked = true,
            )
        )
        assertFalse(
            remoteEditorEffectTokenCurrentUtil(
                tokenGeneration = 7L,
                liveGeneration = 7L,
                endpointMatches = false,
                endpointBacked = true,
            )
        )
        assertFalse(
            remoteEditorEffectTokenCurrentUtil(
                tokenGeneration = 7L,
                liveGeneration = 7L,
                endpointMatches = true,
                endpointBacked = false,
            )
        )
    }

    @Test
    fun `jetbrains prompt poller is removed`() {
        val promptPollerPaths =
            listOf(
                Paths.get("src/main/kotlin/com/github/btakita/agentdoc/PromptPoller.kt"),
                Paths.get(
                    "editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/PromptPoller.kt"
                ),
            )
        val promptPanelPaths =
            listOf(
                Paths.get("src/main/kotlin/com/github/btakita/agentdoc/PromptPanel.kt"),
                Paths.get(
                    "editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/PromptPanel.kt"
                ),
            )

        assertTrue(promptPollerPaths.none { Files.exists(it) })
        assertTrue(promptPanelPaths.none { Files.exists(it) })

        val submitAction =
            Files.readString(
                listOf(
                        Paths.get("src/main/kotlin/com/github/btakita/agentdoc/SubmitAction.kt"),
                        Paths.get(
                            "editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/SubmitAction.kt"
                        ),
                    )
                    .first { Files.exists(it) }
            )
        val lifecycle =
            Files.readString(
                listOf(
                        Paths.get(
                            "src/main/kotlin/com/github/btakita/agentdoc/PluginLifecycleListener.kt"
                        ),
                        Paths.get(
                            "editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/PluginLifecycleListener.kt"
                        ),
                    )
                    .first { Files.exists(it) }
            )

        assertFalse(submitAction.contains("PromptPoller"))
        assertFalse(lifecycle.contains("PromptPoller"))
        assertFalse(lifecycle.contains("PromptPanel"))
        assertFalse(lifecycle.contains("prompt poller"))
    }

    @Test
    fun `jetbrains plugin uses event loops instead of hot polling`() {
        fun read(path: String): String =
            Files.readString(
                listOf(
                        Paths.get("src/main/kotlin/com/github/btakita/agentdoc/$path"),
                        Paths.get(
                            "editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/$path"
                        ),
                    )
                    .first { Files.exists(it) }
            )

        val turnWidget = read("TurnStateStatusBarWidget.kt")
        val turnRefresher = read("TurnStateBannerRefresher.kt")
        val turnBridge = read("TurnStateBridge.kt")
        val turnProvider = read("TurnStateBannerProvider.kt")
        val typingTracker = read("TypingTracker.kt")
        val crdtReplica = read("CrdtReplicaManager.kt")
        val patchWatcher = read("PatchWatcher.kt")
        val layoutDetector = read("LayoutChangeDetector.kt")
        val visualHighlighter = read("VisualHighlighterManager.kt")
        val nativeReload = read("NativeReloadCoordinator.kt")
        val lifecycle = read("PluginLifecycleListener.kt")

        assertFalse("status widget must not own a Swing timer", turnWidget.contains("Alarm("))
        assertFalse(
            "status widget must not call native projection while painting",
            turnWidget.contains("presentationForFile("),
        )
        assertFalse("banner refresher must not use a timer", turnRefresher.contains("Alarm("))
        assertFalse(
            "banner refresher must not define a polling interval",
            turnRefresher.contains("POLL_MS"),
        )
        assertFalse(
            "banner refresher must not retain slow-projection backoff",
            turnRefresher.contains("SLOW_BACKOFF"),
        )
        assertTrue(
            "banner refresher must cap each event-drain slice",
            turnRefresher.contains("TURN_STATE_MAX_PATHS_PER_DRAIN"),
        )
        assertTrue(
            "banner refresher must yield between backlog slices",
            turnRefresher.contains("TURN_STATE_DRAIN_YIELD_MS"),
        )
        assertTrue(
            "banner refresher must observe the native authority cache",
            turnRefresher.contains("NativeAdminControls.documentAuthority"),
        )
        assertTrue(
            "banner refresher must retain a bounded cache-observation cadence",
            turnRefresher.contains("TURN_STATE_CACHE_OBSERVE_INTERVAL_MS"),
        )
        assertFalse(
            "turn projection must not make imperative Project Controller subscriptions",
            turnBridge.contains("subscribeMirrorForFileViaProjectController"),
        )
        assertTrue(
            "turn projection must show Project Controller disconnects",
            turnBridge.contains("Project Controller disconnected"),
        )
        assertFalse(
            "turn projection must not call the legacy sidecar-capable FFI",
            turnBridge.contains("agent_doc_turn_projection"),
        )
        assertFalse(
            "banner collection must read cached state",
            turnProvider.contains("TurnStateBridge.presentationForFile"),
        )
        assertFalse(
            "banner collection must not trigger its own refresh loop",
            turnProvider.contains("banner-collect"),
        )
        assertFalse(
            "typing debounce report must not probe turn-state just for logging",
            typingTracker.contains("TurnStateBridge.presentationForFile"),
        )
        assertFalse(
            "CRDT replica manager must not schedule fixed-delay pulls",
            crdtReplica.contains("scheduleWithFixedDelay"),
        )
        assertFalse(
            "CRDT replica manager must not keep a poller thread",
            crdtReplica.contains("crdt-replica-poller"),
        )
        assertFalse(
            "Lazily liveness must not be shadowed by a plugin-owner heartbeat",
            crdtReplica.contains("PLUGIN_OWNER") || crdtReplica.contains("plugin_owner"),
        )
        assertFalse(
            "PatchWatcher must block on WatchService events",
            patchWatcher.contains("watchService.poll("),
        )
        assertFalse(
            "reload broadcast must not use a polling interval",
            patchWatcher.contains("LIB_RELOAD_BROADCAST_POLL_MS"),
        )
        assertFalse(
            "PatchWatcher must not use CRDT event sidecars",
            patchWatcher.contains(".agent-doc/crdt-replica-events"),
        )
        assertFalse(
            "missing-file recovery must not refresh the whole LocalFileSystem",
            patchWatcher.contains("LocalFileSystem.getInstance().refresh(false)"),
        )
        val patchWatcherStart =
            patchWatcher
                .substringAfter("fun start()")
                .substringBefore("/**\n     * Register a root directory")
        assertTrue(
            "base-root listener startup must leave the projectOpened EDT",
            patchWatcherStart.contains("executeOnPooledThread") &&
                patchWatcherStart.contains("registerRoot(basePath)"),
        )
        assertFalse(
            "plugin startup must not scan dormant nested project roots",
            patchWatcher.contains("discoverNestedRoots"),
        )
        assertTrue(
            "open editor files must register their concrete root on demand",
            lifecycle.contains("patchWatcher.registerRootForFile(file.path)"),
        )
        val registerRoot =
            patchWatcher
                .substringAfter("fun registerRoot(root: String)")
                .substringBefore("internal fun quiesceNativeEndpointsForReload")
        assertTrue(registerRoot.contains("SwingUtilities.isEventDispatchThread()"))
        assertTrue(registerRoot.contains("executeOnPooledThread(startListener)"))
        assertTrue(
            "PatchWatcher must wake CRDT drains from the shared typed editor intent",
            patchWatcher.contains("EditorIntent.DeliverCrdtRemote.token"),
        )
        assertFalse(
            "delivery events must not select imperative editor recovery branches",
            patchWatcher.contains("CrdtReplicaEventReason") ||
                patchWatcher.contains("requestTextAdopt") ||
                patchWatcher.contains("forceRefreshOpenDocumentReplica"),
        )
        assertFalse(
            "layout detector must not run a fallback polling thread",
            layoutDetector.contains("startFallbackPoll"),
        )
        assertFalse(
            "layout detector must not define a polling interval",
            layoutDetector.contains("POLL_INTERVAL_MS"),
        )
        assertFalse(
            "layout detector must not recursively attach listeners to the Swing tree",
            layoutDetector.contains("addRecursiveContainerListener") ||
                layoutDetector.contains(".addContainerListener("),
        )
        assertTrue(
            "layout detector must own one removable filtered AWT listener",
            layoutDetector.contains("AWTEventListener") &&
                layoutDetector.contains("AWTEvent.CONTAINER_EVENT_MASK") &&
                layoutDetector.contains("removeAWTEventListener"),
        )
        assertTrue(
            "structural layout changes must use the same surface graph as focus changes",
            layoutDetector.contains(
                "EditorTabSyncListener.install(project).onEditorLayoutChanged(project)"
            ),
        )
        assertFalse(
            "layout detector must not run a second tmux sync planner",
            layoutDetector.contains("runCommandWithTimeout") ||
                layoutDetector.contains("agent_doc_sync_try_lock") ||
                layoutDetector.contains("buildSyncCommand"),
        )
        assertFalse(
            "visual highlighter must not use a Swing timer",
            visualHighlighter.contains("Alarm("),
        )
        assertFalse(
            "visual highlighter must not tokenize the editor text on the UI apply path",
            visualHighlighter.contains("NativePatching.visualTokens(editor.document.text)"),
        )
        assertTrue(
            "visual highlighter tokenization must run on its event worker",
            visualHighlighter.contains("agent-doc-visual-highlighter-events"),
        )
        for ((name, source) in
            listOf(
                "PatchWatcher" to patchWatcher,
                "TypingTracker" to typingTracker,
                "CrdtReplicaManager" to crdtReplica,
                "VisualHighlighterManager" to visualHighlighter,
            )) {
            assertFalse(
                "$name must not let a native/background worker block indefinitely on an IDEA read permit",
                source.contains("runReadAction"),
            )
        }
        assertTrue(
            "native reload must stop inbound listeners before disposing CRDT managers",
            nativeReload.indexOf("PatchWatcher.quiesceAllForNativeReload()") <
                nativeReload.indexOf("CrdtReplicaManager.quiesceAllForNativeReload()"),
        )
        val forceRefreshReplica =
            crdtReplica
                .substringAfter("fun forceRefreshOpenDocumentReplica(")
                .substringBefore("fun ensureReplicaForOpenDocument(")
        assertTrue(forceRefreshReplica.contains("runOnEdtNonBlocking"))
        assertFalse(
            "native callbacks must not block waiting for an IDEA read permit",
            forceRefreshReplica.contains("runReadAction"),
        )
    }

    @Test
    fun `jetbrains crdt replica transport talks to cp not supervisor`() {
        val forwarder =
            Files.readString(
                listOf(
                        Paths.get(
                            "src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaForwarder.kt"
                        ),
                        Paths.get(
                            "editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaForwarder.kt"
                        ),
                    )
                    .first { Files.exists(it) }
            )

        assertTrue(
            "CRDT transport must target the Project Controller socket",
            forwarder.contains(".agent-doc/controller.sock"),
        )
        assertTrue(
            "CRDT transport must use the controller crdt_replica envelope",
            forwarder.contains("\"crdt_replica\""),
        )
        assertFalse(
            "CRDT transport must not connect to per-session supervisor sockets",
            forwarder.contains(".agent-doc/supervisor"),
        )

        assertFalse(
            "CRDT transport must expose no whole-editor adoption request",
            forwarder.contains("pushTextAdopt") ||
                forwarder.contains("agent_doc_reliable_sync_text_adopt_push") ||
                forwarder.contains("\"crdt_text_adopt\""),
        )
    }

    private fun assertEveryRefreshGated(source: String, refreshToken: String, gateToken: String) {
        var idx = source.indexOf(refreshToken)
        while (idx >= 0) {
            // Look back a small window for the gate `if`.
            val windowStart = (idx - 200).coerceAtLeast(0)
            val window = source.substring(windowStart, idx)
            assertTrue(
                "content-bearing refresh at offset $idx is not gated by shouldRefreshVfsBeforeApplyUtil:\n$window",
                window.contains("if (") && window.contains(gateToken),
            )
            idx = source.indexOf(refreshToken, idx + refreshToken.length)
        }
    }

    private fun functionBody(source: String, signature: String): String {
        val start = source.indexOf(signature)
        assertTrue("missing function signature: $signature", start >= 0)
        val brace = source.indexOf('{', start)
        assertTrue("missing function body: $signature", brace >= 0)
        var depth = 0
        for (index in brace until source.length) {
            when (source[index]) {
                '{' -> depth++
                '}' -> {
                    depth--
                    if (depth == 0) return source.substring(start, index + 1)
                }
            }
        }
        fail("unterminated function body: $signature")
        return ""
    }
}
