package com.github.btakita.agentdoc

import com.intellij.openapi.actionSystem.ActionUpdateThread
import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.actionSystem.CommonDataKeys
import com.intellij.openapi.diagnostic.Logger

/**
 * #s81q: a "Stop Agent" menu command that runs `agent-doc session stop-agent`,
 * stopping the harness agent child while keeping the supervisor alive at its
 * restart-or-quit keepalive prompt. Mirrors [RestartAgentAction]; the operator
 * can bring the agent back with "Restart Agent".
 */
class StopAgentAction : AnAction() {
    private val log = Logger.getInstance(StopAgentAction::class.java)

    override fun actionPerformed(e: AnActionEvent) {
        val project = e.project ?: return
        val file = e.getData(CommonDataKeys.VIRTUAL_FILE) ?: return
        log.warn("[stop-agent] Stop Agent invoked for ${file.path}")
        TerminalUtil.stopAgent(project, file)
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
