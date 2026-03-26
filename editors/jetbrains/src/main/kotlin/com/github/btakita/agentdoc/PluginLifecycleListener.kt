package com.github.btakita.agentdoc

import com.intellij.openapi.editor.EditorFactory
import com.intellij.openapi.fileEditor.FileEditorManagerListener
import com.intellij.openapi.project.Project
import com.intellij.openapi.project.ProjectManagerListener

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
        // Clean up wrong-session/stale panes left from previous IDE sessions
        runResyncFix(project)
    }

    private fun runResyncFix(project: Project) {
        val basePath = project.basePath ?: return
        val agentDocDir = java.io.File(basePath, ".agent-doc")
        if (!agentDocDir.isDirectory) return

        Thread({
            try {
                val process = ProcessBuilder("agent-doc", "resync", "--fix")
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
