package com.github.btakita.agentdoc

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.nio.file.Files
import java.nio.file.Paths

/**
 * Idle-save File Cache Conflict reloaded the editor from disk and dropped an
 * unmarked operator prompt, which a later projection brought back as a seen
 * (`👀`) row. Reloading that pair must be refused. A quiet document that
 * already shares the unmarked line, and an operator delete that lives only
 * on disk, stay reloadable.
 */
class OperatorPromptReloadVetoTest {
    @Test
    fun `reload is refused when disk lost the unmarked operator prompt`() {
        val memory = session("  - $PROMPT")
        val disk = session("")

        assertTrue(reloadWouldDropUnmarkedOperatorPrompt(memory, disk))
    }

    @Test
    fun `reload is refused when disk has only the seen form of the prompt`() {
        val memory = session("  - $PROMPT")
        val disk = session("  - 👀 $PROMPT")

        assertTrue(reloadWouldDropUnmarkedOperatorPrompt(memory, disk))
    }

    @Test
    fun `reload is allowed when both sides already share the unmarked prompt`() {
        val text = session("  - $PROMPT")

        assertFalse(reloadWouldDropUnmarkedOperatorPrompt(text, text))
    }

    @Test
    fun `reload is allowed when the operator deleted the prompt and disk still has it`() {
        val memory = session("")
        val disk = session("  - $PROMPT")

        assertFalse(reloadWouldDropUnmarkedOperatorPrompt(memory, disk))
    }

    @Test
    fun `reload is allowed for a non-session file with the same list line`() {
        val memory = "# notes\n\n  - $PROMPT\n"
        val disk = "# notes\n\n"

        assertFalse(reloadWouldDropUnmarkedOperatorPrompt(memory, disk))
    }

    @Test
    fun `reload is allowed when only a generated field or structure row differs`() {
        val memory = session("- **status**: open\n  - ▫️ child detail")
        val disk = session("")

        assertFalse(reloadWouldDropUnmarkedOperatorPrompt(memory, disk))
    }

    @Test
    fun `an unreadable disk snapshot still refuses to drop an unmarked prompt`() {
        assertTrue(reloadWouldDropUnmarkedOperatorPrompt(session("  - $PROMPT"), null))
    }

    @Test
    fun `crlf disk text still counts as the same unmarked prompt`() {
        val memory = session("  - $PROMPT")
        val disk = memory.replace("\n", "\r\n")

        assertFalse(reloadWouldDropUnmarkedOperatorPrompt(memory, disk))
    }

    @Test
    fun `the shell refuses the reload and keeps memory only for the save batch`() {
        val vetoer = readPluginSource("shell/ShellOperatorPromptConflict.kt")
        val hold = readPluginSource("FileCacheConflictGuard.kt")
            .substringAfter("internal object AgentDocSaveBatchConflictHold")
            .substringBefore("internal fun fileCacheConflictQueuedIdeaLogLine")
        val extensions = readPluginSource("AgentDocEditorExtensions.kt")
        val descriptor = readPluginResource("META-INF/plugin.xml")

        assertTrue(vetoer.contains("override fun mayReloadFileContent"))
        assertTrue(vetoer.contains("reloadDropsUnmarkedOperatorPrompt("))
        assertTrue(vetoer.contains("return false"))
        assertTrue(vetoer.contains("override fun beforeAllDocumentsSaving()"))
        assertTrue(vetoer.contains("holdAgentDocMemoryAcrossSaveBatch()"))
        assertTrue(hold.contains("overrideConflictResolution"))
        assertTrue(hold.contains("KEEP_MEMORY_CHANGES"))
        assertTrue(hold.contains("GenerationWork.invokeLater"))
        assertTrue(hold.contains("Disposer.dispose"))
        assertTrue(extensions.contains("AgentDocSaveBatchConflictHold.holdIfUnsavedAgentDoc"))
        assertTrue(descriptor.contains("ShellOperatorPromptReloadVetoer"))
        assertTrue(descriptor.contains("ShellOperatorPromptSaveListener"))
    }

    private fun readPluginSource(name: String): String {
        val path = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/$name"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/$name"),
        ).first { Files.exists(it) }
        return Files.readString(path)
    }

    private fun readPluginResource(name: String): String {
        val path = listOf(
            Paths.get("src/main/resources/$name"),
            Paths.get("editors/jetbrains/src/main/resources/$name"),
        ).first { Files.exists(it) }
        return Files.readString(path)
    }

    private fun session(body: String): String = """
        ---
        agent_doc_format: session
        ---
        ## main
        $body
        """.trimIndent() + "\n"

    private companion object {
        const val PROMPT = "Why do we need DOCKERHUB_USERNAME + DOCKERHUB_TOKEN?"
    }
}
