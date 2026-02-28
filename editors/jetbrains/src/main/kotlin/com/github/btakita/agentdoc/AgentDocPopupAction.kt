package com.github.btakita.agentdoc

import com.intellij.codeInsight.daemon.impl.HighlightInfo
import com.intellij.openapi.actionSystem.*
import com.intellij.openapi.editor.impl.DocumentMarkupModel
import com.intellij.openapi.ui.popup.JBPopupFactory

/**
 * Shows a popup menu with Agent Doc commands when Alt+Enter is pressed in a .md file.
 *
 * Self-disabling: when the daemon code analyzer has highlights at the caret
 * (e.g. spellcheck quick-fixes), this action disables itself so IDEA's
 * ShowIntentionActions wins Alt+Enter. When no highlights exist, this action
 * is the sole Alt+Enter candidate and shows the Agent Doc popup.
 */
class AgentDocPopupAction : AnAction() {

    override fun actionPerformed(e: AnActionEvent) {
        val editor = e.getData(CommonDataKeys.EDITOR) ?: return

        val group = DefaultActionGroup().apply {
            add(ActionManager.getInstance().getAction("AgentDoc.Submit"))
            add(ActionManager.getInstance().getAction("AgentDoc.Claim"))
            addSeparator()
            add(ActionManager.getInstance().getAction("AgentDoc.SyncLayout"))
        }

        val popup = JBPopupFactory.getInstance()
            .createActionGroupPopup(
                "Agent Doc",
                group,
                e.dataContext,
                JBPopupFactory.ActionSelectionAid.NUMBERING,
                true
            )

        popup.showInBestPositionFor(editor)
    }

    override fun update(e: AnActionEvent) {
        val file = e.getData(CommonDataKeys.VIRTUAL_FILE)
        if (file == null || file.extension?.lowercase() != "md") {
            e.presentation.isEnabledAndVisible = false
            return
        }

        // Disable when intentions/quick-fixes exist at the caret so
        // ShowIntentionActions handles Alt+Enter instead.
        val editor = e.getData(CommonDataKeys.EDITOR)
        val project = e.project
        if (editor != null && project != null) {
            val offset = editor.caretModel.offset
            val markupModel = DocumentMarkupModel.forDocument(
                editor.document, project, false
            )
            if (markupModel != null) {
                for (highlighter in markupModel.allHighlighters) {
                    if (offset in highlighter.startOffset..highlighter.endOffset) {
                        val tooltip = highlighter.errorStripeTooltip
                        if (tooltip is HighlightInfo) {
                            e.presentation.isEnabledAndVisible = false
                            return
                        }
                    }
                }
            }
        }

        e.presentation.isEnabledAndVisible = true
    }

    override fun getActionUpdateThread(): ActionUpdateThread {
        return ActionUpdateThread.BGT
    }
}
