package com.github.btakita.agentdoc

import com.github.btakita.agentdoc.shell.ImplAction
import com.intellij.openapi.actionSystem.*
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.project.DumbAware
import com.intellij.openapi.ui.popup.JBPopupFactory

/**
 * Dumb mode: every agent-doc action, and the popup groups that hold them, is
 * [DumbAware]. The actions read markdown files and call the native library, never
 * the PSI indexes, so "Analyzing project..." must not disable or hide the menu.
 */
internal class DumbAwareGroup(name: String? = null, popup: Boolean = false) :
    DefaultActionGroup(name, popup), DumbAware {
    override fun getActionUpdateThread(): ActionUpdateThread = ActionUpdateThread.BGT
}

/**
 * Shows a popup menu with Agent Doc commands when Ctrl+Shift+Alt+D (rebindable) is pressed in a .md file;
 * the "Default for XWin" keymap also keeps Alt+Space (`altshiftmenu`).
 */
class AgentDocPopupAction : ImplAction {
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
            "AgentDoc.LintDocument",
            "AgentDoc.CancelTurn",
            "AgentDoc.CopySessionDiagnostics",
            "AgentDoc.SyncLayout",
            "AgentDoc.LoadTmuxWindow",
            "AgentDoc.RefreshEnvironment",
            // `gvqv`: project-wide dashboard (work board + controller liveness).
            "AgentDoc.Dashboard",
        )

        internal val OVERFLOW_ACTION_IDS = listOf(
            "AgentDoc.StartAgentDoc",
            // One-time setup for Marketplace users who installed the plugin first.
            "AgentDoc.InstallCli",
            // IdeaVim-style opt-in to the GitHub-hosted early update channel.
            "AgentDoc.UpdateChannel",
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

        /**
         * Popup ids that the modular (exact-262/exact-263) distribution registers only in
         * `agent.doc.frontend`. Under Remote Dev the backend builds this popup from its own
         * `ActionManager`, where these ids are not registered (GH #375): they are omitted from
         * the backend popup and stay reachable from the JetBrains Client's own menus. A
         * single-process IDE loads the frontend module into the same `ActionManager`, so they
         * still resolve there.
         */
        internal val CLIENT_ONLY_ACTION_IDS = setOf(
            // UpdateSettings is process-local; see editors/jetbrains/SPEC.md "Distribution".
            "AgentDoc.UpdateChannel",
        )

        private val LOG = Logger.getInstance(AgentDocPopupAction::class.java)

        /**
         * Resolves popup ids to registered actions, in order. An unregistered id is never
         * passed to `DefaultActionGroup.add` (it rejects null). A known client-only id is
         * dropped quietly; any other unregistered id is reported through [onUnexpectedMissing]
         * and dropped, so one missing registration cannot take the whole popup down.
         */
        internal fun resolveActions(
            ids: List<String>,
            lookup: (String) -> AnAction?,
            onUnexpectedMissing: (String) -> Unit = {},
        ): List<AnAction> = ids.mapNotNull { id ->
            lookup(id) ?: run {
                if (id !in CLIENT_ONLY_ACTION_IDS) onUnexpectedMissing(id)
                null
            }
        }
    }

    override fun actionPerformed(e: AnActionEvent) {
        val editor = e.getData(CommonDataKeys.EDITOR) ?: return
        val actionManager = ActionManager.getInstance()
        val resolve = { ids: List<String> ->
            resolveActions(ids, actionManager::getAction) { id ->
                LOG.warn("Agent Doc popup omits unregistered action $id")
            }
        }

        val group = DumbAwareGroup().apply {
            resolve(PRIMARY_ACTION_IDS).forEach { add(it) }
            addSeparator()
            add(
                DumbAwareGroup("More Actions", true).apply {
                    resolve(OVERFLOW_ACTION_IDS).forEach { add(it) }
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
}
