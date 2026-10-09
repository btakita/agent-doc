package com.github.btakita.agentdoc.split.frontend

import com.github.btakita.agentdoc.split.SurfaceRole
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNotEquals

class FrontendSurfaceCollectorTest {
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
}
