package com.github.btakita.agentdoc

import com.intellij.openapi.actionSystem.ActionUpdateThread
import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.actionSystem.CommonDataKeys
import com.intellij.openapi.diagnostic.Logger

/**
 * #restartagentmenu (Phase 2 of the restart-agent feature): a "Restart Agent"
 * menu command that runs the same `agent-doc restart-supervisor` path as
 * [RestartSupervisorProcessAction]. After Phase 1 (#agentreloadrestart),
 * `restart-supervisor` re-resolves the harness from current frontmatter, so a
 * manual "Restart Agent" brings up a changed `agent:` harness.
 *
 * This is intentionally distinct from "Restart Supervisor Process": they share
 * the same CLI today, but the operator-facing intent differs (restart the agent
 * harness child vs. restart the bound supervisor process).
 */
class RestartAgentAction : AnAction() {
    private val log = Logger.getInstance(RestartAgentAction::class.java)

    override fun actionPerformed(e: AnActionEvent) {
        val project = e.project ?: return
        val file = e.getData(CommonDataKeys.VIRTUAL_FILE) ?: return
        log.warn("[restart-agent] Restart Agent invoked for ${file.path}")
        TerminalUtil.recordRestartAgentMenuInvoked(project, file)
        TerminalUtil.restartSession(project, file)
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
