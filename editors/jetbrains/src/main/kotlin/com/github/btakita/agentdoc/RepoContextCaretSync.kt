package com.github.btakita.agentdoc

import com.intellij.openapi.Disposable
import com.intellij.openapi.application.ReadAction
import com.intellij.openapi.editor.Document
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.editor.event.CaretEvent
import com.intellij.openapi.editor.event.CaretListener
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.project.Project

internal fun documentTargetAtCaret(
    targets: List<DocumentPromptTarget>,
    line: Int,
): DocumentPromptTarget? {
    val matches = targets.filter { target -> line in target.line..(target.endLine ?: target.line) }
    val sourceLines = matches.filterNot { it.isPromptHistory() }.mapTo(mutableSetOf()) { it.line }
    return matches.asSequence()
        .filterNot { it.isPromptHistory() && it.line in sourceLines }
        .maxWithOrNull(compareBy(DocumentPromptTarget::depth, DocumentPromptTarget::line))
}

private fun DocumentPromptTarget.isPromptHistory(): Boolean =
    id.startsWith("prompt-task:") || id.startsWith("prompt-answer:")

/** Applies caret changes to the tree for the selected editor and the same document revision. */
internal class RepoContextCaretSync(
    private val project: Project,
    parent: Disposable,
    private val canFollow: () -> Boolean,
    private val canFollowOnOpen: () -> Boolean,
    private val selectTarget: (String) -> Unit,
) {
    private data class Projection(val document: Document, val stamp: Long, val outline: RepoContextDocumentOutline)

    private var projection: Projection? = null
    private var queuedEditor: Editor? = null
    private var awaitingOutline = false
    private var openedLine: Int? = null
    private var openingEditor: Editor? = null

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
        cancelOpeningFollow()
    }

    fun show(document: Document, stamp: Long, outline: RepoContextDocumentOutline) {
        val changedDocument = projection?.document !== document
        if (changedDocument) openedLine = null
        projection = Projection(document, stamp, outline)
        val opening = openingEditor
        if (opening != null) follow(opening, opening = true)
        else if (changedDocument || awaitingOutline) followSelectedEditor()
    }

    fun openAtCaret(editor: Editor) {
        openedLine = null
        openingEditor = editor
        follow(editor, opening = true)
    }

    fun cancelOpeningFollow() {
        openingEditor = null
    }

    fun followSelectedEditor() {
        if (!canFollow() || project.isDisposed) return
        ReadAction.compute<Editor?, RuntimeException> {
            FileEditorManager.getInstance(project).selectedTextEditor
        }?.let(::follow)
    }

    fun openFromTree(line: Int, navigate: () -> Unit) {
        cancelOpeningFollow()
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

    private fun follow(editor: Editor, opening: Boolean = false) {
        if (!opening && openingEditor != null) return
        val allowed = if (opening) canFollowOnOpen() else canFollow()
        if (!allowed || project.isDisposed || editor.isDisposed) {
            if (opening) cancelOpeningFollow()
            return
        }
        val current = projection ?: return
        val target = ReadAction.compute<DocumentPromptTarget?, RuntimeException> {
            if (editor.document !== current.document ||
                (!opening && editor !== FileEditorManager.getInstance(project).selectedTextEditor)) return@compute null
            if (current.document.modificationStamp != current.stamp) {
                awaitingOutline = true
                return@compute null
            }
            if (opening) cancelOpeningFollow()
            awaitingOutline = false
            val line = editor.caretModel.primaryCaret.logicalPosition.line + 1
            if (openedLine != null) {
                val fromTree = openedLine == line
                openedLine = null
                if (fromTree) return@compute null
            }
            documentTargetAtCaret(current.outline.targets, line)
        }
        target?.id?.let(selectTarget)
    }
}
