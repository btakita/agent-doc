package com.github.btakita.agentdoc

import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.actionSystem.CommonDataKeys
import com.intellij.openapi.fileEditor.FileDocumentManager

/**
 * Action that routes the active document through `agent-doc route`.
 *
 * Triggered by Ctrl+Shift+Alt+A (configurable in Keymap settings).
 * Only enabled when the active editor has a .md file open.
 *
 * Saves the document and routes immediately. Manual Run stays intentionally
 * stateless so the editor does not try to infer whether the tmux session is
 * "already running" or otherwise mid-recovery.
 */
class SubmitAction : AnAction() {

    companion object {
        private val LOG = com.intellij.openapi.diagnostic.Logger.getInstance(SubmitAction::class.java)
    }

    override fun actionPerformed(e: AnActionEvent) {
        val project = e.project ?: return
        val file = e.getData(CommonDataKeys.VIRTUAL_FILE) ?: return

        LOG.warn("[run] actionPerformed: ${file.name}")

        FileDocumentManager.getInstance().saveAllDocuments()
        LOG.warn("[run] invoking sendToTerminal immediately: ${file.name}")
        TerminalUtil.sendToTerminal(project, file)
        PromptPoller.getInstance(project).addFile(file)
    }

    override fun update(e: AnActionEvent) {
        val file = e.getData(CommonDataKeys.VIRTUAL_FILE)
        e.presentation.isEnabledAndVisible =
            file != null && file.extension?.lowercase() == "md"
    }
}
