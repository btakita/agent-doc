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

    @Test
    fun `focus session classification acquires IntelliJ model read access`() {
        val sessionFilesPath = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/SyncLayoutAction.kt"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/SyncLayoutAction.kt"),
        ).first { Files.exists(it) }
        val sessionFiles = Files.readString(sessionFilesPath)
            .substringAfter("internal object AgentDocSessionFiles")
            .substringBefore("class SyncLayoutAction")

        val readAction = sessionFiles.indexOf("ReadAction.compute<Boolean, RuntimeException>")
        val documentLookup = sessionFiles.indexOf("FileDocumentManager.getInstance().getDocument(file)")
        val documentRead = sessionFiles.indexOf("document.charsSequence")

        assertTrue("session classification must enter a read action", readAction >= 0)
        assertTrue("document lookup must happen inside the read action", documentLookup > readAction)
        assertTrue("document text must be read inside the read action", documentRead > readAction)
    }
}
