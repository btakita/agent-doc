package com.github.btakita.agentdoc

import com.intellij.openapi.actionSystem.*
import com.intellij.openapi.project.DumbAware
import com.intellij.openapi.ui.popup.JBPopupFactory

/**
 * Dumb mode: every agent-doc action, and the popup groups that hold them, is
 * [DumbAware]. The actions read markdown files and call the native library, never
 * the PSI indexes, so "Analyzing project..." must not disable or hide the menu.
 */
internal class DumbAwareGroup(name: String? = null, popup: Boolean = false) :
    DefaultActionGroup(name, popup), DumbAware

/**
 * Shows a popup menu with Agent Doc commands when Alt+Enter is pressed in a .md file.
 */
class AgentDocPopupAction : AnAction(), DumbAware {
    companion object {
        internal val PRIMARY_ACTION_IDS = listOf(
            "AgentDoc.Submit",
            "AgentDoc.InitSession",
            "AgentDoc.FixDocument",
            "AgentDoc.Claim",
            "AgentDoc.CompactExchange",
            "AgentDoc.ShowSessionStatus",
            "AgentDoc.RestartSupervisorProcess",
            "AgentDoc.RestartAgent",
            "AgentDoc.CancelTurn",
            "AgentDoc.ClearSessionContext",
            "AgentDoc.InterruptClearSessionContext",
            "AgentDoc.CopySessionDiagnostics",
            "AgentDoc.SyncLayout",
            "AgentDoc.LoadTmuxWindow",
            "AgentDoc.RefreshEnvironment",
        )

        internal val OVERFLOW_ACTION_IDS = listOf(
            "AgentDoc.RunWithJunie",
            "AgentDoc.ForceClaim",
            "AgentDoc.ResyncFixSessions",
            "AgentDoc.GcStaleSessions",
        )
    }

    override fun actionPerformed(e: AnActionEvent) {
        val editor = e.getData(CommonDataKeys.EDITOR) ?: return
        val actionManager = ActionManager.getInstance()

        val group = DumbAwareGroup().apply {
            PRIMARY_ACTION_IDS.forEach { add(actionManager.getAction(it)) }
            addSeparator()
            add(
                DumbAwareGroup("More Actions", true).apply {
                    OVERFLOW_ACTION_IDS.forEach { add(actionManager.getAction(it)) }
                }
            )
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
