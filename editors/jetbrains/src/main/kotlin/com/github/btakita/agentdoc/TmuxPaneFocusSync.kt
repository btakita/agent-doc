package com.github.btakita.agentdoc

import com.google.gson.JsonObject
import com.google.gson.JsonParser
import com.intellij.openapi.Disposable
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.project.Project
import com.intellij.openapi.util.Disposer
import com.intellij.openapi.vfs.LocalFileSystem
import java.io.File
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit

/**
 * Mirrors tmux pane focus back into JetBrains editor selection.
 *
 * This is intentionally gated on the configured tmux session's current window
 * being `agent-doc`. If the operator moves to another tmux window, the stale
 * active pane inside the hidden agent-doc window must not recall the IDE
 * selection.
 */
class TmuxPaneFocusSync private constructor(
    private val project: Project,
) : Disposable {
    private val executor = Executors.newSingleThreadScheduledExecutor { runnable ->
        Thread(runnable, "agent-doc-tmux-pane-focus-sync").apply { isDaemon = true }
    }

    @Volatile
    private var lastDocumentPath: String? = null

    init {
        executor.scheduleWithFixedDelay(
            { poll() },
            POLL_MS,
            POLL_MS,
            TimeUnit.MILLISECONDS,
        )
    }

    private fun poll() {
        if (project.isDisposed) return
        // Cheap gate: when no markdown documents are open there is nothing to
        // mirror back into the editor, so skip the socket round-trips entirely.
        val manager = FileEditorManager.getInstance(project)
        val hasMarkdownOpen = manager.openFiles.any { it.name.endsWith(".md") }
        if (!hasMarkdownOpen) {
            lastDocumentPath = null
            return
        }
        val projectRoots = candidateProjectRoots(project)
        if (projectRoots.isEmpty()) return

        val documentPath = focusedDocumentPath(projectRoots)
        if (documentPath == null) {
            lastDocumentPath = null
            return
        }
        if (!shouldSelectTmuxDocument(documentPath, lastDocumentPath)) return
        lastDocumentPath = documentPath
        selectEditorDocument(documentPath)
    }

    private fun recordCurrentTmuxFocus() {
        if (project.isDisposed) return
        val projectRoots = candidateProjectRoots(project)
        if (projectRoots.isEmpty()) return
        lastDocumentPath = focusedDocumentPath(projectRoots)
    }

    private fun selectEditorDocument(documentPath: String) {
        val canonical = try {
            File(documentPath).canonicalFile
        } catch (_: Exception) {
            File(documentPath)
        }
        if (!canonical.name.endsWith(".md")) return

        ApplicationManager.getApplication().invokeLater {
            if (project.isDisposed) return@invokeLater
            val file = LocalFileSystem.getInstance().refreshAndFindFileByIoFile(canonical)
                ?: return@invokeLater
            val manager = FileEditorManager.getInstance(project)
            val current = manager.selectedTextEditor?.virtualFile
            if (current?.path == file.path) return@invokeLater
            manager.openFile(file, true, true)
        }
    }

    override fun dispose() {
        executor.shutdownNow()
    }

    companion object {
        private const val POLL_MS = 500L
        private val instances = ConcurrentHashMap<Project, TmuxPaneFocusSync>()

        fun install(project: Project) {
            instances.computeIfAbsent(project) { TmuxPaneFocusSync(project) }
        }

        fun recordCurrentTmuxFocus(project: Project) {
            instances.computeIfAbsent(project) { TmuxPaneFocusSync(project) }
                .recordCurrentTmuxFocus()
        }

        fun disposeProject(project: Project) {
            instances.remove(project)?.let { Disposer.dispose(it) }
        }

        internal fun shouldSelectTmuxDocument(
            documentPath: String?,
            lastDocumentPath: String?,
        ): Boolean = documentPath != null && documentPath != lastDocumentPath

        internal fun isAgentDocWindowActive(projectRoot: String): Boolean? =
            CpcRouteClient.tmuxFocusState(projectRoot)?.let { json ->
                extractWindowNameFromFocusState(json) == "agent-doc"
            }

        internal fun extractDocumentPathFromInspection(json: String): String? {
            return try {
                val root = JsonParser.parseString(json).asJsonObject
                val data = root.objectOrNull("data") ?: root
                data.stringOrNull("document_id")
                    ?: data.objectOrNull("record")?.stringOrNull("document_id")
            } catch (_: Exception) {
                null
            }
        }

        internal fun extractDocumentPathFromFocusState(json: String): String? =
            extractDocumentPathFromInspection(json)

        internal fun extractWindowNameFromFocusState(json: String): String? {
            return try {
                val root = JsonParser.parseString(json).asJsonObject
                val data = root.objectOrNull("data") ?: root
                data.stringOrNull("window_name")
            } catch (_: Exception) {
                null
            }
        }

        internal fun focusReceiptFocused(json: String): Boolean? {
            return try {
                val root = JsonParser.parseString(json).asJsonObject
                val data = root.objectOrNull("data") ?: root
                data.booleanOrNull("focused")
            } catch (_: Exception) {
                null
            }
        }

        internal fun focusReceiptReason(json: String): String? {
            return try {
                val root = JsonParser.parseString(json).asJsonObject
                val data = root.objectOrNull("data") ?: root
                data.stringOrNull("reason")
            } catch (_: Exception) {
                null
            }
        }

        private fun JsonObject.objectOrNull(name: String): JsonObject? {
            if (!has(name)) return null
            val value = get(name)
            if (value == null || value.isJsonNull || !value.isJsonObject) return null
            return value.asJsonObject
        }

        private fun JsonObject.stringOrNull(name: String): String? {
            if (!has(name)) return null
            val value = get(name)
            if (value == null || value.isJsonNull) return null
            return value.asString?.trim()?.takeIf { it.isNotEmpty() }
        }

        private fun JsonObject.booleanOrNull(name: String): Boolean? {
            if (!has(name)) return null
            val value = get(name)
            if (value == null || value.isJsonNull) return null
            return value.asBoolean
        }

        private fun candidateProjectRoots(project: Project): List<String> {
            val roots = linkedSetOf<String>()
            val manager = FileEditorManager.getInstance(project)
            val visibleMdFiles = SyncLayoutAction.collectVisibleMarkdownFiles(manager.selectedFiles)
            val focused = manager.selectedTextEditor?.virtualFile
                ?.takeIf { it.name.endsWith(".md") }
                ?: manager.selectedFiles.firstOrNull { it.name.endsWith(".md") }

            val focusedRoot = focused?.let { TerminalUtil.resolveProject(project, it).first }
            focusedRoot?.let { roots.add(it) }

            if (visibleMdFiles.isNotEmpty() && focusedRoot != null) {
                roots.add(
                    SyncLayoutAction.chooseSyncProjectRoot(
                        project.basePath,
                        focusedRoot,
                        visibleMdFiles,
                    ),
                )
            }
            visibleMdFiles
                .mapNotNull { NativePatching.resolveProjectPath(it)?.first }
                .forEach { roots.add(it) }
            project.basePath?.let { roots.add(it) }
            return roots.toList()
        }

        private fun focusedDocumentPath(projectRoots: List<String>): String? {
            for (root in projectRoots) {
                val json = CpcRouteClient.tmuxFocusState(projectRoot = root)
                    ?: continue
                if (extractWindowNameFromFocusState(json) != "agent-doc") continue
                extractDocumentPathFromFocusState(json)?.let { return it }
            }
            return null
        }
    }
}
