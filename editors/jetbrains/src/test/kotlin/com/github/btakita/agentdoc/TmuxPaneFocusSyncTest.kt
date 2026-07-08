package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class TmuxPaneFocusSyncTest {
    @Test
    fun `focus state extracts document only from active agent doc window`() {
        val json = """
            {
              "active": true,
              "reason": "focused_agent_doc_actor",
              "window_name": "agent-doc",
              "pane_id": "%7",
              "document_id": "/repo/tasks/doc.md"
            }
        """.trimIndent()

        assertEquals("agent-doc", TmuxPaneFocusSync.extractWindowNameFromFocusState(json))
        assertEquals("/repo/tasks/doc.md", TmuxPaneFocusSync.extractDocumentPathFromFocusState(json))
    }

    @Test
    fun `focus state outside agent doc window has no document`() {
        val json = """
            {
              "active": false,
              "reason": "outside_agent_doc_window",
              "window_name": "shell",
              "pane_id": "%7"
            }
        """.trimIndent()

        assertEquals("shell", TmuxPaneFocusSync.extractWindowNameFromFocusState(json))
        assertNull(TmuxPaneFocusSync.extractDocumentPathFromFocusState(json))
    }

    @Test
    fun `focus receipt exposes focused boolean and reason`() {
        val json = """
            {
              "focused": false,
              "reason": "outside_agent_doc_window",
              "document_id": "/repo/tasks/doc.md",
              "pane_id": "%7"
            }
        """.trimIndent()

        assertEquals(false, TmuxPaneFocusSync.focusReceiptFocused(json))
        assertEquals("outside_agent_doc_window", TmuxPaneFocusSync.focusReceiptReason(json))
    }

    @Test
    fun `unchanged tmux document is not selected again`() {
        assertEquals(
            false,
            TmuxPaneFocusSync.shouldSelectTmuxDocument(
                "/repo/tasks/haiven.md",
                "/repo/tasks/haiven.md",
            ),
        )
        assertEquals(
            true,
            TmuxPaneFocusSync.shouldSelectTmuxDocument(
                "/repo/tasks/sitscape.md",
                "/repo/tasks/haiven.md",
            ),
        )
    }
}
