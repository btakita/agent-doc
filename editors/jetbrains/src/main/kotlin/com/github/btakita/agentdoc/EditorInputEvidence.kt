package com.github.btakita.agentdoc

import com.intellij.openapi.editor.Document
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.editor.EditorFactory
import com.intellij.openapi.project.Project
import java.awt.EventQueue
import java.awt.KeyboardFocusManager
import java.awt.event.InputEvent
import java.util.concurrent.atomic.AtomicLong
import javax.swing.SwingUtilities

/**
 * Factual, bounded evidence captured at the same edge as an operator [DocumentEvent].
 *
 * This deliberately does not decide whether an edit was intended. A replacement-sized edit can
 * be a selection replacement, an editor action, IdeaVim, or another legitimate input route. The
 * evidence lets a later diagnostic correlate that edit with the focused/writable editor and the
 * exact local-CRDT receipt without turning an unusual edit size into a corruption heuristic.
 *
 * Selection and caret values are post-event values. The exact pre-event affected range is carried
 * separately by the DocumentEvent offset and old length in [CapturedLocalEditorEdit].
 */
internal data class EditorInputEvidence(
    val eventId: Long,
    val focusOwnerClass: String,
    val editorFocused: Boolean,
    val documentWritable: Boolean,
    val postCaretOffset: Int,
    val postSelectionStart: Int,
    val postSelectionEnd: Int,
    val dispatchEventClass: String,
    val dispatchEventId: Int,
    val dispatchModifiers: Int,
)

internal enum class EditorInputEditShape(val wireName: String) {
    Insertion("insertion"),
    Deletion("deletion"),
    Replacement("replacement"),
    Noop("noop"),
}

internal fun editorInputEditShapeUtil(oldLength: Int, newLength: Int): EditorInputEditShape =
    when {
        oldLength > 0 && newLength > 0 -> EditorInputEditShape.Replacement
        oldLength > 0 -> EditorInputEditShape.Deletion
        newLength > 0 -> EditorInputEditShape.Insertion
        else -> EditorInputEditShape.Noop
    }

/**
 * Captures only O(1) document/caret facts plus the editor views of this one Document. It never
 * reads [Document.getText], hashes fragments, waits for another thread, or traverses the project.
 */
internal object EditorInputEvidenceCapture {
    private val nextEventId = AtomicLong(0L)

    fun capture(project: Project, document: Document): EditorInputEvidence {
        val focusOwner = KeyboardFocusManager.getCurrentKeyboardFocusManager().permanentFocusOwner
        val focusedEditor =
            EditorFactory.getInstance()
                .getEditors(document, project)
                .firstOrNull { editor -> focusBelongsToEditor(focusOwner, editor) }
        val dispatchEvent = EventQueue.getCurrentEvent()
        val inputEvent = dispatchEvent as? InputEvent
        return EditorInputEvidence(
            eventId = nextEventId.incrementAndGet(),
            focusOwnerClass = focusOwner?.javaClass?.name ?: "none",
            editorFocused = focusedEditor != null,
            documentWritable = document.isWritable,
            postCaretOffset = focusedEditor?.caretModel?.offset ?: -1,
            postSelectionStart = focusedEditor?.selectionModel?.selectionStart ?: -1,
            postSelectionEnd = focusedEditor?.selectionModel?.selectionEnd ?: -1,
            dispatchEventClass = dispatchEvent?.javaClass?.name ?: "none",
            dispatchEventId = dispatchEvent?.id ?: -1,
            dispatchModifiers = inputEvent?.modifiersEx ?: 0,
        )
    }

    private fun focusBelongsToEditor(focusOwner: java.awt.Component?, editor: Editor): Boolean {
        if (focusOwner == null) return false
        val content = editor.contentComponent
        return focusOwner === content || SwingUtilities.isDescendingFrom(focusOwner, content)
    }
}
