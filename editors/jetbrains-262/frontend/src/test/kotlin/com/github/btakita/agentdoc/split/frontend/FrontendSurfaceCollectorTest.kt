package com.github.btakita.agentdoc.split.frontend

import com.github.btakita.agentdoc.split.SurfaceRole
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNotEquals
import kotlin.test.assertNull

class FrontendSurfaceCollectorTest {
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

    @Test
    fun closeOrRejoinMarksMountedSurfaceForRestoration() {
        val mounted = setOf(SurfaceIncarnation("detached-1", 4), SurfaceIncarnation("detached-2", 7))
        val live = setOf(SurfaceIncarnation("detached-2", 7))

        assertEquals(setOf(SurfaceIncarnation("detached-1", 4)), mountedSurfaceKeysToRestore(mounted, live))
    }
}
