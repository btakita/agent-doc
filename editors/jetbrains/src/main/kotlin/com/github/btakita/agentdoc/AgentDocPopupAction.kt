package com.github.btakita.agentdoc

import com.intellij.openapi.actionSystem.*
import com.intellij.openapi.ui.popup.JBPopupFactory

/**
 * Shows a popup menu with Agent Doc commands when Alt+Enter is pressed in a .md file.
 * This replaces IntentionAction-based registration which doesn't reliably activate
 * for Markdown files (ShowIntentionsPass language filtering + DaemonCodeAnalyzer issues).
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
        e.presentation.isEnabledAndVisible =
            file != null && file.extension?.lowercase() == "md"
    }

    override fun getActionUpdateThread(): ActionUpdateThread {
        return ActionUpdateThread.BGT
    }
}
