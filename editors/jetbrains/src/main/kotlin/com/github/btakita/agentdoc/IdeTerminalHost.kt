package com.github.btakita.agentdoc

import com.intellij.openapi.project.Project
import com.intellij.ui.content.Content
import org.jetbrains.plugins.terminal.ShellTerminalWidget
import org.jetbrains.plugins.terminal.TerminalToolWindowManager

internal const val IDE_HOSTED_TMUX_CAPABILITY = "ide_hosted_tmux_v1"

/**
 * What a focus request was actually able to do (`#jbfocusnoop`). `focusExisting`
 * used to return `Unit` and `return` early on every miss, so a "Focus Agent
 * Terminal" click that found no live tab navigated nowhere and said nothing.
 * Callers need the outcome so they can tell the operator instead of vanishing.
 */
internal enum class TerminalFocusOutcome {
    /** The agent-doc tab was found: selected, and its tool window activated. */
    AGENT_TAB,

    /**
     * A terminal tool window exists but holds no agent-doc tab. Activate it
     * anyway — landing the operator in the terminal is strictly better than a
     * silent no-op, and it shows them the tab is missing.
     */
    TOOL_WINDOW_ONLY,

    /** No terminal tool window at all; nothing can be focused. */
    NOTHING,
}

/** Thin adapter around the optional JetBrains Terminal plugin API. */
internal object IdeTerminalHost {
    private const val TAB_NAME = "agent-doc"

    fun hasLiveAgentDocTab(project: Project): Boolean = liveTab(project) != null

    /**
     * Focus the agent-doc terminal, reporting what it managed to do.
     *
     * Deliberately resolved through [agentTab], not [liveTab]: a tab whose shell
     * died, or whose widget is not a [ShellTerminalWidget], is still the thing the
     * operator asked to look at. Requiring a connected tty is a precondition for
     * *executing a command* in the tab ([attachExisting]), not for showing it.
     */
    fun focusExisting(project: Project): TerminalFocusOutcome {
        val manager = TerminalToolWindowManager.getInstance(project)
        val toolWindow = manager.toolWindow ?: return TerminalFocusOutcome.NOTHING
        val content = agentTab(toolWindow)
        if (content == null) {
            toolWindow.activate(null)
            return TerminalFocusOutcome.TOOL_WINDOW_ONLY
        }
        focus(manager, content)
        return TerminalFocusOutcome.AGENT_TAB
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

    private fun agentTab(
        toolWindow: com.intellij.openapi.wm.ToolWindow,
    ): Content? = toolWindow.contentManager.contents.firstOrNull { it.displayName == TAB_NAME }

    private fun liveTab(
        project: Project,
    ): Triple<TerminalToolWindowManager, Content, ShellTerminalWidget>? {
        val manager = TerminalToolWindowManager.getInstance(project)
        val toolWindow = manager.toolWindow ?: return null
        val content = agentTab(toolWindow) ?: return null
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

    /**
     * The pure decision behind [focusExisting], so the outcome contract is
     * testable without an IDE fixture — the same `internal fun` decision-helper
     * pattern used by `TmuxPaneFocusSync.decideTmuxFocusMirror`.
     */
    internal fun decideTerminalFocus(
        hasToolWindow: Boolean,
        hasAgentTab: Boolean,
    ): TerminalFocusOutcome = when {
        !hasToolWindow -> TerminalFocusOutcome.NOTHING
        hasAgentTab -> TerminalFocusOutcome.AGENT_TAB
        else -> TerminalFocusOutcome.TOOL_WINDOW_ONLY
    }
}
