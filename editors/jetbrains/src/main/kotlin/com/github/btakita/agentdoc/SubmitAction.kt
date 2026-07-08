package com.github.btakita.agentdoc

import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.actionSystem.CommonDataKeys
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.fileEditor.FileDocumentManager

/**
 * Action that routes the active document through the Project Controller editor_route RPC.
 *
 * Triggered by Ctrl+Shift+Alt+A (configurable in Keymap settings).
 * Only enabled when the active editor has a .md file open.
 *
 * Saves the active document and routes immediately.
 * Manual Run stays intentionally stateless so the editor does not try to infer
 * whether the tmux session is "already running" or otherwise mid-recovery.
 */
class SubmitAction : AnAction() {

    companion object {
        private val LOG = com.intellij.openapi.diagnostic.Logger.getInstance(SubmitAction::class.java)
    }

    override fun actionPerformed(e: AnActionEvent) {
        val project = e.project ?: return
        val file = e.getData(CommonDataKeys.VIRTUAL_FILE) ?: return

        val (cwd, relativePath) = TerminalUtil.resolveProject(project, file)
        LOG.warn("[run] actionPerformed: ${file.name}")
        val attempt = RunAgentDocAttemptLedger.begin(
            cwd = cwd,
            relativePath = relativePath,
            filePath = file.path,
            focusedFile = file.path,
        )

        ApplicationManager.getApplication().invokeLater {
            if (project.isDisposed || !file.isValid) {
                attempt.finishIfCurrent("document_unavailable", error = "project disposed or file invalid")
                return@invokeLater
            }
            if (!attempt.isCurrent()) {
                return@invokeLater
            }
            val fdm = FileDocumentManager.getInstance()
            val document = fdm.getDocument(file)
            if (document != null) {
                attempt.recordIfCurrent("save_active_document")
                fdm.saveDocument(document)
                attempt.recordIfCurrent("active_document_saved")
            } else {
                attempt.recordIfCurrent("document_not_loaded")
        }
        LOG.warn("[run] invoking sendToTerminal after active document save: ${file.name}")
        TerminalUtil.sendToTerminal(project, file, attempt = attempt)
        TurnStateBannerRefresher.getInstance(project).requestRefresh(file, "run-agent-doc")
    }
}

    override fun update(e: AnActionEvent) {
        val file = e.getData(CommonDataKeys.VIRTUAL_FILE)
        e.presentation.isEnabledAndVisible =
            file != null && file.extension?.lowercase() == "md"
    }
}
