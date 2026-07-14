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

    @Test
    fun `fresh editor focus intent suppresses the previously focused tmux document`() {
        assertEquals(
            EditorFocusIntentDecision.SuppressStaleTmux,
            TmuxPaneFocusSync.decideEditorFocusIntent(
                tmuxDocumentPath = "/repo/tasks/old.md",
                intent = EditorFocusIntent(
                    documentPath = "/repo/tasks/selected.md",
                    expiresAtNanos = 2_000,
                ),
                nowNanos = 1_000,
            ),
        )
    }

    @Test
    fun `tmux arrival at the editor intent is acknowledged without selection echo`() {
        assertEquals(
            EditorFocusIntentDecision.Acknowledge,
            TmuxPaneFocusSync.decideEditorFocusIntent(
                tmuxDocumentPath = "/repo/tasks/selected.md",
                intent = EditorFocusIntent(
                    documentPath = "/repo/tasks/selected.md",
                    expiresAtNanos = 2_000,
                ),
                nowNanos = 1_000,
            ),
        )
    }

    @Test
    fun `expired editor focus intent restores tmux to editor following`() {
        assertEquals(
            EditorFocusIntentDecision.Expired,
            TmuxPaneFocusSync.decideEditorFocusIntent(
                tmuxDocumentPath = "/repo/tasks/other.md",
                intent = EditorFocusIntent(
                    documentPath = "/repo/tasks/selected.md",
                    expiresAtNanos = 2_000,
                ),
                nowNanos = 2_000,
            ),
        )
    }

    @Test
    fun `tmux focus mirror is suppressed across project roots`() {
        // Operator focused on a submodule doc while the superproject's agent-doc
        // window is active must NOT have the editor yanked across roots.
        assertEquals(
            false,
            TmuxPaneFocusSync.shouldMirrorTmuxFocusToEditor(
                tmuxFocusedDocRoot = "/repo",
                editorFocusedDocRoot = "/repo/src/boost-client",
            ),
        )
    }

    @Test
    fun `tmux focus mirror fires within one project root`() {
        assertEquals(
            true,
            TmuxPaneFocusSync.shouldMirrorTmuxFocusToEditor(
                tmuxFocusedDocRoot = "/repo",
                editorFocusedDocRoot = "/repo",
            ),
        )
    }

    @Test
    fun `tmux focus mirror fires when a root is unknown`() {
        // No focused markdown editor (or an unresolvable path) leaves single-project
        // following unchanged.
        assertEquals(
            true,
            TmuxPaneFocusSync.shouldMirrorTmuxFocusToEditor(
                tmuxFocusedDocRoot = "/repo",
                editorFocusedDocRoot = null,
            ),
        )
        assertEquals(
            true,
            TmuxPaneFocusSync.shouldMirrorTmuxFocusToEditor(
                tmuxFocusedDocRoot = null,
                editorFocusedDocRoot = "/repo",
            ),
        )
    }
}
