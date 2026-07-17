package com.github.btakita.agentdoc

import java.nio.file.Files
import java.nio.file.Paths
import org.junit.Assert.*
import org.junit.Test

/**
 * `#p2j4` / `#jbcfdiag` — pin the invariant that a content-bearing
 * `VirtualFile.refresh` never runs against an UNSAVED document on an agent
 * write/apply path. A refresh against an unsaved buffer whose disk bytes diverged
 * is what arms IntelliJ's memory↔disk "File Cache Conflict" dialog behind a live
 * editor (the remaining trigger after IPC-first writes removed the Rust-side
 * behind-editor disk writes).
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
        val patchWatcherPath = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt"),
        ).first { Files.exists(it) }
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
        val patchWatcherPath = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt"),
        ).first { Files.exists(it) }
        val patchWatcher = Files.readString(patchWatcherPath)
        val refreshVcs = functionBody(patchWatcher, "private fun refreshVcs()")

        assertFalse(
            "a VCS signal must not refresh every open content file behind unsaved editors",
            refreshVcs.contains("LocalFileSystem.getInstance().refresh"),
        )
        assertTrue(refreshVcs.contains("VcsDirtyScopeManager.getInstance(project).markEverythingDirty()"))
        assertTrue(refreshVcs.contains("without content VFS refresh"))
    }

    @Test
    fun `remote crdt apply refreshes only a clean target before save`() {
        val crdtReplicaPath = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt"),
        ).first { Files.exists(it) }
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
    fun `forced reconnect registers from the live editor without preinstalling a retained target`() {
        val crdtReplicaPath = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt"),
        ).first { Files.exists(it) }
        val crdtReplica = Files.readString(crdtReplicaPath)
        val ensureOpenReplica = functionBody(
            crdtReplica,
            "fun ensureOpenDocumentReplica(",
        )

        assertFalse(ensureOpenReplica.contains("deferredWriteReconnectContent("))
        assertFalse(ensureOpenReplica.contains("applyMinimalDocumentEditUtil("))
        assertFalse(ensureOpenReplica.contains("persistRemoteCrdtTextIfSafe("))
        assertTrue(ensureOpenReplica.contains("registrationText = text"))
        assertTrue(ensureOpenReplica.contains("deferredWriteReconnectPropagated(filePath, registrationText)"))
        assertTrue(ensureOpenReplica.contains("replaceCached = forceRefresh"))
        assertTrue(ensureOpenReplica.contains("expectedEditorTextAtSwap = if (forceRefresh) registrationText else null"))
    }

    @Test
    fun `jetbrains prompt poller is removed`() {
        val promptPollerPaths = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/PromptPoller.kt"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/PromptPoller.kt"),
        )
        val promptPanelPaths = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/PromptPanel.kt"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/PromptPanel.kt"),
        )

        assertTrue(promptPollerPaths.none { Files.exists(it) })
        assertTrue(promptPanelPaths.none { Files.exists(it) })

        val submitAction = Files.readString(
            listOf(
                Paths.get("src/main/kotlin/com/github/btakita/agentdoc/SubmitAction.kt"),
                Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/SubmitAction.kt"),
            ).first { Files.exists(it) }
        )
        val lifecycle = Files.readString(
            listOf(
                Paths.get("src/main/kotlin/com/github/btakita/agentdoc/PluginLifecycleListener.kt"),
                Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/PluginLifecycleListener.kt"),
            ).first { Files.exists(it) }
        )

        assertFalse(submitAction.contains("PromptPoller"))
        assertFalse(lifecycle.contains("PromptPoller"))
        assertFalse(lifecycle.contains("PromptPanel"))
        assertFalse(lifecycle.contains("prompt poller"))
    }

    @Test
    fun `jetbrains plugin uses event loops instead of hot polling`() {
        fun read(path: String): String = Files.readString(
            listOf(
                Paths.get("src/main/kotlin/com/github/btakita/agentdoc/$path"),
                Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/$path"),
            ).first { Files.exists(it) },
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

        assertFalse("status widget must not own a Swing timer", turnWidget.contains("Alarm("))
        assertFalse("status widget must not call native projection while painting", turnWidget.contains("presentationForFile("))
        assertFalse("banner refresher must not use a timer", turnRefresher.contains("Alarm("))
        assertFalse("banner refresher must not define a polling interval", turnRefresher.contains("POLL_MS"))
        assertFalse("banner refresher must not retain slow-projection backoff", turnRefresher.contains("SLOW_BACKOFF"))
        assertTrue("banner refresher must cap each event-drain slice", turnRefresher.contains("TURN_STATE_MAX_PATHS_PER_DRAIN"))
        assertTrue("banner refresher must yield between backlog slices", turnRefresher.contains("TURN_STATE_DRAIN_YIELD_MS"))
        assertTrue("turn projection must log slow Project Controller projection calls", turnBridge.contains("[turn-perf] projection"))
        assertTrue("turn projection must read Project Controller lazily state", turnBridge.contains("subscribeMirrorForFileViaProjectController"))
        assertTrue("turn projection must show Project Controller disconnects", turnBridge.contains("Project Controller disconnected"))
        assertFalse("turn projection must not call the legacy sidecar-capable FFI", turnBridge.contains("agent_doc_turn_projection"))
        assertFalse("banner collection must read cached state", turnProvider.contains("TurnStateBridge.presentationForFile"))
        assertFalse("banner collection must not trigger its own refresh loop", turnProvider.contains("banner-collect"))
        assertFalse("typing debounce report must not probe turn-state just for logging", typingTracker.contains("TurnStateBridge.presentationForFile"))
        assertFalse("CRDT replica manager must not schedule fixed-delay pulls", crdtReplica.contains("scheduleWithFixedDelay"))
        assertFalse("CRDT replica manager must not keep a poller thread", crdtReplica.contains("crdt-replica-poller"))
        assertFalse("Lazily liveness must not be shadowed by a plugin-owner heartbeat", crdtReplica.contains("PLUGIN_OWNER") || crdtReplica.contains("plugin_owner"))
        assertFalse("PatchWatcher must block on WatchService events", patchWatcher.contains("watchService.poll("))
        assertFalse("reload broadcast must not use a polling interval", patchWatcher.contains("LIB_RELOAD_BROADCAST_POLL_MS"))
        assertFalse("PatchWatcher must not use CRDT event sidecars", patchWatcher.contains(".agent-doc/crdt-replica-events"))
        assertTrue(
            "PatchWatcher must wake CRDT drains from the shared typed editor intent",
            patchWatcher.contains("EditorIntent.DeliverCrdtRemote.token"),
        )
        assertTrue(
            "CRDT event protocol states must be parsed into an enum instead of compared as free text",
            patchWatcher.contains("enum class CrdtReplicaEventReason") &&
                patchWatcher.contains("CrdtReplicaEventReason.fromToken") &&
                patchWatcher.contains("CrdtReplicaEventReason.AckRecoveryForceRefresh"),
        )
        assertFalse("layout detector must not run a fallback polling thread", layoutDetector.contains("startFallbackPoll"))
        assertFalse("layout detector must not define a polling interval", layoutDetector.contains("POLL_INTERVAL_MS"))
        assertFalse("visual highlighter must not use a Swing timer", visualHighlighter.contains("Alarm("))
        assertFalse("visual highlighter must not tokenize the editor text on the UI apply path", visualHighlighter.contains("NativePatching.visualTokens(editor.document.text)"))
        assertTrue("visual highlighter tokenization must run on its event worker", visualHighlighter.contains("agent-doc-visual-highlighter-events"))
    }

    @Test
    fun `jetbrains crdt replica transport talks to cpc not supervisor`() {
        val forwarder = Files.readString(
            listOf(
                Paths.get("src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaForwarder.kt"),
                Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaForwarder.kt"),
            ).first { Files.exists(it) },
        )

        assertTrue("CRDT transport must target the Project Controller socket", forwarder.contains(".agent-doc/controller.sock"))
        assertTrue("CRDT transport must use the controller crdt_replica envelope", forwarder.contains("\"crdt_replica\""))
        assertFalse("CRDT transport must not connect to per-session supervisor sockets", forwarder.contains(".agent-doc/supervisor"))
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
