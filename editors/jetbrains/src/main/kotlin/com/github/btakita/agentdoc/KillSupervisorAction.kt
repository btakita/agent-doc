package com.github.btakita.agentdoc

import com.intellij.openapi.actionSystem.ActionUpdateThread
import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.actionSystem.CommonDataKeys
import com.intellij.openapi.diagnostic.Logger

/**
 * #s81q: a "Kill Supervisor" menu command that runs
 * `agent-doc admin kill-supervisor <document>`, stopping the whole route-owned
 * supervisor process for this document. The CLI refuses to kill the caller's own
 * ancestor, so this is safe to run from an editor terminal (not the supervisor's
 * own pane). Mirrors [RestartAgentAction] / [StopAgentAction].
 */
class KillSupervisorAction : AnAction() {
    private val log = Logger.getInstance(KillSupervisorAction::class.java)

    override fun actionPerformed(e: AnActionEvent) {
        val project = e.project ?: return
        val file = e.getData(CommonDataKeys.VIRTUAL_FILE) ?: return
        log.warn("[kill-supervisor] Kill Supervisor invoked for ${file.path}")
        TerminalUtil.killSupervisor(project, file)
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
