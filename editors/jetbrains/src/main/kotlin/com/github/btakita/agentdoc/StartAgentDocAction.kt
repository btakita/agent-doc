package com.github.btakita.agentdoc

import com.github.btakita.agentdoc.shell.ImplAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.actionSystem.CommonDataKeys
import com.intellij.openapi.fileEditor.FileDocumentManager

/** Explicitly starts agent-doc in the focused document's tmux pane. */
class StartAgentDocAction : ImplAction {
    override fun actionPerformed(e: AnActionEvent) {
        val project = e.project ?: return
        val file = e.getData(CommonDataKeys.VIRTUAL_FILE) ?: return
        FileDocumentManager.getInstance().getDocument(file)?.let {
            FileDocumentManager.getInstance().saveDocument(it)
        }
        GenerationWork.executeOnPooledThread {
            if (project.isDisposed || !file.isValid) return@executeOnPooledThread
            val (cwd, relativePath) = TerminalUtil.resolveProject(project, file)
            IdeTerminalCoordinator.startAndAttach(
                project = project,
                cwd = cwd,
                relativePath = relativePath,
                onReady = { TerminalUtil.showHint(project, "Agent Doc started for $relativePath") },
                onFailure = { TerminalUtil.notifyError(project, "Failed to start Agent Doc: $it") },
            )
        }
    }

    override fun update(e: AnActionEvent) {
        val file = e.getData(CommonDataKeys.VIRTUAL_FILE)
        e.presentation.isEnabledAndVisible = file != null && file.extension?.lowercase() == "md"
    }
}
