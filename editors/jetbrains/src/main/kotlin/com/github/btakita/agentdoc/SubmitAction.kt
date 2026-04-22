package com.github.btakita.agentdoc

import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.actionSystem.CommonDataKeys
import com.intellij.openapi.fileEditor.FileDocumentManager
import java.util.concurrent.atomic.AtomicBoolean

/**
 * Action that routes the active document through `agent-doc route`.
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

        LOG.warn("[run] actionPerformed: ${file.name}")

        if (!routing.compareAndSet(false, true)) {
            LOG.warn("[run] BLOCKED: route already in progress for ${file.name}")
            TerminalUtil.showHint(project, "Route already in progress")
            return
        }

        if (TypingTracker.isRecentlyTyping(file.path)) {
            LOG.warn("[run] recent typing detected, debouncing before route: ${file.name}")
            TerminalUtil.showHint(project, "Waiting for typing to settle...")
            Thread({
                try {
                    val idle = TypingTracker.awaitIdle(file.path)
                    LOG.warn("[run] debounce finished: idle=$idle for ${file.name}")
                    if (!idle) {
                        LOG.warn("[run] debounce TIMEOUT — routing anyway: ${file.name}")
                    }
                    com.intellij.openapi.application.ApplicationManager.getApplication().invokeLater {
                        FileDocumentManager.getInstance().saveAllDocuments()
                        LOG.warn("[run] invoking sendToTerminal after debounce: ${file.name}")
                        TerminalUtil.sendToTerminal(project, file, onComplete = {
                            LOG.warn("[run] route complete (onComplete callback): ${file.name}")
                            routing.set(false)
                        })
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
            LOG.warn("[run] no recent typing, invoking sendToTerminal immediately: ${file.name}")
            TerminalUtil.sendToTerminal(project, file, onComplete = {
                LOG.warn("[run] route complete (onComplete callback): ${file.name}")
                routing.set(false)
            })
            PromptPoller.getInstance(project).addFile(file)
        }
    }

    override fun update(e: AnActionEvent) {
        val file = e.getData(CommonDataKeys.VIRTUAL_FILE)
        e.presentation.isEnabledAndVisible =
            file != null && file.extension?.lowercase() == "md"
    }
}
