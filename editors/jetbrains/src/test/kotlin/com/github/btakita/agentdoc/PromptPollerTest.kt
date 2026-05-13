package com.github.btakita.agentdoc

import org.junit.Assert.*
import org.junit.Test

class PromptPollerTest {

    @Test
    fun `parsePromptAllJson accepts flat prompt entries with selected`() {
        val json = """
            [
              {
                "session_id": "abc",
                "file": "tasks/demo.md",
                "active": true,
                "question": "Permission required",
                "options": [
                  {"index": 1, "label": "Allow once"},
                  {"index": 2, "label": "Reject"}
                ],
                "selected": 1
              }
            ]
        """.trimIndent()

        val entries = requireNotNull(parsePromptAllJson(json))

        assertEquals(1, entries.size)
        val entry = entries.single()
        assertEquals("abc", entry.sessionId)
        assertEquals("tasks/demo.md", entry.file)
        assertTrue(entry.info.active)
        assertEquals("Permission required", entry.info.question)
        assertEquals(1, entry.info.selected)
        assertEquals(
            listOf(PromptOption(1, "Allow once"), PromptOption(2, "Reject")),
            entry.info.options,
        )
    }
}
