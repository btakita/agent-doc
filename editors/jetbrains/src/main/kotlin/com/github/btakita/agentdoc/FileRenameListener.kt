package com.github.btakita.agentdoc

import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.newvfs.BulkFileListener
import com.intellij.openapi.vfs.newvfs.events.VFileEvent
import com.intellij.openapi.vfs.newvfs.events.VFileMoveEvent
import com.intellij.openapi.vfs.newvfs.events.VFilePropertyChangeEvent
import com.intellij.openapi.vfs.VirtualFile

/**
 * Detects file renames/moves for session documents and triggers a sync
 * so the CLI updates sessions.json with the new path (reusing the existing pane).
 */
class FileRenameListener(private val project: Project) : BulkFileListener {

    companion object {
        private val LOG = Logger.getInstance(FileRenameListener::class.java)

        fun shouldHandleFile(fileName: String?): Boolean {
            return fileName != null && fileName.endsWith(".md")
        }

        fun buildSyncCommand(agentDoc: String, visibleMd: List<String>, relativePath: String): List<String> {
            val colArg = visibleMd.joinToString(",")
            return listOf(agentDoc, "sync", "--col", colArg, "--focus", relativePath, "--rename")
        }
    }

    override fun after(events: List<VFileEvent>) {
        for (event in events) {
            val file: VirtualFile? = when (event) {
                is VFileMoveEvent -> event.file
                is VFilePropertyChangeEvent -> {
                    if (event.propertyName == VirtualFile.PROP_NAME) event.file else null
                }
                else -> null
            }
            if (file == null || !shouldHandleFile(file.name)) continue

            val basePath = project.basePath ?: continue
            val relativePath = TerminalUtil.relativePath(project, file)

            LOG.info("[rename] detected rename/move → $relativePath, triggering sync")

            Thread({
                try {
                    val agentDoc = TerminalUtil.resolveAgentDoc(basePath)
                    val manager = FileEditorManager.getInstance(project)
                    val visibleMd = manager.selectedFiles
                        .filter { shouldHandleFile(it.name) }
                        .map { TerminalUtil.relativePath(project, it) }
                        .distinct()
                    val cmd = buildSyncCommand(agentDoc, visibleMd, relativePath)
                    val process = ProcessBuilder(cmd)
                        .directory(java.io.File(basePath))
                        .redirectErrorStream(true)
                        .start()
                    process.waitFor()
                } catch (e: Exception) {
                    LOG.warn("[rename] sync after rename failed: ${e.message}")
                }
            }, "agent-doc-rename-sync").apply {
                isDaemon = true
                start()
            }
        }
    }
}
