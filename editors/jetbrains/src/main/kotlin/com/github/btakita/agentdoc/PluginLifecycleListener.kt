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
 * Disposes per-project resources when a project closes.
 *
 * Registered in plugin.xml as a projectListener so IntelliJ manages the lifecycle.
 * Native code is process-lifetime state, so plugin updates require a full IDE restart.
 */
class PluginLifecycleListener : ProjectManagerListener {
    override fun projectOpened(project: Project) {
        // Track document changes for typing debounce in SubmitAction
        EditorFactory.getInstance().eventMulticaster.addDocumentListener(TypingTracker, project)
        // Attach markdown buffers as CRDT replicas when the CP endpoint is available.
        CrdtReplicaManager.getInstance(project)
        // Re-register every already-open markdown document through the
        // deferred-reconnect path after process startup so a Lazily-retained
        // response/backlog target cannot remain parked until an operator focus
        // change. This scans open documents without selecting or focusing any
        // editor.
        CrdtReplicaManager.forceRefreshOpenDocumentReplicas(project, "plugin-startup")
        // Start watching for IPC patch files from agent-doc write --ipc
        PatchWatcher.getInstance(project)
        // Highlight agent-doc-specific markdown structures in the editor.
        VisualHighlighterManager.getInstance(project)
        // Detect editor layout changes (tab drags, new splits) and sync tmux
        LayoutChangeDetector.getInstance(project)
        // Flip the turn-state editor banner on/off as the CP turn phase changes.
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
        // Editor selection is operator-owned. Project Controller/tmux activity can
        // follow an explicit editor focus change through EditorTabSyncListener, but
        // background agent, recovery, restart, or pane-focus events must never open
        // or select a different IDE document. The reverse focus mirror therefore
        // remains uninstalled.
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
        TmuxPaneFocusSync.disposeProject(project)
    }
}
