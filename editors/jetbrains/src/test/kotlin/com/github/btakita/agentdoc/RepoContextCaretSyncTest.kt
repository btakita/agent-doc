package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class RepoContextCaretSyncTest {
    @Test
    fun `caret chooses the deepest source row covering it`() {
        val document = target("document", 1, 20, 0)
        val heading = target("heading", 2, 20, 1)
        val run = target("run", 4, 9, 2)
        val parent = target("parent", 5, 7, 3)
        val child = target("child", 6, 7, 4)
        val task = target("prompt-task:receipt", 6, 7, 5, DocumentPromptAction.NAVIGATE)
        val answer = target("prompt-answer:receipt", 6, 7, 6, DocumentPromptAction.NAVIGATE)
        val targets = listOf(document, heading, run, parent, child, task, answer)

        assertEquals(document, documentTargetAtCaret(targets, 1))
        assertEquals(parent, documentTargetAtCaret(targets, 5))
        assertEquals(child, documentTargetAtCaret(targets, 7))
        assertEquals(run, documentTargetAtCaret(targets, 9))
        assertEquals(heading, documentTargetAtCaret(targets, 10))
        assertNull(documentTargetAtCaret(targets, 21))
    }

    @Test
    fun `caret selects a navigation-only row within Run`() {
        val run = target("run", 4, 9, 1, DocumentPromptAction.QUEUE)
        val followUp = target("follow-up", 6, 6, 2, DocumentPromptAction.NAVIGATE)

        assertEquals(followUp, documentTargetAtCaret(listOf(run, followUp), 6))
    }

    private fun target(
        id: String,
        line: Int,
        endLine: Int,
        depth: Int,
        action: DocumentPromptAction = DocumentPromptAction.EXCHANGE,
    ) = DocumentPromptTarget(id, id, id, line, depth, action, endLine = endLine)
}
