package com.github.btakita.agentdoc

import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.actionSystem.CommonDataKeys
import com.intellij.openapi.application.ApplicationManager
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

        val fence = try {
            ClaimDocumentFence.acquire(project, file)
        } catch (failure: Throwable) {
            TerminalUtil.notifyError(
                project,
                "Force claim was not started because IDEA could not save the active document: ${failure.message}",
            )
            return
        }

        ApplicationManager.getApplication().executeOnPooledThread {
            try {
                TmuxPaneFocusSync.recordCurrentTmuxFocus(project)
                val agentDoc = TerminalUtil.resolveAgentDoc(cwd)
                val cmd = ClaimAction.buildClaimCommand(
                    agentDoc,
                    relativePath,
                    position,
                    force = true,
                    newPane = false,
                )
                LOG.debug("force-claim: ${cmd.joinToString(" ")}")
                val result = SyncLayoutAction.runCommandWithTimeout(
                    cmd,
                    cwd,
                    ClaimAction.CLAIM_PROCESS_TIMEOUT_MS,
                )
                val output = if (result.timedOut) {
                    listOf(
                        result.output,
                        "Force claim timed out after ${ClaimAction.CLAIM_PROCESS_TIMEOUT_MS / 1_000} seconds",
                    ).filter { it.isNotEmpty() }.joinToString("\n")
                } else {
                    result.output
                }
                val exitCode = result.exitCode
                if (exitCode == 0) {
                    TerminalUtil.showHint(project, output.ifEmpty { "Force-claimed $relativePath" })
                    SyncLayoutAction.syncLayout(project, notify = false, noAutostart = false)
                } else {
                    TerminalUtil.notifyError(project, "Force claim failed (exit $exitCode):\n$output")
                }
            } catch (ex: Exception) {
                TerminalUtil.notifyError(project, "Failed to run agent-doc claim --force: ${ex.message}")
            } finally {
                fence.release()
            }
        }
    }

    override fun update(e: AnActionEvent) {
        val file = e.getData(CommonDataKeys.VIRTUAL_FILE)
        e.presentation.isEnabledAndVisible =
            file != null && file.extension?.lowercase() == "md"
    }
}
