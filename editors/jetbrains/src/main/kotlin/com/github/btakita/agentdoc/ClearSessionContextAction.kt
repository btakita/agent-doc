package com.github.btakita.agentdoc

import com.intellij.openapi.actionSystem.ActionUpdateThread
import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.actionSystem.CommonDataKeys
import com.intellij.openapi.diagnostic.Logger

class ClearSessionContextAction : AnAction() {
    companion object {
        private val LOG = Logger.getInstance(ClearSessionContextAction::class.java)
    }

    override fun actionPerformed(e: AnActionEvent) {
        val project = e.project ?: return
        val file = e.getData(CommonDataKeys.VIRTUAL_FILE) ?: return

        // Coalesce a rapid re-fire so a second `/clear` keystroke for this same
        // document doesn't stack up at the terminal layer (#9adk).
        val (cwd, relativePath) = TerminalUtil.resolveProject(project, file)
        val routeKey = RunAgentDocAttemptLedger.routeKey(cwd, relativePath)
        if (!InvocationCoalescer.shouldProceed(
                InvocationCoalescer.key("clear", routeKey),
                System.currentTimeMillis(),
            )
        ) {
            LOG.warn("[clear] coalesced duplicate Clear Session Context for ${file.name} (invocation already pending)")
            return
        }

        TerminalUtil.clearSessionContext(project, file)
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
