package com.github.btakita.agentdoc

import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.actionSystem.CommonDataKeys
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.fileEditor.ex.FileEditorManagerEx

/**
 * Force-claim the focused .md file, overriding the binding invariant.
 *
 * Like ClaimAction but passes --force, which allows commandeering a pane
 * already claimed by another document. Use when the normal claim provisions
 * a new pane but you want to reuse the existing one.
 */
class ForceClaimAction : AnAction() {

    companion object {
        private val LOG = Logger.getInstance(ForceClaimAction::class.java)
    }

    override fun actionPerformed(e: AnActionEvent) {
        val project = e.project ?: return
        val file = e.getData(CommonDataKeys.VIRTUAL_FILE) ?: return

        val (cwd, relativePath) = TerminalUtil.resolveProject(project, file)

        val layoutRelPath = TerminalUtil.relativePath(project, file)
        val managerEx = FileEditorManagerEx.getInstanceEx(project)
        val editorLayout = if (managerEx.windows.size > 1)
            LayoutDetector.detectEditorLayout(project) else null
        val position = editorLayout?.let { layout ->
            val colIdx = layout.columns.indexOfFirst { col -> layoutRelPath in col.files }
            when {
                colIdx < 0 -> null
                colIdx == 0 -> "left"
                colIdx == layout.columns.size - 1 -> "right"
                else -> null
            }
        }

        Thread {
            try {
                TmuxPaneFocusSync.recordCurrentTmuxFocus(project)
                val agentDoc = TerminalUtil.resolveAgentDoc(cwd)
                val cmd = mutableListOf(agentDoc, "claim", relativePath, "--force")
                if (position != null) {
                    cmd.addAll(listOf("--position", position))
                }
                LOG.debug("force-claim: ${cmd.joinToString(" ")}")
                val process = ProcessBuilder(cmd)
                    .directory(java.io.File(cwd))
                    .redirectErrorStream(true)
                    .start()
                val output = process.inputStream.bufferedReader().readText().trim()
                val exitCode = process.waitFor()
                if (exitCode == 0) {
                    TerminalUtil.showHint(project, output.ifEmpty { "Force-claimed $relativePath" })
                    SyncLayoutAction.syncLayout(project, notify = false, noAutostart = false)
                } else {
                    TerminalUtil.notifyError(project, "Force claim failed (exit $exitCode):\n$output")
                }
            } catch (ex: Exception) {
                TerminalUtil.notifyError(project, "Failed to run agent-doc claim --force: ${ex.message}")
            }
        }.start()
    }

    override fun update(e: AnActionEvent) {
        val file = e.getData(CommonDataKeys.VIRTUAL_FILE)
        e.presentation.isEnabledAndVisible =
            file != null && file.extension?.lowercase() == "md"
    }
}
