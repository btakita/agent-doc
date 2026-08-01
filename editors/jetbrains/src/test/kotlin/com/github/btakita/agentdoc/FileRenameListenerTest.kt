package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File

class FileRenameListenerTest {
    @Test
    fun `shouldHandleFile is scoped to markdown destinations`() {
        assertTrue(FileRenameListener.shouldHandleFile("plan.md"))
        assertTrue(FileRenameListener.shouldHandleFile("mary-ellen-zellerbach.md"))
        assertFalse(FileRenameListener.shouldHandleFile("main.rs"))
        assertFalse(FileRenameListener.shouldHandleFile("readme.markdown"))
        assertFalse(FileRenameListener.shouldHandleFile(null))
    }

    @Test
    fun `rename and move paths retain the exact old identity`() {
        assertEquals(
            "/work/tasks/mary-elle-zellerbach.md",
            FileRenameListener.oldPathForRename(
                "/work/tasks",
                "mary-elle-zellerbach.md",
            ),
        )
        assertEquals(
            "/work/old/doc.md",
            FileRenameListener.oldPathForMove("/work/old", "doc.md"),
        )
    }

    @Test
    fun `transition identity is deterministic and path-pair scoped`() {
        val first = FileRenameListener.pathTransitionId("/work/old.md", "/work/new.md")
        val repeated = FileRenameListener.pathTransitionId("/work/old.md", "/work/new.md")
        val different = FileRenameListener.pathTransitionId("/work/old.md", "/work/other.md")

        assertEquals(first, repeated)
        assertNotEquals(first, different)
    }

    @Test
    fun `retry projection uses bounded exponential delay`() {
        assertEquals(250L, documentPathTransitionRetryDelayMs(0))
        assertEquals(500L, documentPathTransitionRetryDelayMs(1))
        assertEquals(30_000L, documentPathTransitionRetryDelayMs(20))
    }

    @Test
    fun `rename listener never invokes sync or a layout process`() {
        val source =
            File(
                "src/main/kotlin/com/github/btakita/agentdoc/FileRenameListener.kt",
            ).readText()

        assertFalse(source.contains("ProcessBuilder"))
        assertFalse(source.contains("\"sync\""))
        assertFalse(source.contains("--rename"))
        assertFalse(source.contains("sync_tmux_layout"))
        assertTrue(source.contains("document_path_transition_observe").not())
        assertTrue(source.contains("observeDocumentPathTransition"))
        assertTrue(source.contains("ThreadSafeSourceMap"))
    }
}
