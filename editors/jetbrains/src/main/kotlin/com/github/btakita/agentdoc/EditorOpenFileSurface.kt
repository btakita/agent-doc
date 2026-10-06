package com.github.btakita.agentdoc

import com.intellij.codeInsight.daemon.impl.EditorTracker
import com.intellij.openapi.client.ClientKind
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.fileEditor.ClientFileEditorManager
import com.intellij.openapi.fileEditor.FileDocumentManager
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VirtualFile

/**
 * One EDT snapshot of the files the IDE currently exposes as open editor surfaces.
 *
 * A Remote Dev backend can report no backend-local [FileEditorManager.openFiles]
 * while client-scoped managers and [EditorTracker] still expose every visible tab.
 * Native reload, liveness republish, and layout discovery must consume the same
 * observation or a reload can tear down every replica and rediscover none of them.
 */
internal object EditorOpenFileSurface {
    private val log =
        com.intellij.openapi.diagnostic.Logger.getInstance(EditorOpenFileSurface::class.java)

    fun snapshot(project: Project): List<VirtualFile> {
        if (project.isDisposed) return emptyList()
        val files = linkedMapOf<String, VirtualFile>()

        fun retain(file: VirtualFile) {
            files.putIfAbsent(file.path, file)
        }

        FileEditorManager.getInstance(project).openFiles.forEach(::retain)
        try {
            project.getServices(ClientFileEditorManager::class.java, ClientKind.REMOTE)
                .forEach { manager -> manager.getAllFiles().forEach(::retain) }
        } catch (error: Throwable) {
            log.warn("[editor-surface] remote open-file snapshot unavailable", error)
        }
        activeEditors(project).forEach { editor ->
            FileDocumentManager.getInstance().getFile(editor.document)?.let(::retain)
        }
        return files.values.toList()
    }

    fun find(project: Project, filePath: String): VirtualFile? =
        snapshot(project).firstOrNull { it.path == filePath }

    fun activeEditors(project: Project): Set<Editor> = try {
        EditorTracker.getInstance(project).activeEditors.toSet()
    } catch (error: Throwable) {
        log.warn("[editor-surface] active-editor snapshot unavailable", error)
        emptySet()
    }
}
