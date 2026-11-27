package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class RepoContextCaretSyncTest {
    @Test
    fun `caret chooses the deepest promptable range covering it`() {
        val document = target("document", 1, 20, 0)
        val heading = target("heading", 2, 20, 1)
        val run = target("run", 4, 9, 2)
        val parent = target("parent", 5, 7, 3)
        val child = target("child", 6, 7, 4)
        val task = target("synthetic task", 6, 7, 5, DocumentPromptAction.NAVIGATE)
        val targets = listOf(document, heading, run, parent, child, task)

        assertEquals(document, promptTargetAtCaret(targets, 1))
        assertEquals(parent, promptTargetAtCaret(targets, 5))
        assertEquals(child, promptTargetAtCaret(targets, 7))
        assertEquals(run, promptTargetAtCaret(targets, 9))
        assertEquals(heading, promptTargetAtCaret(targets, 10))
        assertNull(promptTargetAtCaret(targets, 21))
    }

    private fun target(
        id: String,
        line: Int,
        endLine: Int,
        depth: Int,
        action: DocumentPromptAction = DocumentPromptAction.EXCHANGE,
    ) = DocumentPromptTarget(id, id, id, line, depth, action, endLine = endLine)
}
