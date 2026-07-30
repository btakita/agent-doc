package com.github.btakita.agentdoc

import com.intellij.openapi.actionSystem.ActionUpdateThread
import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.actionSystem.CommonDataKeys
import com.intellij.openapi.diagnostic.Logger

/**
 * "Restart Agent" replaces the harness child and re-resolves the current
 * `agent:` frontmatter. It is intentionally separate from
 * [RestartSupervisorProcessAction], which recycles controller code while
 * preserving the child.
 */
class RestartAgentAction : AnAction() {
    private val log = Logger.getInstance(RestartAgentAction::class.java)

    override fun actionPerformed(e: AnActionEvent) {
        val project = e.project ?: return
        val file = e.getData(CommonDataKeys.VIRTUAL_FILE) ?: return
        log.warn("[restart-agent] Restart Agent invoked for ${file.path}")
        TerminalUtil.recordRestartAgentMenuInvoked(project, file)
        TerminalUtil.restartAgentSession(project, file)
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
