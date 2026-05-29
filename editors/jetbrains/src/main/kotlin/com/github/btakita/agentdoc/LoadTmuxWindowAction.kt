package com.github.btakita.agentdoc

import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.actionSystem.CommonDataKeys
import com.intellij.openapi.diagnostic.Logger

/**
 * Action that loads (autostarts) the tmux window for the focused .md session
 * document on demand.
 *
 * Opening a document only fires the passive `agent-doc sync --no-autostart`
 * path (see [EditorTabSyncListener]) so the binary never spawns panes behind
 * the user's back — that passive path deliberately avoids attach-first pane
 * growth. As a result a freshly-opened doc whose tmux pane is not already up
 * does NOT autoload one. This action is the explicit, discoverable on-demand
 * loader the user reaches for in that case.
 *
 * It runs the proven autostart layout-sync path
 * ([SyncLayoutAction.syncLayout] with `noAutostart = false`), which ensures the
 * pane exists and the harness is started via `route::auto_start`. We do NOT
 * call `agent-doc start` directly: that runs the harness as a blocking child
 * inside a restart loop and is not appropriate to invoke from a plugin thread.
 */
class LoadTmuxWindowAction : AnAction() {

    companion object {
        private val LOG = Logger.getInstance(LoadTmuxWindowAction::class.java)

        /** Pure enable gate (testable): only enabled for markdown documents. */
        internal fun shouldEnable(extension: String?): Boolean =
            extension?.lowercase() == "md"
    }

    override fun actionPerformed(e: AnActionEvent) {
        val project = e.project ?: return
        TerminalUtil.showHint(project, "Loading tmux window…")
        // syncLayout runs on a background thread and is safe to call from EDT.
        SyncLayoutAction.syncLayout(project, notify = true, noAutostart = false)
    }

    override fun update(e: AnActionEvent) {
        val file = e.getData(CommonDataKeys.VIRTUAL_FILE)
        e.presentation.isEnabledAndVisible = shouldEnable(file?.extension)
    }
}
