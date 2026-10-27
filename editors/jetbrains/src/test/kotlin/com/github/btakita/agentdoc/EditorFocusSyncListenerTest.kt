package com.github.btakita.agentdoc

import com.intellij.openapi.progress.ProcessCanceledException
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
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
        // GH #339 R1 Tier A: staged through the shell forwarder for the multicaster.
        assertTrue(listener.contains("GenerationListeners.addEditorMouseListener(mouseListener, this)"))
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

    @Test
    fun `install before the editor split tree exists defers attachment instead of throwing`() {
        // On IntelliJ 263, projectOpened can run before
        // FileEditorManagerImpl.mainSplitters is assigned, and getSplitters() throws this.
        var splittersBuilt = false
        val attached = mutableListOf<String>()
        val attachment =
            DeferredEditorTreeAttachment(
                resolveRoot = {
                    if (!splittersBuilt) {
                        throw UninitializedPropertyAccessException(
                            "lateinit property mainSplitters has not been initialized",
                        )
                    }
                    "editor-splitters"
                },
                isActive = { true },
                attach = attached::add,
            )

        assertFalse("install must not attach before the split tree exists", attachment.tryAttach())
        assertFalse(attachment.isAttached)
        assertTrue(attached.isEmpty())

        // The first editor event (fileOpened / selectionChanged) arrives after the platform has
        // built the split tree, and the retry attaches exactly once.
        splittersBuilt = true
        assertTrue(attachment.tryAttach())
        assertTrue(attachment.tryAttach())
        assertEquals(listOf("editor-splitters"), attached)
    }

    @Test
    fun `deferred editor-tree attachment is inert after disposal or for an inactive project`() {
        val attached = mutableListOf<String>()
        var active = false
        val attachment =
            DeferredEditorTreeAttachment(
                resolveRoot = { "editor-splitters" },
                isActive = { active },
                attach = attached::add,
            )

        assertFalse(attachment.tryAttach())
        active = true
        attachment.close()
        assertFalse(attachment.tryAttach())
        assertTrue(attached.isEmpty())
    }

    @Test
    fun `split tree readiness maps only the not-yet-built state to null`() {
        assertNull(
            EditorSplittersReadiness.resolveOrNull<String> {
                throw UninitializedPropertyAccessException("lateinit property mainSplitters has not been initialized")
            },
        )
        assertEquals("root", EditorSplittersReadiness.resolveOrNull { "root" })
        assertThrows(IllegalStateException::class.java) {
            EditorSplittersReadiness.resolveOrNull<String> { throw IllegalStateException("real failure") }
        }
    }

    @Test
    fun `a failed focus-sync start is discarded and reported without escaping project startup`() {
        val discarded = mutableListOf<String>()
        val reported = mutableListOf<Throwable>()
        val failure = IllegalStateException("start failed")

        val started =
            startIsolated(
                create = { "listener" },
                start = { throw failure },
                discard = discarded::add,
                report = reported::add,
            )

        assertNull("a failed start must not be recorded as installed", started)
        assertEquals(listOf("listener"), discarded)
        assertEquals(listOf<Throwable>(failure), reported)
        assertEquals(
            "listener",
            startIsolated(create = { "listener" }, start = {}, discard = discarded::add, report = reported::add),
        )
    }

    @Test
    fun `platform cancellation still propagates out of an isolated start`() {
        val discarded = mutableListOf<String>()
        assertThrows(ProcessCanceledException::class.java) {
            startIsolated(
                create = { "listener" },
                start = { throw ProcessCanceledException() },
                discard = discarded::add,
                report = { throw AssertionError("cancellation must not be reported as a failure") },
            )
        }
        assertEquals(listOf("listener"), discarded)
    }

    @Test
    fun `listener waits for an editor event before touching the split tree and isolates install`() {
        val listenerPath = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/EditorFocusSyncListener.kt"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/EditorFocusSyncListener.kt"),
        ).first { Files.exists(it) }
        val listener = Files.readString(listenerPath)
        val body = listener.substringAfter("class EditorFocusSyncListener private constructor")

        assertTrue(body.contains("if (!editorTreeAttachment.tryAttach()) awaitEditorSplitters()"))
        assertTrue(body.contains("FileEditorManagerListener.FILE_EDITOR_MANAGER"))
        assertTrue(body.contains("override fun fileOpened(source: FileEditorManager, file: VirtualFile)"))
        assertTrue(body.contains("override fun selectionChanged(event: FileEditorManagerEvent)"))
        // GH #339 R1 Tier A: the readiness subscription is staged under a disposable it releases.
        assertTrue(body.contains("readinessConnection.getAndSet(null)?.let(Disposer::dispose)"))
        // Every read of the split tree goes through the readiness guard.
        val unguardedReads = Regex("""\.splitters as\? Container""").findAll(body).count()
        val guardedReads =
            Regex("""EditorSplittersReadiness\.resolveOrNull \{\s*FileEditorManagerEx""").findAll(body).count() +
                Regex("""resolveRoot = \{ FileEditorManagerEx""").findAll(body).count()
        assertEquals("every splitters read must be readiness-guarded", unguardedReads, guardedReads)
        // The constructor no longer registers anything; install starts it inside the isolation.
        assertTrue(!body.contains("    init {"))
        assertTrue(body.contains("startIsolated("))
        assertTrue(body.contains("start = { it.start() }"))
        assertTrue(body.contains("discard = { Disposer.dispose(it) }"))
        assertTrue(body.contains("LOG.error("))
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
