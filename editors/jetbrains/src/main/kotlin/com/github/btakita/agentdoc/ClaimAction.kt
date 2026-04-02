package com.github.btakita.agentdoc

import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.actionSystem.CommonDataKeys
import com.intellij.openapi.diagnostic.Logger

/**
 * Action that claims the focused .md file and syncs the tmux layout.
 *
 * Triggered by Ctrl+Shift+Alt+C (configurable in Keymap settings).
 * 1. Runs `agent-doc claim <file>` on the focused file (adds frontmatter if missing)
 * 2. Runs SyncLayoutAction.syncLayout() to arrange tmux panes
 *
 * The claim step is essential for unclaimed files — it generates the session UUID
 * and scaffolds components. Sync alone can't manage files without session UUIDs.
 */
class ClaimAction : AnAction() {

    companion object {
        private val LOG = Logger.getInstance(ClaimAction::class.java)
    }

    override fun actionPerformed(e: AnActionEvent) {
        val project = e.project ?: return
        val file = e.getData(CommonDataKeys.VIRTUAL_FILE) ?: return
        val basePath = project.basePath ?: return

        val relativePath = TerminalUtil.relativePath(project, file)
        val windowId = TerminalUtil.projectWindowId(project)

        Thread {
            try {
                // Step 1: Claim the focused file (adds frontmatter if missing)
                val agentDoc = TerminalUtil.resolveAgentDoc(basePath)
                val cmd = mutableListOf(agentDoc, "claim", relativePath)
                if (windowId != null) {
                    cmd.addAll(listOf("--window", windowId))
                }
                LOG.debug("claim: ${cmd.joinToString(" ")}")
                val process = ProcessBuilder(cmd)
                    .directory(java.io.File(basePath))
                    .redirectErrorStream(true)
                    .start()
                val output = process.inputStream.bufferedReader().readText().trim()
                val exitCode = process.waitFor()
                if (exitCode == 0) {
                    TerminalUtil.showHint(project, output.ifEmpty { "Claimed $relativePath" })
                } else {
                    TerminalUtil.notifyError(project, "Claim failed (exit $exitCode):\n$output")
                }

                // Step 2: Sync layout to arrange tmux panes
                SyncLayoutAction.syncLayout(project, notify = false)
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
