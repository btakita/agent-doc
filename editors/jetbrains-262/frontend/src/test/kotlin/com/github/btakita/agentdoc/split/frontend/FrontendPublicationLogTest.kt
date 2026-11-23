package com.github.btakita.agentdoc.split.frontend

import com.github.btakita.agentdoc.split.EditorWindowBounds
import com.github.btakita.agentdoc.split.EditorWindowSnapshot
import com.github.btakita.agentdoc.split.FrontendSurface
import com.github.btakita.agentdoc.split.FrontendSurfaceSnapshot
import com.github.btakita.agentdoc.split.SurfaceIngressAck
import com.github.btakita.agentdoc.split.SurfaceIngressStatus
import com.github.btakita.agentdoc.split.SurfaceRole
import com.intellij.platform.project.ProjectId
import fleet.util.UID
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

/**
 * GH #435: with the frontend module loaded in a Coder JetBrains Client, the backend still had no
 * measured geometry, and neither idea.log said which hop dropped it. A refusal the backend can
 * outgrow now republishes on a bounded backoff, and the Client logs the first acceptance, each
 * refusal change, and the acceptance that clears a refusal.
 */
class FrontendPublicationLogTest {
    @Test
    fun recoverableRefusalsRepublishOnABoundedBackoff() {
        listOf(
            SurfaceIngressStatus.DELIVERY_FAILED,
            SurfaceIngressStatus.PROJECT_MISMATCH,
            SurfaceIngressStatus.INCOMPLETE,
        ).forEach { status ->
            assertEquals(1_000L, frontendSnapshotRetryDelayMillis(status, 0), status.name)
            assertEquals(2_000L, frontendSnapshotRetryDelayMillis(status, 1), status.name)
            assertEquals(30_000L, frontendSnapshotRetryDelayMillis(status, 50), status.name)
        }
    }

    @Test
    fun refusalsAnotherCaptureCannotFixWaitForTheNextEditorEvent() {
        listOf(
            SurfaceIngressStatus.STALE_LEASE,
            SurfaceIngressStatus.STALE_SEQUENCE,
            SurfaceIngressStatus.INVALID_SCHEMA,
            SurfaceIngressStatus.INVALID_MAIN_CARDINALITY,
            SurfaceIngressStatus.INVALID_SURFACE,
            SurfaceIngressStatus.ACCEPTED,
        ).forEach { status -> assertNull(frontendSnapshotRetryDelayMillis(status, 0), status.name) }
    }

    @Test
    fun logsFirstAcceptanceRefusalChangesAndRecoveryOnly() {
        val log = FrontendPublicationLog(connectionGeneration = 7)
        val snapshot = snapshot()

        val first = assertNotNull(log.accepted(ack(SurfaceIngressStatus.ACCEPTED), snapshot))
        assertTrue(first.contains("snapshot accepted generation=7"), first)
        assertTrue(first.contains("measured_windows=1/2"), first)
        assertTrue(first.contains("first=true after_refusal=none"), first)
        assertNull(log.accepted(ack(SurfaceIngressStatus.ACCEPTED), snapshot), "steady acceptance is quiet")

        val busy = ack(SurfaceIngressStatus.DELIVERY_FAILED, "Timed out waiting for 2000 ms")
        val refused = assertNotNull(log.refused(busy, snapshot, 1_000L))
        assertTrue(refused.contains("snapshot rejected status=DELIVERY_FAILED"), refused)
        assertTrue(refused.contains("retry_in_ms=1000"), refused)
        assertTrue(refused.contains("diagnostic=Timed out waiting for 2000 ms"), refused)
        assertNull(log.refused(busy, snapshot, 2_000L), "a repeated refusal is counted, not relogged")
        val stale = assertNotNull(log.refused(ack(SurfaceIngressStatus.STALE_LEASE, "stale"), snapshot, null))
        assertTrue(stale.contains("retry_in_ms=none(next_editor_event)"), stale)
        assertTrue(stale.contains("previous_repeats=1"), stale)

        val recovered = assertNotNull(log.accepted(ack(SurfaceIngressStatus.ACCEPTED), snapshot))
        assertTrue(recovered.contains("first=false after_refusal=STALE_LEASE:stale"), recovered)
    }

    private fun ack(status: SurfaceIngressStatus, diagnostic: String? = null) =
        SurfaceIngressAck(status, connectionGeneration = 7, acceptedSequence = null, diagnostic = diagnostic)

    private fun snapshot() = FrontendSurfaceSnapshot(
        frontendInstanceId = "frontend-1",
        sequence = 3,
        projectId = ProjectId(UID.random()),
        complete = true,
        surfaces = listOf(
            FrontendSurface(
                paneId = "root",
                surfaceGeneration = 1,
                role = SurfaceRole.MAIN,
                focused = true,
                windows = listOf(
                    EditorWindowSnapshot(0, "/w/a.md", listOf("/w/a.md"), listOf("/w/a.md"))
                        .copy(bounds = EditorWindowBounds(0, 0, 100, 100)),
                    EditorWindowSnapshot(1, "/w/b.md", listOf("/w/b.md"), listOf("/w/b.md")),
                ),
            ),
        ),
    )
}
