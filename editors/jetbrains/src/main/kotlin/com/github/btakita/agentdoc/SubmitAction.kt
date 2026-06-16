package com.github.btakita.agentdoc

import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.actionSystem.CommonDataKeys
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.fileEditor.FileDocumentManager

/**
 * Action that routes the active document through `agent-doc route`.
 *
 * Triggered by Ctrl+Shift+Alt+A (configurable in Keymap settings).
 * Only enabled when the active editor has a .md file open.
 *
 * Waits for active typing to settle, then saves the document and routes.
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

        // Coalesce a rapid re-fire (auto-loop tick racing a manual click, or a
        // double key-chord) so a second `/agent-doc` keystroke for this same
        // document doesn't stack up at the terminal layer (#9adk).
        val (cwd, relativePath) = TerminalUtil.resolveProject(project, file)
        val routeKey = RunAgentDocAttemptLedger.routeKey(cwd, relativePath)
        if (!InvocationCoalescer.shouldProceed(
                InvocationCoalescer.key("run", routeKey),
                System.currentTimeMillis(),
            )
        ) {
            LOG.warn("[run] coalesced duplicate Run Agent Doc for ${file.name} (invocation already pending)")
            return
        }

        LOG.warn("[run] actionPerformed: ${file.name}")
        val attempt = RunAgentDocAttemptLedger.begin(
            cwd = cwd,
            relativePath = relativePath,
            filePath = file.path,
            focusedFile = file.path,
        )

        Thread {
            attempt.recordIfCurrent("await_typing_idle")
            val idle = TypingTracker.awaitIdle(file.path)
            if (!idle) {
                LOG.warn("[run] typing debounce timed out; deferring route until typing settles for ${file.name}")
                attempt.finishIfCurrent("typing_idle_timeout", error = "mtime did not settle")
                return@Thread
            }
            attempt.recordIfCurrent("typing_idle")
            ApplicationManager.getApplication().invokeLater {
                FileDocumentManager.getInstance().saveAllDocuments()
                attempt.recordIfCurrent("documents_saved")
                LOG.warn("[run] invoking sendToTerminal after typing idle: ${file.name}")
                TerminalUtil.sendToTerminal(project, file, attempt = attempt)
                PromptPoller.getInstance(project).addFile(file)
            }
        }.start()
    }

    override fun update(e: AnActionEvent) {
        val file = e.getData(CommonDataKeys.VIRTUAL_FILE)
        e.presentation.isEnabledAndVisible =
            file != null && file.extension?.lowercase() == "md"
    }
}
