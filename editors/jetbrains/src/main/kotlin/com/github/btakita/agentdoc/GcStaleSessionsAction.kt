package com.github.btakita.agentdoc

import com.github.btakita.agentdoc.shell.ImplAction
import com.intellij.openapi.actionSystem.AnActionEvent

/**
 * #plugin-cleanup-menu-command: "GC Stale Sessions" wraps `agent-doc gc` so an
 * operator can reap orphaned panes and prune stale startup/diagnostic state from
 * the IDE without a terminal.
 *
 * Thin event-reporter: the CLI owns all cleanup logic; this only dispatches and
 * surfaces the result. Project-scoped (not file-scoped) — enabled whenever a
 * project is open.
 */
class GcStaleSessionsAction : ImplAction {
    override fun actionPerformed(e: AnActionEvent) {
        val project = e.project ?: return
        TerminalUtil.gcStaleSessions(project)
    }

    override fun update(e: AnActionEvent) {
        e.presentation.isEnabledAndVisible = e.project != null
    }
}
