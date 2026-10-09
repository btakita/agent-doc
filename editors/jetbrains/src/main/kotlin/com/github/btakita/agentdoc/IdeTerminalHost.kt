package com.github.btakita.agentdoc

import com.intellij.openapi.project.Project
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.wm.RegisterToolWindowTask
import com.intellij.openapi.wm.ToolWindow
import com.intellij.openapi.wm.ToolWindowAnchor
import com.intellij.openapi.wm.ToolWindowManager
import com.intellij.openapi.wm.impl.ToolWindowManagerImpl
import com.intellij.ui.content.Content
import org.jetbrains.plugins.terminal.ShellTerminalWidget
import org.jetbrains.plugins.terminal.TerminalToolWindowManager
import java.lang.reflect.Method
import java.util.concurrent.ConcurrentHashMap

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
    private const val TOOL_WINDOW_ID = "Agent Doc Terminal"
    private val desiredPaneByProject = ConcurrentHashMap<Project, String>()
    private val LOG = Logger.getInstance(IdeTerminalHost::class.java)

    fun disposeProject(project: Project) {
        desiredPaneByProject.remove(project)
        restoreStockTab(project)
        val manager = ToolWindowManager.getInstance(project)
        if (manager.getToolWindow(TOOL_WINDOW_ID) != null) {
            manager.unregisterToolWindow(TOOL_WINDOW_ID)
        }
    }

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
        val toolWindow = agentToolWindow(project) ?: manager.toolWindow
            ?: return TerminalFocusOutcome.NOTHING
        val content = sequenceOf(agentToolWindow(project), manager.toolWindow)
            .filterNotNull()
            .mapNotNull(::agentTab)
            .firstOrNull()
        if (content == null) {
            toolWindow.activate(null)
            return TerminalFocusOutcome.TOOL_WINDOW_ONLY
        }
        focus(project, manager, content)
        return TerminalFocusOutcome.AGENT_TAB
    }

    fun attachExisting(project: Project, attachCommand: String) {
        val (manager, content, widget) = liveTab(project)
            ?: error("agent-doc terminal tab is no longer alive")
        focus(project, manager, content)
        widget.executeCommand(attachCommand)
    }

    fun createAndAttach(project: Project, cwd: String, attachCommand: String) {
        val manager = TerminalToolWindowManager.getInstance(project)
        val widget = manager.createLocalShellWidget(cwd, TAB_NAME)
        migrateStockTab(project, manager, widget)
        widget.executeCommand(attachCommand)
        agentToolWindow(project)?.activate(null)
    }

    /** Apply the controller-owned terminal placement receipt on the EDT. */
    fun applySurfaceDecision(project: Project, receiptJson: String) {
        val decision = parseTerminalSurfaceDecision(receiptJson) ?: return
        when (decision.kind) {
            TerminalSurfaceDecisionKind.MOUNT,
            TerminalSurfaceDecisionKind.ALREADY_MOUNTED -> {
                val surfaceId = decision.surfaceId ?: return
                desiredPaneByProject[project] = surfaceId
                migrateExistingStockTab(project)
                relocateAgentToolWindow(project, surfaceId)
            }

            TerminalSurfaceDecisionKind.STASH -> {
                desiredPaneByProject.remove(project)
                agentToolWindow(project)?.let { toolWindow ->
                    toolWindow.hide(null)
                    toolWindow.setAvailable(false)
                }
            }

            TerminalSurfaceDecisionKind.EXCLUDE,
            TerminalSurfaceDecisionKind.STALE,
            TerminalSurfaceDecisionKind.IDLE -> Unit
        }
    }

    private fun agentTab(
        toolWindow: ToolWindow,
    ): Content? = toolWindow.contentManager.contents.firstOrNull { it.displayName == TAB_NAME }

    private fun liveTab(
        project: Project,
    ): Triple<TerminalToolWindowManager, Content, ShellTerminalWidget>? {
        val manager = TerminalToolWindowManager.getInstance(project)
        val content = sequenceOf(agentToolWindow(project), manager.toolWindow)
            .filterNotNull()
            .mapNotNull(::agentTab)
            .firstOrNull()
            ?: return null
        val widget = TerminalToolWindowManager.getWidgetByContent(content) as? ShellTerminalWidget
            ?: return null
        val connector = widget.processTtyConnector ?: return null
        if (!connector.isConnected) return null
        return Triple(manager, content, widget)
    }

    private fun focus(project: Project, manager: TerminalToolWindowManager, content: Content) {
        val toolWindow = sequenceOf(
            ToolWindowManager.getInstance(project).getToolWindow(TOOL_WINDOW_ID),
            manager.toolWindow,
        ).filterNotNull().firstOrNull { candidate ->
            candidate.contentManager.getIndexOfContent(content) >= 0
        }
        toolWindow?.let {
            it.contentManager.setSelectedContent(content)
            it.activate(null)
        }
    }

    private fun agentToolWindow(project: Project): ToolWindow? =
        ToolWindowManager.getInstance(project).getToolWindow(TOOL_WINDOW_ID)

    private fun ensureAgentToolWindow(project: Project): ToolWindow {
        val manager = ToolWindowManager.getInstance(project)
        return manager.getToolWindow(TOOL_WINDOW_ID)
            ?: manager.registerToolWindow(
                RegisterToolWindowTask.notClosable(TOOL_WINDOW_ID, ToolWindowAnchor.BOTTOM),
            )
    }

    private fun migrateStockTab(
        project: Project,
        manager: TerminalToolWindowManager,
        widget: ShellTerminalWidget,
    ) {
        if (!supportsPaneRelocation(project)) return
        val stock = manager.toolWindow ?: return
        val stockContent = stock.contentManager.contents.firstOrNull {
            TerminalToolWindowManager.getWidgetByContent(it) === widget
        } ?: return
        val terminalWidget = TerminalToolWindowManager.findWidgetByContent(stockContent) ?: return
        val agentToolWindow = ensureAgentToolWindow(project)
        manager.detachWidgetAndRemoveContent(stockContent)
        val content = manager.newTab(agentToolWindow, terminalWidget)
        content.displayName = TAB_NAME
        desiredPaneByProject[project]?.let { relocateAgentToolWindow(project, it) }
    }

    private fun migrateExistingStockTab(project: Project) {
        if (agentToolWindow(project)?.let(::agentTab) != null) return
        val manager = TerminalToolWindowManager.getInstance(project)
        val stockContent = manager.toolWindow?.let(::agentTab) ?: return
        val widget = TerminalToolWindowManager.getWidgetByContent(stockContent) as? ShellTerminalWidget
            ?: return
        migrateStockTab(project, manager, widget)
    }

    private fun restoreStockTab(project: Project) {
        val agentContent = agentToolWindow(project)?.let(::agentTab) ?: return
        val manager = TerminalToolWindowManager.getInstance(project)
        val stock = manager.toolWindow ?: return
        val widget = TerminalToolWindowManager.findWidgetByContent(agentContent) ?: return
        manager.detachWidgetAndRemoveContent(agentContent)
        val content = manager.newTab(stock, widget)
        content.displayName = TAB_NAME
    }

    /**
     * Mount the dedicated Agent Doc tool window in one ToolWindowPane. The
     * internal API is capability-probed so unsupported IDE versions leave the
     * existing terminal untouched.
     */
    private fun relocateAgentToolWindow(project: Project, paneId: String) {
        val toolWindow = agentToolWindow(project) ?: return
        val manager = ToolWindowManager.getInstance(project) as? ToolWindowManagerImpl ?: return
        try {
            val method = paneRelocationMethod(manager) ?: return
            method.invoke(manager, TOOL_WINDOW_ID, paneId, ToolWindowAnchor.BOTTOM, 0, false)
            toolWindow.setAvailable(true)
            toolWindow.show(null)
        } catch (error: Throwable) {
            // Missing internal capability is the documented no-terminal-surface row.
            LOG.debug("[surface] per-pane tool-window relocation unavailable", error)
        }
    }

    private fun supportsPaneRelocation(project: Project): Boolean {
        val manager = ToolWindowManager.getInstance(project) as? ToolWindowManagerImpl
            ?: return false
        return paneRelocationMethod(manager) != null
    }

    private fun paneRelocationMethod(manager: ToolWindowManagerImpl): Method? =
        manager.javaClass.methods.firstOrNull {
            it.name.startsWith("setSideToolAndAnchor") && it.parameterCount == 5
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
