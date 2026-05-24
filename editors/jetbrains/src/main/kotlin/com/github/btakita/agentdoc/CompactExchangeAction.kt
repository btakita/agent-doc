package com.github.btakita.agentdoc

import com.intellij.openapi.actionSystem.ActionUpdateThread
import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.actionSystem.CommonDataKeys
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.fileEditor.FileDocumentManager

class CompactExchangeAction : AnAction() {
    companion object {
        private val LOG = com.intellij.openapi.diagnostic.Logger.getInstance(CompactExchangeAction::class.java)
    }

    override fun actionPerformed(e: AnActionEvent) {
        val project = e.project ?: return
        val file = e.getData(CommonDataKeys.VIRTUAL_FILE) ?: return
        Thread {
            val idle = TypingTracker.awaitIdle(file.path)
            if (!idle) {
                LOG.warn("[compact-exchange] typing debounce timed out; compact deferred for ${file.name}")
                return@Thread
            }
            ApplicationManager.getApplication().invokeLater {
                FileDocumentManager.getInstance().saveAllDocuments()
                TerminalUtil.compactExchange(project, file)
            }
        }.start()
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
