package com.github.btakita.agentdoc

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.nio.file.Files
import java.nio.file.Paths

/** Regression coverage for the no-null-surface hot-reload handoff contract. */
class EditorSurfaceHotReloadContinuityTest {
    private fun source(name: String): String =
        Files.readString(
            listOf(
                Paths.get("src/main/kotlin/com/github/btakita/agentdoc/$name"),
                Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/$name"),
            ).first { Files.exists(it) },
        )

    @Test
    fun `only terminal editor lifecycle transitions retire controller projections`() {
        assertTrue(EditorSurfaceShutdownPolicy.RetireControllerProjection.retireControllerProjection)
        assertFalse(EditorSurfaceShutdownPolicy.PreserveForDynamicReplacement.retireControllerProjection)
    }

    @Test
    fun `dynamic plugin replacement preserves the acknowledged surface until republish`() {
        val lifecycle = source("AgentDocProjectLifecycle.kt")
        val replacementCleanup =
            lifecycle
                .substringAfter("fun disposeOpenProjectsForDynamicUnload(): Int")
                .substringBefore("internal fun disposeProjectResources(")

        assertTrue(
            "replacement cleanup must select the preserving transition",
            replacementCleanup.contains("EditorSurfaceShutdownPolicy.PreserveForDynamicReplacement"),
        )
        assertFalse(
            "replacement cleanup must not use the terminal default",
            replacementCleanup.contains("projects.forEach(::disposeProjectResources)"),
        )
        assertTrue(
            "replacement initialization must immediately seed the surviving editor surface",
            lifecycle.contains("editorTabSync.onIdeActivated(project)"),
        )
    }

    @Test
    fun `native library reload keeps socket surface authority and republishes before gate release`() {
        val coordinator = source("NativeReloadCoordinator.kt")
        val handoff =
            coordinator
                .substringAfter("fun requestReload(trigger: String")
                .substringBeforeLast("    }")
        val republish = handoff.indexOf("onEditorLayoutChanged(project)")
        val release = handoff.indexOf("reloadGate.complete(handoff)", republish)

        assertFalse(
            "native library handoff must not retire controller-owned surface authority",
            handoff.contains("forgetEditorSurface") || handoff.contains("forgetEditorFocus"),
        )
        assertTrue("native reload must republish its settled surface", republish >= 0)
        assertTrue("surface republish must be queued before the handoff gate opens", release > republish)
    }
}
