package com.github.btakita.agentdoc

import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.actionSystem.CommonDataKeys
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.fileEditor.FileDocumentManager
import com.intellij.openapi.project.DumbAware

class InitSessionAction : AnAction(), DumbAware {

    companion object {
        private val LOG = Logger.getInstance(InitSessionAction::class.java)

        internal fun buildCommand(agentDoc: String, relativePath: String): List<String> =
            listOf(agentDoc, "init-session", relativePath)
    }

    override fun actionPerformed(e: AnActionEvent) {
        val project = e.project ?: return
        val file = e.getData(CommonDataKeys.VIRTUAL_FILE) ?: return
        val fileDocumentManager = FileDocumentManager.getInstance()
        fileDocumentManager.getDocument(file)?.let { document ->
            fileDocumentManager.saveDocument(document)
            if (fileDocumentManager.isDocumentUnsaved(document)) {
                TerminalUtil.notifyError(project, "Save the document before initializing a session")
                return
            }
        }

        val (cwd, relativePath) = TerminalUtil.resolveProject(project, file)
        val agentDoc = TerminalUtil.resolveAgentDoc(cwd)

        ApplicationManager.getApplication().executeOnPooledThread {
            try {
                val cmd = buildCommand(agentDoc, relativePath)
                LOG.debug("init-session: ${cmd.joinToString(" ")}")
                val process = ProcessBuilder(cmd)
                    .directory(java.io.File(cwd))
                    .redirectErrorStream(true)
                    .start()
                val output = process.inputStream.bufferedReader().readText().trim()
                val exitCode = process.waitFor()
                file.refresh(false, false)
                if (exitCode == 0) {
                    TerminalUtil.showHint(project, output.ifEmpty { "Initialized session for $relativePath" })
                } else {
                    TerminalUtil.notifyError(project, "Initialize session failed (exit $exitCode):\n$output")
                }
            } catch (ex: Exception) {
                TerminalUtil.notifyError(project, "Failed to initialize session: ${ex.message}")
            }
        }
    }

    override fun update(e: AnActionEvent) {
        val file = e.getData(CommonDataKeys.VIRTUAL_FILE)
        e.presentation.isEnabledAndVisible =
            file != null && file.extension?.lowercase() == "md"
    }
}
