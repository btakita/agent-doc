package com.github.btakita.agentdoc

import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.actionSystem.CommonDataKeys
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.fileEditor.FileDocumentManager

/**
 * Action that routes the active document through the Project Controller editor_route RPC.
 *
 * Triggered by Ctrl+Shift+Alt+A (configurable in Keymap settings). Only enabled when the active
 * editor has a .md file open.
 *
 * Saves the active document and routes immediately. Manual Run stays intentionally stateless so the
 * editor does not try to infer whether the tmux session is "already running" or otherwise
 * mid-recovery.
 */
class SubmitAction : AnAction() {

    companion object {
        private val LOG =
            com.intellij.openapi.diagnostic.Logger.getInstance(SubmitAction::class.java)
    }

    override fun actionPerformed(e: AnActionEvent) {
        val project = e.project ?: return
        val file = e.getData(CommonDataKeys.VIRTUAL_FILE) ?: return

        LOG.warn("[run] actionPerformed: ${file.name}")
        val fileDocumentManager = FileDocumentManager.getInstance()
        val document = fileDocumentManager.getDocument(file)
        val saveStage =
            if (document == null) {
                "document_not_loaded"
            } else {
                // IntelliJ Local History can throw an AssertionError while saving
                // a valid document when its private store is corrupt. Saving is
                // best-effort for dispatch; log the failure and continue.
                try {
                    fileDocumentManager.saveDocument(document)
                    "active_document_saved"
                } catch (failure: Throwable) {
                    LOG.warn(
                        "[run] saveDocument failed; continuing dispatch: ${failure.message}",
                        failure,
                    )
                    "active_document_save_failed"
                }
            }

        ApplicationManager.getApplication().executeOnPooledThread {
            if (project.isDisposed || !file.isValid) return@executeOnPooledThread
            val resolved =
                try {
                    TerminalUtil.resolveProject(project, file)
                } catch (failure: Throwable) {
                    LOG.warn("[run] project resolution failed for ${file.path}", failure)
                    return@executeOnPooledThread
                }
            val (cwd, relativePath) = resolved
            val attempt =
                RunAgentDocAttemptLedger.begin(
                    cwd = cwd,
                    relativePath = relativePath,
                    filePath = file.path,
                    focusedFile = file.path,
                )
            attempt.recordIfCurrent(saveStage)
            ApplicationManager.getApplication().invokeLater {
                if (project.isDisposed || !file.isValid) {
                    attempt.finishIfCurrent(
                        "document_unavailable",
                        error = "project disposed or file invalid",
                    )
                    return@invokeLater
                }
                if (!attempt.isCurrent()) return@invokeLater
                LOG.warn("[run] invoking sendToTerminal after active document save: ${file.name}")
                TerminalUtil.sendToTerminal(
                    project,
                    file,
                    attempt = attempt,
                    resolved = resolved,
                )
                TurnStateBannerRefresher.getInstance(project).requestRefresh(file, "run-agent-doc")
            }
        }
    }

    override fun update(e: AnActionEvent) {
        val file = e.getData(CommonDataKeys.VIRTUAL_FILE)
        e.presentation.isEnabledAndVisible = file != null && file.extension?.lowercase() == "md"
    }
}
