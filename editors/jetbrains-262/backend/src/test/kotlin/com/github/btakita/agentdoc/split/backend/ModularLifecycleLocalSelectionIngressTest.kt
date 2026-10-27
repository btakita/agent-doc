package com.github.btakita.agentdoc.split.backend

import java.nio.file.Files
import java.nio.file.Path
import java.nio.file.Paths
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/**
 * A monolithic 262/263 IDE runs the modular frontend as a *local* lease. That lease is
 * detached-view transport only (`local_client`): it never becomes pane-layout authority and is
 * never forwarded to `editor_surface_observe`. The classic `EditorTabSyncListener` edges are
 * therefore the only automatic tmux auto-sync source in that topology.
 *
 * The modular backend compiles a generated `AgentDocProjectLifecycle` derived from the classic
 * one (GH #339 R1 moved the body behind the shell's `PluginLifecycleListener`). When that generator stripped the classic selection, focus, and activation edges, a tab
 * switch in a local 2026.3 IDE reached tmux only on an unrelated republish: a native-library
 * reload during `make install`, a Run Agent Doc surface reference, or an editor container change.
 * The operator saw this as a large auto-sync delay that "went away" after each install.
 */
class ModularLifecycleLocalSelectionIngressTest {
    private fun generatedLifecycle(): String {
        val relative =
            "build/generated/agent-doc-backend-sources/com/github/btakita/agentdoc/" +
                "AgentDocProjectLifecycle262Generated.kt"
        val path: Path =
            listOf(
                Paths.get(relative),
                Paths.get("backend", relative),
                Paths.get("editors/jetbrains-262/backend", relative),
            ).firstOrNull { Files.exists(it) }
                ?: error("generated modular AgentDocProjectLifecycle not found; run generateModularLifecycle")
        return Files.readString(path)
    }

    private fun projectOpenedBody(source: String): String {
        val start = source.indexOf("fun projectOpened(project: Project)")
        assertTrue(start >= 0, "generated lifecycle has no projectOpened")
        val end = source.indexOf("fun initializeOpenProjectsAfterDynamicLoad()", start).takeIf { it > start } ?: source.length
        return source.substring(start, end)
    }

    @Test
    fun localTabSelectionReachesTheClassicSurfacePublisher() {
        val body = projectOpenedBody(generatedLifecycle())
        assertEquals(
            1,
            Regex("""val editorTabSync = EditorTabSyncListener\.install\(project\)""").findAll(body).count(),
            body,
        )
        assertTrue(
            Regex(
                """GenerationListeners\.subscribe\(\s*project,\s*FileEditorManagerListener\.FILE_EDITOR_MANAGER,\s*editorTabSync,\s*lifecycle,\s*\)""",
            ).containsMatchIn(body),
            "tab selection must be subscribed to the classic EditorTabSyncListener:\n$body",
        )
    }

    @Test
    fun localSplitFocusAndActivationEdgesStayInstalled() {
        val body = projectOpenedBody(generatedLifecycle())
        assertTrue(body.contains("EditorFocusSyncListener.install(project, editorTabSync)"), body)
        assertTrue(body.contains("editorTabSync.onIdeActivated(project)"), body)
        assertTrue(body.contains("ApplicationActivationListener.TOPIC"), body)
    }

    @Test
    fun oneSidedRemoteDevDiagnosticIsStillInstalledAfterTheClassicEdges() {
        val body = projectOpenedBody(generatedLifecycle())
        val subscription = body.indexOf("FileEditorManagerListener.FILE_EDITOR_MANAGER,")
        val fallback = body.indexOf("BackendGuestSelectionFallback.install(")
        assertTrue(subscription >= 0, body)
        assertTrue(fallback > subscription, "fallback diagnostic must follow the classic edges:\n$body")
    }
}
