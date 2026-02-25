package com.github.btakita.agentdoc

import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.actionSystem.CommonDataKeys

/**
 * Action that sends `/agent-doc <relative-path>` to the active terminal.
 *
 * Triggered by Ctrl+Shift+Alt+A (configurable in Keymap settings).
 * Only enabled when the active editor has a .md file open.
 */
class SubmitAction : AnAction() {

    override fun actionPerformed(e: AnActionEvent) {
        val project = e.project ?: return
        val file = e.getData(CommonDataKeys.VIRTUAL_FILE) ?: return

        val relativePath = TerminalUtil.relativePath(project, file)
        TerminalUtil.sendToTerminal(project, relativePath)
    }

    override fun update(e: AnActionEvent) {
        val file = e.getData(CommonDataKeys.VIRTUAL_FILE)
        e.presentation.isEnabledAndVisible =
            file != null && file.extension?.lowercase() == "md"
    }
}
