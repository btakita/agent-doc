package com.github.btakita.agentdoc

import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.actionSystem.ActionUpdateThread
import com.intellij.openapi.actionSystem.CommonDataKeys
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.fileEditor.ex.FileEditorManagerEx
import com.intellij.openapi.ui.Splitter
import java.awt.Component

/**
 * Manually re-syncs the tmux pane layout to match the current IDE editor split.
 *
 * Triggered by Ctrl+Shift+Alt+L or via the Alt+Enter popup menu.
 * Runs immediately (no debounce) and clears the dedup cache so
 * automatic sync picks up subsequent changes.
 */
class SyncLayoutAction : AnAction() {

    companion object {
        /**
         * Syncs tmux layout to match the IDE editor split. Can be called from
         * any action (e.g. ClaimAction calls this after claiming).
         * Runs on a background thread — safe to call from EDT.
         */
        fun syncLayout(project: com.intellij.openapi.project.Project, notify: Boolean = true) {
            val basePath = project.basePath ?: return

            val manager = FileEditorManager.getInstance(project)
            val visibleMdFiles = manager.selectedFiles
                .filter { it.name.endsWith(".md") }
                .map { TerminalUtil.relativePath(project, it) }
                .distinct()

            if (visibleMdFiles.isEmpty()) {
                if (notify) TerminalUtil.notifyInfo(project, "No .md files open")
                return
            }

            val split = detectSplitOrientation(project)
            EditorTabSyncListener.clearLastFileSet()

            Thread {
                try {
                    val agentDoc = TerminalUtil.resolveAgentDoc()
                    val cmd = if (visibleMdFiles.size == 1) {
                        listOf(agentDoc, "focus", visibleMdFiles[0])
                    } else {
                        val splitFlag = if (split == "v") "v" else "h"
                        listOf(agentDoc, "layout") + visibleMdFiles + listOf("--split", splitFlag)
                    }
                    val process = ProcessBuilder(cmd)
                        .directory(java.io.File(basePath))
                        .redirectErrorStream(true)
                        .start()
                    val output = process.inputStream.bufferedReader().readText().trim()
                    val exitCode = process.waitFor()
                    if (notify) {
                        if (exitCode == 0) {
                            TerminalUtil.notifyInfo(project, output.ifEmpty { "Layout synced" })
                        } else {
                            TerminalUtil.notifyError(project, "Sync failed (exit $exitCode):\n$output")
                        }
                    }
                } catch (ex: Exception) {
                    if (notify) TerminalUtil.notifyError(project, "Failed to sync layout: ${ex.message}")
                }
            }.start()
        }

        private fun detectSplitOrientation(project: com.intellij.openapi.project.Project): String {
            try {
                val managerEx = FileEditorManagerEx.getInstanceEx(project)
                val splitters = managerEx.splitters
                val root = (splitters as? java.awt.Container) ?: return "h"
                return findSplitterOrientation(root) ?: "h"
            } catch (_: Exception) {
                return "h"
            }
        }

        private fun findSplitterOrientation(component: Component): String? {
            if (component is Splitter) {
                return if (component.isVertical) "v" else "h"
            }
            if (component is java.awt.Container) {
                for (child in component.components) {
                    val result = findSplitterOrientation(child)
                    if (result != null) return result
                }
            }
            return null
        }
    }

    override fun actionPerformed(e: AnActionEvent) {
        val project = e.project ?: return
        syncLayout(project)
    }

    override fun update(e: AnActionEvent) {
        val file = e.getData(CommonDataKeys.VIRTUAL_FILE)
        e.presentation.isEnabledAndVisible =
            file != null && file.extension?.lowercase() == "md"
    }

    override fun getActionUpdateThread(): ActionUpdateThread {
        return ActionUpdateThread.BGT
    }
}
