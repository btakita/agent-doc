package com.github.btakita.agentdoc

import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.actionSystem.CommonDataKeys
import com.intellij.openapi.fileEditor.FileDocumentManager
import java.util.concurrent.atomic.AtomicBoolean

/**
 * Action that sends `/agent-doc <relative-path>` to the active terminal.
 *
 * Triggered by Ctrl+Shift+Alt+A (configurable in Keymap settings).
 * Only enabled when the active editor has a .md file open.
 *
 * **Conditional debounce:** If the user was typing within 1.5s of Run,
 * waits for typing to settle (via FFI `await_idle`) before routing.
 * Otherwise routes immediately with zero delay.
 *
 * Guarded against rapid double-invocation — if a route is already in flight,
 * subsequent calls are silently skipped.
 */
class SubmitAction : AnAction() {

    companion object {
        private val routing = AtomicBoolean(false)
        private val LOG = com.intellij.openapi.diagnostic.Logger.getInstance(SubmitAction::class.java)
    }

    override fun actionPerformed(e: AnActionEvent) {
        val project = e.project ?: return
        val file = e.getData(CommonDataKeys.VIRTUAL_FILE) ?: return

        if (!routing.compareAndSet(false, true)) {
            TerminalUtil.showHint(project, "Route already in progress")
            return
        }

        val relativePath = TerminalUtil.relativePath(project, file)

        if (TypingTracker.isRecentlyTyping(file.path)) {
            LOG.info("[run] recent typing detected, debouncing")
            TerminalUtil.showHint(project, "Waiting for typing to settle...")
            Thread({
                try {
                    TypingTracker.awaitIdle(file.path)
                    com.intellij.openapi.application.ApplicationManager.getApplication().invokeLater {
                        FileDocumentManager.getInstance().saveAllDocuments()
                        TerminalUtil.sendToTerminal(project, relativePath, onComplete = { routing.set(false) })
                        PromptPoller.getInstance(project).addFile(file)
                    }
                } catch (ex: Exception) {
                    LOG.warn("[run] debounce error: ${ex.message}")
                    routing.set(false)
                }
            }, "agent-doc-run-debounce").apply {
                isDaemon = true
                start()
            }
        } else {
            FileDocumentManager.getInstance().saveAllDocuments()
            TerminalUtil.sendToTerminal(project, relativePath, onComplete = { routing.set(false) })
            PromptPoller.getInstance(project).addFile(file)
        }
    }

    override fun update(e: AnActionEvent) {
        val file = e.getData(CommonDataKeys.VIRTUAL_FILE)
        e.presentation.isEnabledAndVisible =
            file != null && file.extension?.lowercase() == "md"
    }
}
