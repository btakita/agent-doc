package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class TmuxPaneFocusSyncTest {
    @Test
    fun `focused editor component outranks background tmux document changes`() {
        assertEquals(
            TmuxFocusMirrorDecision.PreserveFocusedEditor,
            TmuxPaneFocusSync.decideTmuxFocusMirror(
                editorContentFocused = true,
                editorDocumentPath = "/repo/tasks/haiven-websocket-hub-takehome-v2.md",
                tmuxDocumentPath = "/repo/tasks/agent-doc/agent-doc-bugs2.md",
                tmuxDocumentVisible = false,
                tmuxFocusedDocRoot = "/repo",
                editorFocusedDocRoot = "/repo",
            ),
        )
        assertEquals(
            TmuxFocusMirrorDecision.Mirror,
            TmuxPaneFocusSync.decideTmuxFocusMirror(
                editorContentFocused = false,
                editorDocumentPath = "/repo/tasks/haiven-websocket-hub-takehome-v2.md",
                tmuxDocumentPath = "/repo/tasks/agent-doc/agent-doc-bugs2.md",
                tmuxDocumentVisible = false,
                tmuxFocusedDocRoot = "/repo",
                editorFocusedDocRoot = "/repo",
            ),
        )
    }

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
    fun `embedded terminal mirrors a visible document across project roots`() {
        assertEquals(
            TmuxFocusMirrorDecision.Mirror,
            TmuxPaneFocusSync.decideTmuxFocusMirror(
                editorContentFocused = false,
                editorDocumentPath = "/repo/tasks/agent-doc/agent-doc-bugs.md",
                tmuxDocumentPath = "/repo/src/sample-app/tasks/infra.md",
                tmuxDocumentVisible = true,
                tmuxFocusedDocRoot = "/repo/src/sample-app",
                editorFocusedDocRoot = "/repo",
            ),
        )
    }

    @Test
    fun `hidden foreign root tmux document cannot steal editor selection`() {
        assertEquals(
            TmuxFocusMirrorDecision.SuppressHiddenForeignRoot,
            TmuxPaneFocusSync.decideTmuxFocusMirror(
                editorContentFocused = false,
                editorDocumentPath = "/repo/src/sample-app/tasks/infra.md",
                tmuxDocumentPath = "/repo/tasks/agent-doc/agent-doc-bugs2.md",
                tmuxDocumentVisible = false,
                tmuxFocusedDocRoot = "/repo",
                editorFocusedDocRoot = "/repo/src/sample-app",
            ),
        )
    }

    @Test
    fun `tmux focus mirror fires within one project root`() {
        assertEquals(
            TmuxFocusMirrorDecision.Mirror,
            TmuxPaneFocusSync.decideTmuxFocusMirror(
                editorContentFocused = false,
                editorDocumentPath = "/repo/tasks/current.md",
                tmuxDocumentPath = "/repo/tasks/other.md",
                tmuxDocumentVisible = false,
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
            TmuxFocusMirrorDecision.Mirror,
            TmuxPaneFocusSync.decideTmuxFocusMirror(
                editorContentFocused = false,
                editorDocumentPath = null,
                tmuxDocumentPath = "/repo/tasks/other.md",
                tmuxDocumentVisible = false,
                tmuxFocusedDocRoot = "/repo",
                editorFocusedDocRoot = null,
            ),
        )
        assertEquals(
            TmuxFocusMirrorDecision.Mirror,
            TmuxPaneFocusSync.decideTmuxFocusMirror(
                editorContentFocused = false,
                editorDocumentPath = "/repo/tasks/current.md",
                tmuxDocumentPath = "/repo/tasks/other.md",
                tmuxDocumentVisible = false,
                tmuxFocusedDocRoot = null,
                editorFocusedDocRoot = "/repo",
            ),
        )
    }
}
