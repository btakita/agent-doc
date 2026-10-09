package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

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
}
