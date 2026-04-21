package com.github.btakita.agentdoc

import com.intellij.openapi.editor.EditorFactory
import com.intellij.openapi.fileEditor.FileEditorManagerListener
import com.intellij.openapi.project.Project
import com.intellij.openapi.project.ProjectManagerListener
import com.intellij.openapi.vfs.VirtualFileManager

/**
 * Disposes per-project resources (PromptPoller, PromptPanel) when a project closes
 * or when the plugin is dynamically unloaded.
 *
 * Registered in plugin.xml as a projectListener so IntelliJ manages the lifecycle.
 * This enables `require-restart="false"` (dynamic plugin install/update/unload).
 */
class PluginLifecycleListener : ProjectManagerListener {
    override fun projectOpened(project: Project) {
        // Track document changes for typing debounce in SubmitAction
        EditorFactory.getInstance().eventMulticaster.addDocumentListener(TypingTracker, project)
        // Start watching for IPC patch files from agent-doc write --ipc
        PatchWatcher.getInstance(project)
        // Detect editor layout changes (tab drags, new splits) and sync tmux
        LayoutChangeDetector.getInstance(project)
        // Register EditorTabSyncListener via code (not XML) so it survives hot-reload
        project.messageBus.connect().subscribe(
            FileEditorManagerListener.FILE_EDITOR_MANAGER,
            EditorTabSyncListener()
        )
        // Detect file renames/moves and update sessions.json path
        project.messageBus.connect().subscribe(
            VirtualFileManager.VFS_CHANGES,
            FileRenameListener(project)
        )
        // Clean up wrong-session/stale panes left from previous IDE sessions
        runResyncFix(project)
    }

    private fun runResyncFix(project: Project) {
        val basePath = project.basePath ?: return
        val agentDocDir = java.io.File(basePath, ".agent-doc")
        if (!agentDocDir.isDirectory) return

        Thread({
            try {
                // Determine the current tmux session so resync uses relocation
                // instead of killing panes in a different session.
                val sessionName = try {
                    val tmuxProc = ProcessBuilder("tmux", "display-message", "-p", "#{session_name}")
                        .redirectErrorStream(true)
                        .start()
                    val name = tmuxProc.inputStream.bufferedReader().readText().trim()
                    if (tmuxProc.waitFor() == 0 && name.isNotEmpty()) name else null
                } catch (_: Exception) { null }

                val cmd = mutableListOf("agent-doc", "resync", "--fix")
                if (sessionName != null) {
                    cmd.addAll(listOf("--session", sessionName))
                    LOG.info("[resync] using relocation mode with session: $sessionName")
                }
                val process = ProcessBuilder(cmd)
                    .directory(java.io.File(basePath))
                    .redirectErrorStream(true)
                    .start()
                val exitCode = process.waitFor()
                val output = process.inputStream.bufferedReader().readText().trim()
                if (output.isNotEmpty()) {
                    LOG.info("[resync] $output")
                }
                if (exitCode != 0) {
                    LOG.warn("[resync] agent-doc resync --fix exited with code $exitCode")
                }
            } catch (e: Exception) {
                LOG.info("[resync] agent-doc not available: ${e.message}")
            }

            // Auto-start prompt polling after resync cleans up stale sessions
            val sessionsFile = java.io.File(basePath, ".agent-doc/sessions.json")
            if (sessionsFile.exists()) {
                LOG.info("[lifecycle] auto-starting prompt poller for ${project.name}")
                PromptPoller.getInstance(project).startPolling()
            }
        }, "agent-doc-resync-fix").apply {
            isDaemon = true
            start()
        }
    }

    companion object {
        private val LOG = com.intellij.openapi.diagnostic.Logger.getInstance(PluginLifecycleListener::class.java)
    }

    override fun projectClosed(project: Project) {
        PromptPanel.dismiss(project)
        PromptPoller.disposeProject(project)
        PatchWatcher.disposeProject(project)
        LayoutChangeDetector.disposeProject(project)
    }
}
