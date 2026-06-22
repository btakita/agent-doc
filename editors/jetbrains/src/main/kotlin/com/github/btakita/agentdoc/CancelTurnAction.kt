package com.github.btakita.agentdoc

import com.intellij.openapi.actionSystem.ActionUpdateThread
import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.actionSystem.CommonDataKeys
import com.intellij.openapi.diagnostic.Logger

/**
 * A "Cancel Turn" menu command that runs `agent-doc session cancel-turn`,
 * cancelling the currently running turn while leaving the agent harness and its
 * supervisor alive. No-op when the agent is idle, so it never closes the agent.
 * Mirrors [StopAgentAction].
 */
class CancelTurnAction : AnAction() {
    private val log = Logger.getInstance(CancelTurnAction::class.java)

    override fun actionPerformed(e: AnActionEvent) {
        val project = e.project ?: return
        val file = e.getData(CommonDataKeys.VIRTUAL_FILE) ?: return
        log.warn("[cancel-turn] Cancel Turn invoked for ${file.path}")
        TerminalUtil.cancelTurn(project, file)
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
