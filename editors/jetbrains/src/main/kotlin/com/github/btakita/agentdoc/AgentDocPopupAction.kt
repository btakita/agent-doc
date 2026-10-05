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
 * Shows a popup menu with Agent Doc commands when Ctrl+Shift+Alt+D (rebindable) is pressed in a .md file;
 * the "Default for XWin" keymap also keeps Alt+Space (`altshiftmenu`).
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
            // Keep the routine, non-interrupting context reset on the first
            // numbered page. Position nine is the last single-key selection.
            "AgentDoc.ClearSessionContext",
            "AgentDoc.CancelTurn",
            "AgentDoc.CopySessionDiagnostics",
            "AgentDoc.SyncLayout",
            "AgentDoc.LoadTmuxWindow",
            "AgentDoc.RefreshEnvironment",
            // `gvqv`: project-wide dashboard (work board + controller liveness).
            "AgentDoc.Dashboard",
        )

        internal val OVERFLOW_ACTION_IDS = listOf(
            "AgentDoc.RunWithJunie",
            "AgentDoc.ForceClaim",
            // The explicit interrupting clear remains behind More Actions.
            "AgentDoc.InterruptClearSessionContext",
            "AgentDoc.ResyncFixSessions",
            "AgentDoc.GcStaleSessions",
            // #gh116: every declared AgentDoc.* action is reachable from the popup.
            "AgentDoc.StopAgent",
            "AgentDoc.KillSupervisor",
            // `editoractionmenu`: which plugin/CLI/native library is running.
            "AgentDoc.About",
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
