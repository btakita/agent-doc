package com.github.btakita.agentdoc

import com.intellij.openapi.editor.EditorFactory
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.fileEditor.FileEditorManagerEvent
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
    override fun projectOpened(project: Project) {
        // Track document changes for typing debounce in SubmitAction
        EditorFactory.getInstance().eventMulticaster.addDocumentListener(TypingTracker, project)
        // Attach markdown buffers as CRDT replicas when the CPC endpoint is available.
        CrdtReplicaManager.getInstance(project)
        // Start watching for IPC patch files from agent-doc write --ipc
        PatchWatcher.getInstance(project)
        // Highlight agent-doc-specific markdown structures in the editor.
        VisualHighlighterManager.getInstance(project)
        // Detect editor layout changes (tab drags, new splits) and sync tmux
        LayoutChangeDetector.getInstance(project)
        // Flip the turn-state editor banner on/off as the CPC turn phase changes.
        TurnStateBannerRefresher.getInstance(project).start()
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
                override fun selectionChanged(event: FileEditorManagerEvent) {
                    val file = event.newFile ?: return
                    if (file.name.endsWith(".md")) {
                        CrdtReplicaManager.requestRemoteDrain(project, file.path, "selection")
                    }
                }

                override fun fileOpened(source: FileEditorManager, file: VirtualFile) {
                    TypingTracker.scheduleOpenDocumentReport(file)
                    if (file.name.endsWith(".md")) {
                        CrdtReplicaManager.requestRemoteDrain(project, file.path, "file-opened")
                    }
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
