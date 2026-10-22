package com.github.btakita.agentdoc.split.backend

import com.intellij.openapi.client.ClientType
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
    fun fallbackResumesTheProjectionBeforeForwardingASelection() {
        val delivery =
            source()
                .substringAfter("ApplicationManager.getApplication().invokeLater {")
                .substringBefore("FileEditorManagerEvent(")
        val fence = delivery.indexOf("ingress.frontendAuthorityActive.value")
        val resume = delivery.indexOf("listener.resumeControllerProjection()")
        val forward = delivery.indexOf("listener.selectionChanged(")
        assertTrue(fence >= 0 && resume >= 0 && forward >= 0, delivery)
        assertTrue(
            fence < resume,
            "resume only after the effect-time fence proves no frontend snapshot is authoritative",
        )
        assertTrue(resume < forward, "the forwarded selection must publish as a live frame")
    }

    @Test
    fun remoteDevControllerSelectionCarriesFocusEvidence() {
        val source = source()
        assertTrue(
            source.contains(
                "remoteControllerSelectionFocusEvidence =\n" +
                    "                                        BackendRemoteSessionGate.selectionProvesFocus(session.type)",
            ),
            source,
        )
    }

    @Test
    fun missingFrontendDiagnosticNamesTheWorkingTwoSidedInstallRemedy() {
        assertEquals(5_000L, FRONTEND_AUTHORITY_DIAGNOSTIC_DELAY_MS)
        val diagnostic = frontendAuthorityUnavailableDiagnostic("client-1", ClientType.CONTROLLER)
        assertTrue(diagnostic.contains("frontend layout publisher unavailable"), diagnostic)
        assertTrue(diagnostic.contains("reason=flat_evidence_has_no_column_geometry"), diagnostic)
        assertTrue(diagnostic.contains("JetBrains Client/Gateway"), diagnostic)
        assertTrue(diagnostic.contains("Remote Dev backend"), diagnostic)
        assertTrue(diagnostic.contains("restart both"), diagnostic)
    }
}
