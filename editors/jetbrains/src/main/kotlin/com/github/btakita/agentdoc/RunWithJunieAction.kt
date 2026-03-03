package com.github.btakita.agentdoc

import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.actionSystem.CommonDataKeys
import com.intellij.openapi.fileEditor.FileDocumentManager
import java.util.concurrent.atomic.AtomicBoolean

/**
 * Action that runs agent-doc via the Junie agent.
 *
 * This calls `agent-doc run --agent junie <relative-path>`.
 *
 * Triggered by Ctrl+Shift+Alt+J (configurable in Keymap settings).
 * Only enabled when the active editor has a .md file open.
 *
 * Guarded against rapid double-invocation.
 */
class RunWithJunieAction : AnAction() {

    companion object {
        private val running = AtomicBoolean(false)
    }

    override fun actionPerformed(e: AnActionEvent) {
        val project = e.project ?: return
        val file = e.getData(CommonDataKeys.VIRTUAL_FILE) ?: return

        // Guard: skip if a run is already in flight
        if (!running.compareAndSet(false, true)) {
            TerminalUtil.showHint(project, "Run with Junie already in progress")
            return
        }

        // Save the file before running so agent-doc sees the latest content
        FileDocumentManager.getInstance().saveAllDocuments()

        val relativePath = TerminalUtil.relativePath(project, file)
        TerminalUtil.runWithAgent(project, "junie", relativePath, onComplete = { running.set(false) })
    }

    override fun update(e: AnActionEvent) {
        val file = e.getData(CommonDataKeys.VIRTUAL_FILE)
        e.presentation.isEnabledAndVisible =
            file != null && file.extension?.lowercase() == "md"
    }
}
