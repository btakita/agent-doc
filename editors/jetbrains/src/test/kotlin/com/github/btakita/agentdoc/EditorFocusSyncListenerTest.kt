package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import java.nio.file.Files
import java.nio.file.Paths

class EditorFocusSyncListenerTest {
    @Test
    fun `split editor activation observes mouse and global focus ingress`() {
        val listenerPath = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/EditorFocusSyncListener.kt"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/EditorFocusSyncListener.kt"),
        ).first { Files.exists(it) }
        val listener = Files.readString(listenerPath)

        assertTrue(listener.contains("EditorMouseListener"))
        assertTrue(listener.contains("override fun mousePressed(event: EditorMouseEvent)"))
        assertTrue(listener.contains("handleEditorActivated(event.editor)"))
        assertTrue(listener.contains("factory.eventMulticaster.addEditorMouseListener(mouseListener, this)"))
        assertTrue(listener.contains("if (editor.project != project) return"))
        assertTrue(!listener.contains("editorEx.addEditorMouseListener"))
        assertTrue(
            listener.contains("AWTEvent.MOUSE_EVENT_MASK or AWTEvent.FOCUS_EVENT_MASK"),
        )
        assertTrue(listener.contains("is FocusEvent"))
        assertTrue(listener.contains("event.id != FocusEvent.FOCUS_GAINED"))
        assertTrue(listener.contains("SwingUtilities.isDescendingFrom(component, root)"))
        assertTrue(
            listener.contains("FileEditorManagerEx.getInstanceEx(project).splitters as? Container"),
        )
        assertTrue(listener.contains("KeyboardFocusManager.getCurrentKeyboardFocusManager()"))
        assertTrue(listener.contains("permanentFocusOwner"))
        assertTrue(listener.contains("IdeFocusManager.getInstance(project).doWhenFocusSettlesDown"))
        assertTrue(!listener.contains("scheduleNextEdt"))
        assertTrue(listener.contains("removeAWTEventListener(editorTreeMouseListener)"))
    }

    @Test
    fun `editor-tree click emits focus-settled selected file rather than stale pre-click file`() {
        val callbacks = mutableListOf<() -> Unit>()
        val emitted = mutableListOf<String>()
        var selected = "left.md"
        val probe = focusProbe(callbacks, { selected }, emitted)

        probe.observeMousePress(belongsToEditorTree = true)
        selected = "right.md"
        callbacks.removeFirst().invoke()

        assertEquals(listOf("right.md"), emitted)
    }

    @Test
    fun `outside-tree and inactive editor clicks are inert`() {
        val callbacks = mutableListOf<() -> Unit>()
        val emitted = mutableListOf<String>()
        var active = true
        val probe = focusProbe(callbacks, { "right.md" }, emitted) { active }

        probe.observeMousePress(belongsToEditorTree = false)
        assertTrue(callbacks.isEmpty())

        probe.observeMousePress(belongsToEditorTree = true)
        active = false
        callbacks.removeFirst().invoke()
        assertTrue(emitted.isEmpty())
    }

    @Test
    fun `newer editor-tree click supersedes an older queued probe`() {
        val callbacks = mutableListOf<() -> Unit>()
        val emitted = mutableListOf<String>()
        var selected = "left.md"
        val probe = focusProbe(callbacks, { selected }, emitted)

        probe.observeMousePress(belongsToEditorTree = true)
        selected = "right.md"
        probe.observeMousePress(belongsToEditorTree = true)
        callbacks.removeFirst().invoke()
        callbacks.removeFirst().invoke()

        assertEquals(listOf("right.md"), emitted)
    }

    @Test
    fun `dispose fences a queued editor-tree probe`() {
        val callbacks = mutableListOf<() -> Unit>()
        val emitted = mutableListOf<String>()
        val probe = focusProbe(callbacks, { "right.md" }, emitted)

        probe.observeMousePress(belongsToEditorTree = true)
        probe.close()
        callbacks.removeFirst().invoke()

        assertTrue(emitted.isEmpty())
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

    @Test
    fun `input-required notification routes its document through editor focus command plane`() {
        val sourceRoot = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc"),
        ).first { Files.exists(it) }
        val notification = Files.readString(sourceRoot.resolve("TurnStateBannerRefresher.kt"))
        val focusListener = Files.readString(sourceRoot.resolve("EditorFocusSyncListener.kt"))

        assertTrue(
            "notification action must retain the document identity when focusing the terminal",
            notification.contains("EditorFocusSyncListener.routeDocumentFocus(project, filePath)"),
        )
        assertTrue(
            "explicit notification focus must reuse the document-specific command-plane path",
            focusListener.contains("onEditorFocusGained(project, file)"),
        )
    }

    private fun focusProbe(
        callbacks: MutableList<() -> Unit>,
        selected: () -> String?,
        emitted: MutableList<String>,
        active: () -> Boolean = { true },
    ): SettledEditorTreeFocusProbe<String> =
        SettledEditorTreeFocusProbe(
            scheduleWhenFocusSettles = callbacks::add,
            selectedValue = selected,
            isActive = active,
            isEligible = { it.endsWith(".md") },
            emit = emitted::add,
        )
}
