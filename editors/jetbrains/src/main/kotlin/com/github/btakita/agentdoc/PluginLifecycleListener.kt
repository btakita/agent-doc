package com.github.btakita.agentdoc

import com.intellij.openapi.editor.EditorFactory
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.fileEditor.FileEditorManagerListener
import com.intellij.openapi.project.Project
import com.intellij.openapi.project.ProjectManagerListener
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.openapi.vfs.VirtualFileManager

/**
 * Disposes per-project resources when a project closes or when the plugin is
 * dynamically unloaded.
 *
 * Registered in plugin.xml as a projectListener so IntelliJ manages the lifecycle.
 * This enables `require-restart="false"` (dynamic plugin install/update/unload).
 */
class PluginLifecycleListener : ProjectManagerListener {
    internal fun buildStartupResyncCommand(sessionName: String?): List<String> {
        val cmd = mutableListOf("agent-doc", "resync")
        if (sessionName != null) {
            LOG.info("[resync] startup audit will inspect current tmux session: $sessionName")
        }
        return cmd
    }

    override fun projectOpened(project: Project) {
        // Track document changes for typing debounce in SubmitAction
        EditorFactory.getInstance().eventMulticaster.addDocumentListener(TypingTracker, project)
        // Attach markdown buffers as CRDT replicas when the supervisor supports it.
        CrdtReplicaManager.getInstance(project)
        // Start watching for IPC patch files from agent-doc write --ipc
        PatchWatcher.getInstance(project)
        // Highlight agent-doc-specific markdown structures in the editor.
        VisualHighlighterManager.getInstance(project)
        // Detect editor layout changes (tab drags, new splits) and sync tmux
        LayoutChangeDetector.getInstance(project)
        // Register EditorTabSyncListener via code (not XML) so it survives hot-reload
        val editorTabSync = EditorTabSyncListener()
        project.messageBus.connect().subscribe(
            FileEditorManagerListener.FILE_EDITOR_MANAGER,
            editorTabSync
        )
        // Drive tmux pane focus on split-editor focus changes (#panefocussplit):
        // selectionChanged does not fire for focus movement between existing
        // splits, so this reuses editorTabSync's reconcile from focus events.
        EditorFocusSyncListener.install(project, editorTabSync)
        project.messageBus.connect().subscribe(
            FileEditorManagerListener.FILE_EDITOR_MANAGER,
            object : FileEditorManagerListener {
                override fun fileOpened(source: FileEditorManager, file: VirtualFile) {
                    TypingTracker.scheduleOpenDocumentReport(file)
                }

                override fun fileClosed(source: FileEditorManager, file: VirtualFile) {
                    TypingTracker.clearOpenDocumentReport(file)
                }
            }
        )
        TypingTracker.reportOpenMarkdownDocuments(project)
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

                val cmd = buildStartupResyncCommand(sessionName)
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
                    LOG.warn("[resync] agent-doc resync exited with code $exitCode")
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
        CrdtReplicaManager.disposeProject(project)
        PatchWatcher.disposeProject(project)
        LayoutChangeDetector.disposeProject(project)
        VisualHighlighterManager.disposeProject(project)
        EditorFocusSyncListener.disposeProject(project)
    }
}
