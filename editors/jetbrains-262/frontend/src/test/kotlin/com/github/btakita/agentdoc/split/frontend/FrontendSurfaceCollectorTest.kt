package com.github.btakita.agentdoc.split.frontend

import com.github.btakita.agentdoc.split.EditorWindowBounds
import com.github.btakita.agentdoc.split.EditorWindowSnapshot
import com.github.btakita.agentdoc.split.FrontendLayoutColumn
import com.github.btakita.agentdoc.split.SurfaceIngressLease
import com.github.btakita.agentdoc.split.SurfaceRole
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertNotEquals
import kotlin.test.assertNull
import kotlinx.coroutines.flow.emptyFlow
import kotlinx.coroutines.flow.flowOf
import kotlinx.coroutines.runBlocking
import javax.swing.JPanel

class FrontendSurfaceCollectorTest {
    @Test
    fun snapshotTimingWindowsAreExplicitAndDoNotDefineAuthority() {
        assertEquals(75L, FRONTEND_CAPTURE_DEBOUNCE_MS)
        assertEquals(5_000L, FRONTEND_SCHEMA_NEGOTIATION_TIMEOUT_MS)
    }

    @Test
    fun emptyIngressCompletionRetriesWithBoundedBackoff() {
        assertEquals(1_000L, frontendIngressRetryDelayMillis(0))
        assertEquals(2_000L, frontendIngressRetryDelayMillis(1))
        assertEquals(4_000L, frontendIngressRetryDelayMillis(2))
        assertEquals(16_000L, frontendIngressRetryDelayMillis(4))
        assertEquals(30_000L, frontendIngressRetryDelayMillis(5))
        assertEquals(30_000L, frontendIngressRetryDelayMillis(100))
        assertEquals(1_000L, frontendIngressRetryDelayMillis(-1))
    }

    @Test
    fun emptyIngressFlowRetriesAndAcceptsTheNextLease() = runBlocking {
        var attempts = 0
        val sleeps = mutableListOf<Long>()

        assertFailsWith<StopIngressTest> {
            maintainFrontendIngress(
                openIngress = {
                    attempts++
                    if (attempts == 1) emptyFlow() else flowOf(SurfaceIngressLease("lease-2", 2))
                },
                onLease = { throw StopIngressTest() },
                onNormalCompletion = { _, _ -> },
                sleep = { sleeps += it },
            )
        }

        assertEquals(2, attempts)
        assertEquals(listOf(1_000L), sleeps)
    }

    @Test
    fun terminalCommandUsesOnlyControllerProvidedSession() {
        assertEquals(
            listOf("tmux", "attach-session", "-t", "agent-doc-view-bound"),
            Exact262DetachedPresentationAdapter.terminalCommand("agent-doc-view-bound"),
        )
    }

    @Test
    fun exactWindowSelectionRejectsMissingAndAmbiguousSplits() {
        assertEquals(1, exactWindowIndex(listOf(setOf("a.md"), setOf("b.md")), "b.md"))
        assertNull(exactWindowIndex(listOf(setOf("a.md"), setOf("b.md")), "c.md"))
        assertNull(exactWindowIndex(listOf(setOf("a.md"), setOf("a.md")), "a.md"))
    }

    @Test
    fun rootIsMainAndDetachedPaneIsNotPromotedByFocus() {
        assertEquals(SurfaceRole.MAIN, FrontendSurfaceCollector.roleForPaneId("root"))
        assertEquals(SurfaceRole.DETACHED, FrontendSurfaceCollector.roleForPaneId("dock-window-7"))
    }

    @Test
    fun mainFrameReplacementFencesReusedRootPaneId() {
        val tracker = FrontendSurfaceIdentityTracker()
        val firstFrame = Any()
        val replacementFrame = Any()

        val first = tracker.assign(setOf(firstFrame), firstFrame, complete = true).getValue(firstFrame)
        val stable = tracker.assign(setOf(firstFrame), firstFrame, complete = true).getValue(firstFrame)
        val replacement = tracker.assign(
            setOf(replacementFrame),
            replacementFrame,
            complete = true,
        ).getValue(replacementFrame)

        assertEquals("root", first.paneId)
        assertEquals("root", replacement.paneId)
        assertEquals(first.surfaceGeneration, stable.surfaceGeneration)
        assertNotEquals(first.surfaceGeneration, replacement.surfaceGeneration)
    }

    @Test
    fun disappearanceAndReappearanceFencesSamePaneObject() {
        val tracker = FrontendSurfaceIdentityTracker()
        val frame = Any()
        val first = tracker.assign(setOf(frame), null, complete = true).getValue(frame)

        tracker.assign(emptySet(), null, complete = true)
        val reopened = tracker.assign(setOf(frame), null, complete = true).getValue(frame)

        assertNotEquals(first.surfaceGeneration, reopened.surfaceGeneration)
        assertNotEquals(first.paneId, reopened.paneId)
    }

    @Test
    fun incompleteCaptureCannotRetireMissingFrame() {
        val tracker = FrontendSurfaceIdentityTracker()
        val frame = Any()
        val first = tracker.assign(setOf(frame), null, complete = true).getValue(frame)

        tracker.assign(emptySet(), null, complete = false)
        val retained = tracker.assign(setOf(frame), null, complete = true).getValue(frame)

        assertEquals(first, retained)
    }

    /** Schema 5 carries the activation receipt; older peers receive only known fields. */
    @Test
    fun snapshotSchemaNegotiationFallsBackToSchemaOneForAnOldBackend() {
        assertEquals(1, negotiatedSnapshotSchema(null))
        assertEquals(1, negotiatedSnapshotSchema(1))
        assertEquals(2, negotiatedSnapshotSchema(2))
        assertEquals(3, negotiatedSnapshotSchema(3))
        assertEquals(4, negotiatedSnapshotSchema(4))
        assertEquals(5, negotiatedSnapshotSchema(5))
        assertEquals(5, negotiatedSnapshotSchema(9))
        assertEquals(1, negotiatedSnapshotSchema(0))
    }

    @Test
    fun frontendAuthorsColumnMembershipAndOrderFromPositiveGeometryOnly() {
        fun window(path: String, x: Int, y: Int) = EditorWindowSnapshot(
            ordinal = 0,
            selectedPath = path,
            openPaths = listOf(path),
            visiblePaths = listOf(path),
            bounds = EditorWindowBounds(x, y, 600, 400),
        )
        assertEquals(
            listOf(
                FrontendLayoutColumn(listOf("/repo/a.md", "/repo/b.md")),
                FrontendLayoutColumn(listOf("/repo/c.md")),
            ),
            frontendLayoutColumns(
                listOf(
                    window("/repo/c.md", 640, 40),
                    window("/repo/b.md", 4, 500),
                    window("/repo/a.md", 0, 40),
                ),
            ),
        )
        assertNull(frontendLayoutColumns(listOf(window("/repo/a.md", 0, 0), window("/repo/b.md", 0, 0))))
    }

    @Test
    fun permanentFocusOwnerSelectsTheRightEditorWindowOnly() {
        val left = JPanel()
        val right = JPanel()
        val rightEditor = JPanel().also(right::add)
        assertEquals(
            "/repo/right.md",
            focusedEditorPath(
                rightEditor,
                listOf(left to "/repo/left.md", right to "/repo/right.md"),
            ),
        )
        assertNull(
            focusedEditorPath(
                JPanel(),
                listOf(left to "/repo/left.md", right to "/repo/right.md"),
            ),
        )
    }

    @Test
    fun unlaidOutWindowHasNoMeasuredBounds() {
        assertEquals(EditorWindowBounds(640, 40, 600, 800), editorWindowBounds(640, 40, 600, 800))
        assertNull(editorWindowBounds(0, 0, 0, 800))
        assertNull(editorWindowBounds(0, 0, 600, 0))
    }

    @Test
    fun closeOrRejoinMarksMountedSurfaceForRestoration() {
        val mounted = setOf(SurfaceIncarnation("detached-1", 4), SurfaceIncarnation("detached-2", 7))
        val live = setOf(SurfaceIncarnation("detached-2", 7))

        assertEquals(setOf(SurfaceIncarnation("detached-1", 4)), mountedSurfaceKeysToRestore(mounted, live))
    }
}

private class StopIngressTest : RuntimeException()
