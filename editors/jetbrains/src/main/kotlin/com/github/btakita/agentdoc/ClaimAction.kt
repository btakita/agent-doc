package com.github.btakita.agentdoc

import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.actionSystem.CommonDataKeys
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.fileEditor.ex.FileEditorManagerEx

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

        // Determine position from editor layout so claim targets the correct tmux pane.
        // Without --position, claim falls back to the last active tmux pane (wrong).
        // LayoutDetector maps horizontal splits to left/right columns, vertical to same column.
        val managerEx = FileEditorManagerEx.getInstanceEx(project)
        val editorLayout = if (managerEx.windows.size > 1)
            LayoutDetector.detectEditorLayout(project) else null
        val position = editorLayout?.let { layout ->
            val colIdx = layout.columns.indexOfFirst { col -> relativePath in col.files }
            when {
                colIdx < 0 -> null                         // file not found in layout
                colIdx == 0 -> "left"
                colIdx == layout.columns.size - 1 -> "right"
                else -> null                               // middle column — skip for now
            }
        }

        Thread {
            try {
                // Step 1: Claim the focused file (adds frontmatter if missing)
                val agentDoc = TerminalUtil.resolveAgentDoc(basePath)
                val cmd = mutableListOf(agentDoc, "claim", relativePath)
                if (windowId != null) {
                    cmd.addAll(listOf("--window", windowId))
                }
                if (position != null) {
                    cmd.addAll(listOf("--position", position))
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
