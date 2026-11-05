package com.github.btakita.agentdoc

import com.github.btakita.agentdoc.shell.ImplAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.actionSystem.CommonDataKeys
import com.intellij.openapi.command.WriteCommandAction
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.ide.CopyPasteManager
import com.intellij.openapi.project.Project
import com.intellij.openapi.ui.popup.JBPopupFactory
import com.intellij.openapi.ui.popup.ListSeparator
import com.intellij.openapi.ui.popup.PopupStep
import com.intellij.openapi.ui.popup.util.BaseListPopupStep
import java.awt.datatransfer.StringSelection

/**
 * `#treestatusjetbrains`: "Agent Doc: Tree Status" (Alt+Shift+S, rebindable).
 *
 * Asks the native tree-status query (`agent_doc_queue_tree_status_json`) about the queue subtree
 * at the caret line of the live buffer text, shows the answer's `text` verbatim in a popup, and
 * offers one delete quick-fix per `deletable` row (lines `line..=end_line`). The quick-fix
 * deletes only when those lines are still byte-identical to the text the answer was computed
 * from. The binary owns every state and word; this action never derives one.
 */
class TreeStatusAction : ImplAction {
    override fun actionPerformed(e: AnActionEvent) {
        val project = e.project ?: return
        val editor = e.getData(CommonDataKeys.EDITOR) ?: return
        val document = editor.document
        val snapshot = document.text
        val line = document.getLineNumber(editor.caretModel.offset) + 1
        GenerationWork.executeOnPooledThread {
            val result = QueueTreeStatus.query(snapshot, line)
            GenerationWork.invokeLater {
                if (project.isDisposed || editor.isDisposed) return@invokeLater
                when (result) {
                    is TreeStatusQueryResult.Answer -> showPopup(project, editor, snapshot, result.answer)
                    TreeStatusQueryResult.NoQueue -> TerminalUtil.showHint(project, QueueTreeStatus.NO_QUEUE_MESSAGE)
                    is TreeStatusQueryResult.Unavailable ->
                        TerminalUtil.notifyError(project, "Tree Status unavailable: ${result.reason}.")
                }
            }
        }
    }

    override fun update(e: AnActionEvent) {
        val file = e.getData(CommonDataKeys.VIRTUAL_FILE)
        e.presentation.isEnabledAndVisible =
            e.getData(CommonDataKeys.EDITOR) != null && file != null && file.extension?.lowercase() == "md"
    }

    private fun showPopup(project: Project, editor: Editor, snapshot: String, answer: QueueTreeStatusAnswer) {
        val items = QueueTreeStatus.popupItems(answer)
        val firstQuickFix = QueueTreeStatus.firstQuickFix(items)
        val step =
            object : BaseListPopupStep<TreeStatusPopupItem>(QueueTreeStatus.POPUP_TITLE, items) {
                override fun getTextFor(value: TreeStatusPopupItem): String = value.label

                override fun isSelectable(value: TreeStatusPopupItem): Boolean = value.selectable

                override fun getSeparatorAbove(value: TreeStatusPopupItem): ListSeparator? =
                    if (value === firstQuickFix) ListSeparator("Quick fixes") else null

                override fun onChosen(selectedValue: TreeStatusPopupItem, finalChoice: Boolean): PopupStep<*>? {
                    doFinalStep { apply(project, editor, snapshot, answer, selectedValue) }
                    return PopupStep.FINAL_CHOICE
                }
            }
        JBPopupFactory.getInstance().createListPopup(step).showInBestPositionFor(editor)
    }

    private fun apply(
        project: Project,
        editor: Editor,
        snapshot: String,
        answer: QueueTreeStatusAnswer,
        item: TreeStatusPopupItem,
    ) {
        when (item) {
            is TreeStatusPopupItem.StatusLine -> Unit
            TreeStatusPopupItem.CopyText -> CopyPasteManager.getInstance().setContents(StringSelection(answer.text))
            is TreeStatusPopupItem.DeleteRows -> {
                if (project.isDisposed || editor.isDisposed) return
                val document = editor.document
                val range = QueueTreeStatus.verifiedDeleteRange(snapshot, document.immutableCharSequence, item.row)
                if (range == null) {
                    TerminalUtil.showHint(project, QueueTreeStatus.STALE_MESSAGE)
                    return
                }
                WriteCommandAction.runWriteCommandAction(project, "Agent Doc: Delete Queue Rows", null, {
                    // Re-verify inside the write action: nothing may have moved since the check.
                    val verified =
                        QueueTreeStatus.verifiedDeleteRange(snapshot, document.immutableCharSequence, item.row)
                    if (verified == range) document.deleteString(range.first, range.last + 1)
                })
            }
        }
    }
}
