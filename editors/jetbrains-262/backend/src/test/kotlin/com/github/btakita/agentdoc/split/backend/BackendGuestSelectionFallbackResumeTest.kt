package com.github.btakita.agentdoc.split.backend

import java.nio.file.Files
import java.nio.file.Path
import java.nio.file.Paths
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/**
 * `#surfaceretireformal`: the Remote Dev sibling of `#tmuxautosyncretired`. A frontend handoff
 * retires the backend listener's frames (`retireControllerProjection`); when the frontend goes away
 * the backend fallback resumes on the SAME frames at the SAME per-process connection generation,
 * which the controller fences forever unless the listener knows it is live again and re-registers
 * (model: `formal/tla/PassiveTmuxSyncRetireNet.tla`, action `Fallback`; wedge
 * `PassiveTmuxSyncRetireNetFallbackMuteWedge`).
 */
class BackendGuestSelectionFallbackResumeTest {
    private fun source(): String {
        val relative =
            "src/main/kotlin/com/github/btakita/agentdoc/split/backend/BackendGuestSelectionFallback.kt"
        val path: Path =
            listOf(
                Paths.get(relative),
                Paths.get("backend", relative),
                Paths.get("editors/jetbrains-262/backend", relative),
            ).firstOrNull { Files.exists(it) } ?: error("BackendGuestSelectionFallback.kt not found")
        return Files.readString(path)
    }

    @Test
    fun oneSidedInstallDoesNotLinkPrivateClientSessionApis() {
        val source = source()
        assertTrue(source.contains("Marketplace-compatible split frontend is required"), source)
        assertTrue(!source.contains("ClientFileEditorManager"), source)
        assertTrue(!source.contains("RemoteClientSessionListener"), source)
    }

    @Test
    fun missingFrontendDiagnosticNamesTheWorkingTwoSidedInstallRemedy() {
        assertEquals(5_000L, FRONTEND_AUTHORITY_DIAGNOSTIC_DELAY_MS)
        val diagnostic = frontendAuthorityUnavailableDiagnostic("client-1", "CONTROLLER")
        assertTrue(diagnostic.contains("frontend layout publisher unavailable"), diagnostic)
        assertTrue(diagnostic.contains("readiness=blocked"), diagnostic)
        assertTrue(
            diagnostic.contains("readiness_reason=frontend_authority_absent_or_generation_mismatched"),
            diagnostic,
        )
        assertTrue(diagnostic.contains("geometry_reason=flat_evidence_has_no_column_geometry"), diagnostic)
        assertTrue(diagnostic.contains("structural_routing=blocked fallback=unavailable"), diagnostic)
        assertTrue(diagnostic.contains("JetBrains Client/Gateway"), diagnostic)
        assertTrue(diagnostic.contains("Remote Dev backend"), diagnostic)
        assertTrue(diagnostic.contains("verify both sides report that exact plugin version"), diagnostic)
        assertTrue(diagnostic.contains("restart both"), diagnostic)
        assertTrue(diagnostic.contains("including the JetBrains Client/Gateway process"), diagnostic)
        assertTrue(
            diagnostic.contains(
                "restarting only the Remote Dev backend cannot replace its already-loaded frontend module",
            ),
            diagnostic,
        )
    }
}
