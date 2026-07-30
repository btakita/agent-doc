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
 * Registered in plugin.xml as a projectListener so IntelliJ manages the lifecycle. Native code is
 * process-lifetime state, so plugin updates require a full IDE restart.
 */
class PluginLifecycleListener : ProjectManagerListener {
    override fun projectOpened(project: Project) {
        // Track document changes for typing debounce in SubmitAction
        EditorFactory.getInstance().eventMulticaster.addDocumentListener(TypingTracker, project)
        // Attach markdown buffers as CRDT replicas when the CP endpoint is available.
        CrdtReplicaManager.getInstance(project)
        // `#ctrlkillreregister` Tier 3: ask the controller which of THIS editor's
        // registrations it holds no replica for, and rebuild exactly those, so a
        // Lazily-retained response/backlog target cannot remain parked until an
        // operator focus change. Nothing is selected or focused.
        //
        // This replaces the blind re-register of every open markdown document. The
        // sweep dropped and rebuilt healthy CRDT baselines on every startup — the
        // lossiest operation the replica manager has — and still missed a stranded
        // registration whose document was not open in a tab. It remains the fallback
        // inside `pullMissingReplicas` for when the pull itself cannot be asked.
        CrdtReplicaManager.pullMissingReplicas(project, "plugin-startup")
        // Start watching for IPC patch files from agent-doc write --ipc
        val patchWatcher = PatchWatcher.getInstance(project)
        // Highlight agent-doc-specific markdown structures in the editor.
        VisualHighlighterManager.getInstance(project)
        // Detect editor layout changes (tab drags, new splits) and sync tmux
        LayoutChangeDetector.getInstance(project)
        // Flip the turn-state editor banner on/off as the CP turn phase changes.
        TurnStateBannerRefresher.getInstance(project).start()
        // Register EditorTabSyncListener via code (not XML) so it survives hot-reload
        val editorTabSync = EditorTabSyncListener.install(project)
        project.messageBus
            .connect(project)
            .subscribe(
                FileEditorManagerListener.FILE_EDITOR_MANAGER,
                editorTabSync,
            )
        editorTabSync.onEditorLayoutChanged(project)
        // Drive tmux pane focus on split-editor focus changes (#panefocussplit):
        // selectionChanged does not fire for focus movement between existing
        // splits, so this reuses editorTabSync's reconcile from focus events.
        EditorFocusSyncListener.install(project, editorTabSync)
        // Editor selection is operator-owned. Project Controller/tmux activity can
        // follow an explicit editor focus change through EditorTabSyncListener, but
        // background agent, recovery, restart, or pane-focus events must never open
        // or select a different IDE document. The reverse focus mirror therefore
        // remains uninstalled.
        project.messageBus
            .connect(project)
            .subscribe(
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
                            patchWatcher.registerRootForFile(file.path)
                            CrdtReplicaManager.requestRemoteDrain(project, file.path, "file-opened")
                        }
                    }

                    override fun fileClosed(source: FileEditorManager, file: VirtualFile) {
                        TypingTracker.clearOpenDocumentReport(file)
                    }
                },
            )
        FileEditorManager.getInstance(project)
            .openFiles
            .asSequence()
            .filter { it.name.endsWith(".md") }
            .forEach { patchWatcher.registerRootForFile(it.path) }
        TypingTracker.reportOpenMarkdownDocuments(project)
        // Detect file renames/moves and update sessions.json path
        project.messageBus
            .connect(project)
            .subscribe(
                VirtualFileManager.VFS_CHANGES,
                FileRenameListener(project),
            )
    }

    companion object {
        private val LOG =
            com.intellij.openapi.diagnostic.Logger.getInstance(PluginLifecycleListener::class.java)
    }

    override fun projectClosed(project: Project) {
        CrdtReplicaManager.disposeProject(project)
        PatchWatcher.disposeProject(project)
        LayoutChangeDetector.disposeProject(project)
        VisualHighlighterManager.disposeProject(project)
        // Stop feeding focus events before releasing the surface graph, so a
        // late observation cannot re-create the root we are about to forget.
        EditorFocusSyncListener.disposeProject(project)
        EditorTabSyncListener.disposeProject(project)
        TmuxPaneFocusSync.disposeProject(project)
    }
}
