package com.github.btakita.agentdoc

import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.actionSystem.CommonDataKeys
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.fileEditor.ex.FileEditorManagerEx
import com.intellij.openapi.ui.Splitter
import com.intellij.openapi.vfs.VirtualFile
import java.awt.Component

/**
 * Action that claims the selected .md file for the correct tmux pane
 * based on its position in the editor split.
 *
 * Triggered by Ctrl+Shift+Alt+C (configurable in Keymap settings).
 * Detects whether the file is in the left/right (or top/bottom) editor split
 * and passes `--position` to `agent-doc claim` so the correct tmux pane
 * is selected by coordinates rather than whichever pane is focused.
 * After claiming, triggers a layout sync so the pane joins the layout window.
 */
class ClaimAction : AnAction() {

    companion object {
        private val LOG = Logger.getInstance(ClaimAction::class.java)
    }

    override fun actionPerformed(e: AnActionEvent) {
        val project = e.project ?: return
        val file = e.getData(CommonDataKeys.VIRTUAL_FILE) ?: return
        val basePath = project.basePath ?: return

        val relativePath = TerminalUtil.relativePath(project, file)
        val agentDoc = TerminalUtil.resolveAgentDoc()
        val position = detectEditorPosition(e, file)

        LOG.debug("claim $relativePath position=$position agentDoc=$agentDoc")

        val windowId = TerminalUtil.projectWindowId(project)

        Thread {
            try {
                val cmd = mutableListOf(agentDoc, "claim", relativePath)
                if (position != null) {
                    cmd.addAll(listOf("--position", position))
                }
                if (windowId != null) {
                    cmd.addAll(listOf("--window", windowId))
                }
                LOG.debug("cmd: ${cmd.joinToString(" ")}")
                val process = ProcessBuilder(cmd)
                    .directory(java.io.File(basePath))
                    .redirectErrorStream(true)
                    .start()
                val output = process.inputStream.bufferedReader().readText().trim()
                val exitCode = process.waitFor()
                if (exitCode == 0) {
                    TerminalUtil.showHint(project, output.ifEmpty { "Claimed $relativePath (pos=$position)" })
                    // Re-sync layout so the claimed pane joins the layout window
                    SyncLayoutAction.syncLayout(project, notify = false)
                } else {
                    TerminalUtil.notifyError(project, "Claim failed (exit $exitCode):\n$output")
                }
            } catch (ex: Exception) {
                TerminalUtil.notifyError(project, "Failed to run agent-doc claim: ${ex.message}")
            }
        }.start()
    }

    /**
     * Detect the position of the current file in the editor split layout.
     * Strategy:
     * 1. Try to find the editor component in the Splitter tree (works when action
     *    is triggered from an open editor with EDITOR context available)
     * 2. Fall back to checking which split window contains the file (works from
     *    context menu, file tree, etc.)
     */
    private fun detectEditorPosition(e: AnActionEvent, file: VirtualFile): String? {
        val project = e.project ?: return null
        try {
            val managerEx = FileEditorManagerEx.getInstanceEx(project)
            val splitters = managerEx.splitters
            val root = (splitters as? java.awt.Container) ?: run {
                LOG.debug("splitters not a Container: ${splitters?.javaClass?.name}")
                return null
            }

            // Strategy 1: Use the EDITOR component from action context
            val editor = e.getData(CommonDataKeys.EDITOR)
            if (editor != null) {
                val editorComponent = editor.component
                LOG.debug("strategy1: editor component=${editorComponent.javaClass.name}")
                val pos = findPositionInSplitter(root, editorComponent)
                if (pos != null) {
                    LOG.debug("strategy1: found position=$pos")
                    return pos
                }
                LOG.debug("strategy1: position not found in splitter tree")
            } else {
                LOG.debug("strategy1: EDITOR is null")
            }

            // Strategy 2: Find which split window contains the target file
            // by looking at the open files in each window/composite
            val windows = managerEx.windows
            LOG.debug("strategy2: ${windows.size} windows")
            if (windows.size >= 2) {
                // Find which window has our file
                for ((idx, window) in windows.withIndex()) {
                    val files = window.files
                    LOG.debug("  window[$idx]: ${files.map { it.name }}")
                    if (files.any { it.path == file.path }) {
                        // Determine if this window's component is first or second in the splitter
                        val windowComponent = (window as? Component)
                        if (windowComponent != null) {
                            val pos = findPositionInSplitter(root, windowComponent)
                            if (pos != null) {
                                LOG.debug("strategy2: window[$idx] position=$pos")
                                return pos
                            }
                        }
                        // Heuristic: first window = left/top, second = right/bottom
                        val orientation = findSplitterOrientation(root)
                        val pos = when {
                            idx == 0 && orientation == "h" -> "left"
                            idx == 0 && orientation == "v" -> "top"
                            idx >= 1 && orientation == "h" -> "right"
                            idx >= 1 && orientation == "v" -> "bottom"
                            else -> null
                        }
                        LOG.debug("strategy2: heuristic idx=$idx orientation=$orientation -> $pos")
                        return pos
                    }
                }
            }

            LOG.debug("no position detected")
            return null
        } catch (ex: Exception) {
            LOG.debug("exception: ${ex.message}")
            return null
        }
    }

    /**
     * Walk the component tree to find the Splitter containing the target component,
     * then determine if the target is in the first or second child.
     */
    private fun findPositionInSplitter(component: Component, target: Component): String? {
        if (component is Splitter) {
            val firstChild = component.firstComponent
            val secondChild = component.secondComponent
            val isVertical = component.isVertical

            if (firstChild != null && isDescendant(firstChild, target)) {
                return if (isVertical) "top" else "left"
            }
            if (secondChild != null && isDescendant(secondChild, target)) {
                return if (isVertical) "bottom" else "right"
            }
        }
        if (component is java.awt.Container) {
            for (child in component.components) {
                val result = findPositionInSplitter(child, target)
                if (result != null) return result
            }
        }
        return null
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

    /**
     * Check if target is a descendant of (or equal to) the container.
     */
    private fun isDescendant(container: Component, target: Component): Boolean {
        if (container === target) return true
        if (container is java.awt.Container) {
            for (child in container.components) {
                if (isDescendant(child, target)) return true
            }
        }
        return false
    }

    override fun update(e: AnActionEvent) {
        val file = e.getData(CommonDataKeys.VIRTUAL_FILE)
        e.presentation.isEnabledAndVisible =
            file != null && file.extension?.lowercase() == "md"
    }
}
