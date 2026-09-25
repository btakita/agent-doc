package com.github.btakita.agentdoc

import com.intellij.openapi.Disposable
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.editor.EditorFactory
import com.intellij.openapi.editor.event.EditorFactoryEvent
import com.intellij.openapi.editor.event.EditorFactoryListener
import com.intellij.openapi.editor.event.EditorMouseEvent
import com.intellij.openapi.editor.event.EditorMouseListener
import com.intellij.openapi.editor.ex.EditorEx
import com.intellij.openapi.editor.ex.FocusChangeListener
import com.intellij.openapi.fileEditor.FileDocumentManager
import com.intellij.openapi.fileEditor.ex.FileEditorManagerEx
import com.intellij.openapi.project.Project
import com.intellij.openapi.util.Disposer
import com.intellij.openapi.vfs.LocalFileSystem
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.openapi.wm.WindowManager
import java.awt.AWTEvent
import java.awt.Component
import java.awt.Container
import java.awt.Toolkit
import java.awt.event.AWTEventListener
import java.awt.event.FocusEvent
import java.awt.event.MouseEvent
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicLong
import java.util.concurrent.atomic.AtomicReference
import javax.swing.SwingUtilities

/**
 * Latest-wins settled selection probe used by editor-tree mouse ingress.
 *
 * IDEA applies a tab/split click over more than one EDT turn. The clicked component cannot be
 * trusted to identify the destination document, so the callback reads the authoritative selected
 * file on the next turn. A newer press or disposal makes every older callback inert.
 */
internal class SettledEditorTreeFocusProbe<T>(
    private val scheduleNextEdt: ((() -> Unit) -> Unit),
    private val selectedValue: () -> T?,
    private val isActive: () -> Boolean,
    private val isEligible: (T) -> Boolean,
    private val emit: (T) -> Unit,
) {
    private val generation = AtomicLong(0)
    private val closed = AtomicBoolean(false)

    fun observeMousePress(belongsToEditorTree: Boolean) {
        if (closed.get() || !belongsToEditorTree) return
        val requestedGeneration = generation.incrementAndGet()
        scheduleNextEdt {
            if (
                closed.get() ||
                    generation.get() != requestedGeneration ||
                    !isActive()
            ) {
                return@scheduleNextEdt
            }
            val selected = selectedValue() ?: return@scheduleNextEdt
            if (isEligible(selected)) emit(selected)
        }
    }

    fun close() {
        closed.set(true)
        generation.incrementAndGet()
    }
}

/**
 * Drives tmux pane focus when the operator moves editor focus between split
 * editor windows (`#panefocussplit`).
 *
 * [EditorTabSyncListener] reconciles the tmux active pane on
 * [com.intellij.openapi.fileEditor.FileEditorManagerListener.selectionChanged],
 * which fires for tab / visible-file-set changes but NOT for focus movement
 * between two already-open split editors. When two agent-doc documents are open
 * side by side (a 2-column editor split mirrored as two co-visible tmux panes),
 * clicking into the other split never fired a reconcile, so the tmux active
 * pane did not follow the editor selection.
 *
 * This listener closes that gap: it attaches per-editor focus listeners, an editor-event mouse
 * listener, and one project-filtered AWT listener covering the entire editor split tree (including
 * tab chrome and custom editors), then routes activation back through the same debounced,
 * generation-guarded reconcile in [EditorTabSyncListener].
 *
 * Thin-plugin contract: this only reports the focus event. All debounce / dedup
 * / tmux-targeting logic stays in [EditorTabSyncListener] and the `agent-doc`
 * CLI, not here.
 */
class EditorFocusSyncListener private constructor(
    private val project: Project,
    private val tabSync: EditorTabSyncListener,
) : Disposable {
    private val disposed = AtomicBoolean(false)
    private val listenerInstalled = AtomicBoolean(false)
    private val editorsRoot = AtomicReference<Container?>(null)

    private val settledTreeFocusProbe =
        SettledEditorTreeFocusProbe<VirtualFile>(
            scheduleNextEdt = { callback ->
                ApplicationManager.getApplication().invokeLater(callback)
            },
            selectedValue = {
                FileEditorManagerEx.getInstanceEx(project).currentWindow?.selectedFile
            },
            isActive = {
                !disposed.get() &&
                    !project.isDisposed &&
                    WindowManager.getInstance().getFrame(project)?.isActive == true
            },
            isEligible = AgentDocSessionFiles::isSessionDocument,
            emit = { file -> tabSync.onEditorFocusGained(project, file) },
        )

    private val editorTreeMouseListener = AWTEventListener { event ->
        if (disposed.get() || project.isDisposed) return@AWTEventListener
        val mouseEvent = event as? MouseEvent ?: return@AWTEventListener
        if (mouseEvent.id != MouseEvent.MOUSE_PRESSED) return@AWTEventListener
        val component = mouseEvent.source as? Component ?: return@AWTEventListener
        val root = editorsRoot.get() ?: return@AWTEventListener
        val belongsToEditorTree =
            component === root || SwingUtilities.isDescendingFrom(component, root)
        settledTreeFocusProbe.observeMousePress(belongsToEditorTree)
    }

    private val focusListener = object : FocusChangeListener {
        override fun focusGained(editor: Editor) = handleEditorActivated(editor)

        override fun focusGained(editor: Editor, event: FocusEvent) = handleEditorActivated(editor)
    }

    private val mouseListener = object : EditorMouseListener {
        override fun mousePressed(event: EditorMouseEvent) = handleEditorActivated(event.editor)
    }

    init {
        val factory = EditorFactory.getInstance()
        // The event multicaster observes every current and future editor. A
        // per-editor mouse listener can miss a restored split when its editor
        // was created during project/plugin startup before attachment completed
        // (`#panefocussplit`). Project filtering remains in the event handler.
        factory.eventMulticaster.addEditorMouseListener(mouseListener, this)
        factory.addEditorFactoryListener(
            object : EditorFactoryListener {
                override fun editorCreated(event: EditorFactoryEvent) = attach(event.editor)
            },
            this,
        )
        // Attach to editors already open when the project (or a hot-reloaded
        // plugin) starts, so focus sync works without reopening files.
        for (editor in factory.allEditors) {
            attach(editor)
        }
        attachEditorTreeMouseListener()
    }

    private fun attachEditorTreeMouseListener() {
        val attach = Runnable {
            if (disposed.get() || project.isDisposed) return@Runnable
            val root = FileEditorManagerEx.getInstanceEx(project).splitters as? Container
                ?: return@Runnable
            editorsRoot.set(root)
            if (listenerInstalled.compareAndSet(false, true)) {
                Toolkit.getDefaultToolkit()
                    .addAWTEventListener(editorTreeMouseListener, AWTEvent.MOUSE_EVENT_MASK)
            }
        }
        if (SwingUtilities.isEventDispatchThread()) {
            attach.run()
        } else {
            ApplicationManager.getApplication().invokeLater(attach)
        }
    }

    private fun attach(editor: Editor) {
        val owner = editor.project
        if (owner != null && owner != project) return
        val editorEx = editor as? EditorEx ?: return
        editorEx.addFocusListener(focusListener, this)
    }

    private fun handleEditorActivated(editor: Editor) {
        if (project.isDisposed) return
        if (editor.project != project) return
        val file = FileDocumentManager.getInstance().getFile(editor.document) ?: return
        tabSync.onEditorFocusGained(project, file)
    }

    override fun dispose() {
        if (!disposed.compareAndSet(false, true)) return
        settledTreeFocusProbe.close()
        if (!listenerInstalled.compareAndSet(true, false)) {
            editorsRoot.set(null)
            return
        }
        val detach = Runnable {
            try {
                Toolkit.getDefaultToolkit().removeAWTEventListener(editorTreeMouseListener)
            } finally {
                editorsRoot.set(null)
            }
        }
        if (SwingUtilities.isEventDispatchThread()) {
            detach.run()
        } else {
            ApplicationManager.getApplication().invokeLater(detach)
        }
    }

    companion object {
        private val instances = ConcurrentHashMap<Project, EditorFocusSyncListener>()

        fun install(project: Project, tabSync: EditorTabSyncListener) {
            instances.computeIfAbsent(project) { EditorFocusSyncListener(project, tabSync) }
        }

        /**
         * Route an explicit document-scoped focus request through the same command-plane lane as
         * an editor activation. Notification actions know the owning document even when a generic
         * IDE terminal tab is currently showing a different tmux pane (`#jbfocusdocroute`).
         */
        fun routeDocumentFocus(project: Project, documentPath: String): Boolean {
            if (project.isDisposed) return false
            val file = LocalFileSystem.getInstance().findFileByPath(documentPath) ?: return false
            EditorTabSyncListener.install(project).onEditorFocusGained(project, file)
            return true
        }

        fun disposeProject(project: Project) {
            instances.remove(project)?.let { Disposer.dispose(it) }
        }
    }
}
