package com.github.btakita.agentdoc

import com.intellij.openapi.actionSystem.ActionUpdateThread
import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent

/**
 * #plugin-cleanup-menu-command: "Resync / Fix Sessions" wraps
 * `agent-doc resync --fix` so an operator can repair stale routing/registry
 * state and reap dead/wrong-session panes from the IDE without a terminal.
 *
 * Thin event-reporter: the CLI owns all cleanup logic; this only dispatches and
 * surfaces the result. Project-scoped (not file-scoped) — enabled whenever a
 * project is open.
 */
class ResyncFixSessionsAction : AnAction() {
    override fun actionPerformed(e: AnActionEvent) {
        val project = e.project ?: return
        TerminalUtil.resyncFixSessions(project)
    }

    override fun update(e: AnActionEvent) {
        e.presentation.isEnabledAndVisible = e.project != null
    }

    override fun getActionUpdateThread(): ActionUpdateThread {
        return ActionUpdateThread.BGT
    }
}
