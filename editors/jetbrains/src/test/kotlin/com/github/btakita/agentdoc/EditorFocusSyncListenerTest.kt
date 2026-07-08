package com.github.btakita.agentdoc

import org.junit.Assert.assertTrue
import org.junit.Test
import java.nio.file.Files
import java.nio.file.Paths

class EditorFocusSyncListenerTest {
    @Test
    fun `split editor activation is driven by mouse presses as well as focus gained`() {
        val listenerPath = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/EditorFocusSyncListener.kt"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/EditorFocusSyncListener.kt"),
        ).first { Files.exists(it) }
        val listener = Files.readString(listenerPath)

        assertTrue(listener.contains("EditorMouseListener"))
        assertTrue(listener.contains("override fun mousePressed(event: EditorMouseEvent)"))
        assertTrue(listener.contains("handleEditorActivated(event.editor)"))
        assertTrue(listener.contains("addEditorMouseListener(mouseListener"))
    }
}
