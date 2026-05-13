package com.github.btakita.agentdoc

import org.junit.Assert.*
import org.junit.Test
import java.io.File
import java.nio.file.Files

class PromptPollerTest {

    @Test
    fun `parsePromptAllJson accepts flat prompt entries with selected`() {
        val json = """
            [
              {
                "session_id": "abc",
                "file": "tasks/demo.md",
                "cwd": "/work/demo",
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
        assertEquals("/work/demo", entry.cwd)
        assertTrue(entry.info.active)
        assertEquals("Permission required", entry.info.question)
        assertEquals(1, entry.info.selected)
        assertEquals(
            listOf(PromptOption(1, "Allow once"), PromptOption(2, "Reject")),
            entry.info.options,
        )
    }

    @Test
    fun `buildPromptAnswerCommand sends one based option position`() {
        assertEquals(
            listOf("/bin/agent-doc", "prompt", "--answer", "2", "tasks/demo.md"),
            buildPromptAnswerCommand("/bin/agent-doc", 2, "tasks/demo.md"),
        )
    }

    @Test
    fun `runPromptAnswerCommand runs in prompt owning cwd`() {
        val cwd = Files.createTempDirectory("agent-doc-prompt-cwd-").toFile()
        val output = File.createTempFile("agent-doc-prompt-answer-", ".txt")
        val script = File.createTempFile("agent-doc-prompt-answer-", ".sh").apply {
            writeText(
                """
                |#!/usr/bin/env bash
                |printf '%s\n' "${'$'}PWD" "${'$'}@" > "${output.absolutePath}"
                |""".trimMargin()
            )
            setExecutable(true)
        }

        try {
            val result = runPromptAnswerCommand(
                agentDoc = script.absolutePath,
                cwd = cwd.absolutePath,
                relativePath = "tasks/demo.md",
                optionIndex = 2,
            )

            assertEquals(0, result.exitCode)
            val lines = output.readLines()
            assertEquals(cwd.absolutePath, lines[0])
            assertEquals(listOf("prompt", "--answer", "2", "tasks/demo.md"), lines.drop(1))
        } finally {
            script.delete()
            output.delete()
            cwd.deleteRecursively()
        }
    }
}
