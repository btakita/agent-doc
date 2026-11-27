package com.github.btakita.agentdoc

import com.intellij.openapi.Disposable
import com.intellij.openapi.application.ReadAction
import com.intellij.openapi.editor.Document
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.editor.event.CaretEvent
import com.intellij.openapi.editor.event.CaretListener
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.project.Project

internal fun promptTargetAtCaret(targets: List<DocumentPromptTarget>, line: Int): DocumentPromptTarget? =
    targets.asSequence()
        .filter { target ->
            target.action != DocumentPromptAction.NAVIGATE && line >= target.line && line <= (target.endLine ?: target.line)
        }
        .maxWithOrNull(compareBy(DocumentPromptTarget::depth, DocumentPromptTarget::line))

/** Applies caret changes to the tree for the selected editor and the same document revision. */
internal class RepoContextCaretSync(
    private val project: Project,
    parent: Disposable,
    private val canFollow: () -> Boolean,
    private val selectTarget: (String) -> Unit,
) {
    private data class Projection(val document: Document, val stamp: Long, val outline: RepoContextDocumentOutline)

    private var projection: Projection? = null
    private var queuedEditor: Editor? = null
    private var awaitingOutline = false
    private var openedLine: Int? = null

    init {
        GenerationListeners.addCaretListener(object : CaretListener {
            override fun caretPositionChanged(event: CaretEvent) {
                val editor = event.editor
                if (editor.project === project && (event.caret == null || event.caret == editor.caretModel.primaryCaret)) {
                    queue(editor)
                }
            }
        }, parent)
    }

    fun clear() {
        projection = null
        awaitingOutline = false
        openedLine = null
    }

    fun show(document: Document, stamp: Long, outline: RepoContextDocumentOutline) {
        val changedDocument = projection?.document !== document
        if (changedDocument) openedLine = null
        projection = Projection(document, stamp, outline)
        if (changedDocument || awaitingOutline) followSelectedEditor()
    }

    fun followSelectedEditor() {
        if (!canFollow() || project.isDisposed) return
        ReadAction.compute<Editor?, RuntimeException> {
            FileEditorManager.getInstance(project).selectedTextEditor
        }?.let(::follow)
    }

    fun openFromTree(line: Int, navigate: () -> Unit) {
        openedLine = line
        navigate()
    }

    private fun queue(editor: Editor) {
        // A click can move focus from the tree before the callback runs.
        // The same callback handles any newer caret events from that click.
        val alreadyQueued = queuedEditor != null
        queuedEditor = editor
        if (alreadyQueued) return
        GenerationWork.invokeLater {
            val newest = queuedEditor
            queuedEditor = null
            newest?.let(::follow)
        }
    }

    private fun follow(editor: Editor) {
        if (!canFollow() || project.isDisposed || editor.isDisposed) return
        val current = projection ?: return
        val target = ReadAction.compute<DocumentPromptTarget?, RuntimeException> {
            if (editor.document !== current.document ||
                editor !== FileEditorManager.getInstance(project).selectedTextEditor) return@compute null
            if (current.document.modificationStamp != current.stamp) {
                awaitingOutline = true
                return@compute null
            }
            awaitingOutline = false
            val line = editor.caretModel.primaryCaret.logicalPosition.line + 1
            if (openedLine != null) {
                val fromTree = openedLine == line
                openedLine = null
                if (fromTree) return@compute null
            }
            promptTargetAtCaret(current.outline.targets, line)
        }
        target?.id?.let(selectTarget)
    }
}
