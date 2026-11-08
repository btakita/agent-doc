package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.awt.Window

class JetBrainsEditorSurfaceTest {
    @Test
    fun `mount receipt names exactly one detached surface`() {
        val decision = parseTerminalSurfaceDecision(
            """{"terminal_decision":{"kind":"mount","surface_id":"dock-window-7","document":"/repo/tasks/plan.md","previous":{"surface_id":"main","document":"/repo/tasks/plan.md"}}}""",
        )

        assertEquals(TerminalSurfaceDecisionKind.MOUNT, decision?.kind)
        assertEquals("dock-window-7", decision?.surfaceId)
        assertEquals("/repo/tasks/plan.md", decision?.document)
    }

    @Test
    fun `stash receipt has no accidental mount target`() {
        val decision = parseTerminalSurfaceDecision(
            """{"terminal_decision":{"kind":"stash","previous":{"surface_id":"dock-window-7","document":"/repo/tasks/plan.md"}}}""",
        )

        assertEquals(TerminalSurfaceDecisionKind.STASH, decision?.kind)
        assertNull(decision?.surfaceId)
    }

    @Test
    fun `old controller receipt is ignored`() {
        assertNull(parseTerminalSurfaceDecision("""{"intent":{"kind":"idle"},"idle":true}"""))
    }

    @Test
    fun `current detached surface augments lagging window enumeration`() {
        assertEquals(
            setOf("main-frame", "frame-detached"),
            JetBrainsEditorSurfaces.authoritativeLiveSurfaceIds(
                setOf("main-frame"),
                "frame-detached",
            ),
        )
    }

    @Test
    fun `keyboard focus on the frame or an owned child is that project surface`() {
        val frame = Window(null as Window?)
        val child = Window(frame)
        val other = Window(null as Window?)
        assertTrue(JetBrainsEditorSurfaces.windowBelongsToFrame(frame, frame))
        assertTrue(JetBrainsEditorSurfaces.windowBelongsToFrame(child, frame))
        assertFalse(JetBrainsEditorSurfaces.windowBelongsToFrame(other, frame))
        assertFalse(JetBrainsEditorSurfaces.windowBelongsToFrame(null, frame))
        child.dispose()
        other.dispose()
        frame.dispose()
    }

    fun `missing backend windows remain unknown despite a current surface hint`() {
        assertNull(
            JetBrainsEditorSurfaces.authoritativeLiveSurfaceIds(
                null,
                "frame-detached",
            ),
        )
    }
}
