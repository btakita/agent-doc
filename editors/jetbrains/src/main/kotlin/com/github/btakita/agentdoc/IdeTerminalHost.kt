package com.github.btakita.agentdoc

import com.intellij.openapi.project.Project
import com.intellij.ui.content.Content
import org.jetbrains.plugins.terminal.ShellTerminalWidget
import org.jetbrains.plugins.terminal.TerminalToolWindowManager

internal const val IDE_HOSTED_TMUX_CAPABILITY = "ide_hosted_tmux_v1"

/** Thin adapter around the optional JetBrains Terminal plugin API. */
internal object IdeTerminalHost {
    private const val TAB_NAME = "agent-doc"

    fun hasLiveAgentDocTab(project: Project): Boolean = liveTab(project) != null

    fun focusExisting(project: Project) {
        val (manager, content, _) = liveTab(project) ?: return
        focus(manager, content)
    }

    fun attachExisting(project: Project, attachCommand: String) {
        val (manager, content, widget) = liveTab(project)
            ?: error("agent-doc terminal tab is no longer alive")
        focus(manager, content)
        widget.executeCommand(attachCommand)
    }

    fun createAndAttach(project: Project, cwd: String, attachCommand: String) {
        val manager = TerminalToolWindowManager.getInstance(project)
        val widget = manager.createLocalShellWidget(cwd, TAB_NAME)
        widget.executeCommand(attachCommand)
        manager.toolWindow?.activate(null)
    }

    private fun liveTab(
        project: Project,
    ): Triple<TerminalToolWindowManager, Content, ShellTerminalWidget>? {
        val manager = TerminalToolWindowManager.getInstance(project)
        val toolWindow = manager.toolWindow ?: return null
        val content = toolWindow.contentManager.contents
            .firstOrNull { it.displayName == TAB_NAME }
            ?: return null
        val widget = TerminalToolWindowManager.getWidgetByContent(content) as? ShellTerminalWidget
            ?: return null
        val connector = widget.processTtyConnector ?: return null
        if (!connector.isConnected) return null
        return Triple(manager, content, widget)
    }

    private fun focus(manager: TerminalToolWindowManager, content: Content) {
        manager.toolWindow?.let { toolWindow ->
            toolWindow.contentManager.setSelectedContent(content)
            toolWindow.activate(null)
        }
    }
}
