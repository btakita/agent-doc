package com.github.btakita.agentdoc

import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.actionSystem.CommonDataKeys

/**
 * Action that claims the selected .md file for the currently active tmux pane.
 *
 * Triggered by Ctrl+Shift+Alt+C (configurable in Keymap settings).
 * Runs `agent-doc claim <relative-path>` which binds the file's session
 * to whichever tmux pane is currently focused, then sets the pane title.
 * After claiming, triggers a layout sync so the pane joins the layout window.
 */
class ClaimAction : AnAction() {

    override fun actionPerformed(e: AnActionEvent) {
        val project = e.project ?: return
        val file = e.getData(CommonDataKeys.VIRTUAL_FILE) ?: return
        val basePath = project.basePath ?: return

        val relativePath = TerminalUtil.relativePath(project, file)
        val agentDoc = TerminalUtil.resolveAgentDoc()

        Thread {
            try {
                val process = ProcessBuilder(agentDoc, "claim", relativePath)
                    .directory(java.io.File(basePath))
                    .redirectErrorStream(true)
                    .start()
                val output = process.inputStream.bufferedReader().readText().trim()
                val exitCode = process.waitFor()
                if (exitCode == 0) {
                    TerminalUtil.notifyInfo(project, output.ifEmpty { "Claimed $relativePath" })
                    // Re-sync layout so the claimed pane joins the layout window
                    SyncLayoutAction.syncLayout(project, notify = false)
                } else {
                    TerminalUtil.notifyError(project, "Claim failed (exit $exitCode):\n$output")
                }
            } catch (ex: Exception) {
                TerminalUtil.notifyError(project, "Failed to run agent-doc claim: ${ex.message}")
            }
        }.start()
    }

    override fun update(e: AnActionEvent) {
        val file = e.getData(CommonDataKeys.VIRTUAL_FILE)
        e.presentation.isEnabledAndVisible =
            file != null && file.extension?.lowercase() == "md"
    }
}
