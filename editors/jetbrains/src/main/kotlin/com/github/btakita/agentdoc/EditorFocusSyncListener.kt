package com.github.btakita.agentdoc

import com.intellij.openapi.Disposable
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.editor.EditorFactory
import com.intellij.openapi.editor.event.EditorFactoryEvent
import com.intellij.openapi.editor.event.EditorFactoryListener
import com.intellij.openapi.editor.event.EditorMouseEvent
import com.intellij.openapi.editor.event.EditorMouseListener
import com.intellij.openapi.editor.ex.EditorEx
import com.intellij.openapi.editor.ex.FocusChangeListener
import com.intellij.openapi.fileEditor.FileDocumentManager
import com.intellij.openapi.project.Project
import com.intellij.openapi.util.Disposer
import java.awt.event.FocusEvent
import java.util.concurrent.ConcurrentHashMap

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
 * This listener closes that gap: it attaches per-editor focus and mouse
 * listeners and routes editor activation back through the same debounced,
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
    private val focusListener = object : FocusChangeListener {
        override fun focusGained(editor: Editor) = handleEditorActivated(editor)

        override fun focusGained(editor: Editor, event: FocusEvent) = handleEditorActivated(editor)
    }

    private val mouseListener = object : EditorMouseListener {
        override fun mousePressed(event: EditorMouseEvent) = handleEditorActivated(event.editor)
    }

    init {
        val factory = EditorFactory.getInstance()
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
    }

    private fun attach(editor: Editor) {
        val owner = editor.project
        if (owner != null && owner != project) return
        val editorEx = editor as? EditorEx ?: return
        editorEx.addFocusListener(focusListener, this)
        editorEx.addEditorMouseListener(mouseListener, this)
    }

    private fun handleEditorActivated(editor: Editor) {
        if (project.isDisposed) return
        val file = FileDocumentManager.getInstance().getFile(editor.document) ?: return
        tabSync.onEditorFocusGained(project, file)
    }

    override fun dispose() {}

    companion object {
        private val instances = ConcurrentHashMap<Project, EditorFocusSyncListener>()

        fun install(project: Project, tabSync: EditorTabSyncListener) {
            instances.computeIfAbsent(project) { EditorFocusSyncListener(project, tabSync) }
        }

        fun disposeProject(project: Project) {
            instances.remove(project)?.let { Disposer.dispose(it) }
        }
    }
}
