package com.github.btakita.agentdoc

import com.intellij.openapi.actionSystem.ActionUpdateThread
import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.project.DumbAware
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.LocalFileSystem
import java.io.File

/**
 * `gvqv`: "Dashboard" shows the Agent Doc dashboard in the editor.
 *
 * Runs `agent-doc dashboard --write`, which renders the fleet work board plus
 * controller/supervisor liveness into the generated projection
 * `.agent-doc/dashboard.md` and arms the project controller to keep it current.
 * The action then opens that file (focus-taking: this is an explicit operator
 * request, not a background delivery). The projection is never a session
 * document, so tab sync, CRDT registration, and layout sync all ignore it.
 *
 * Thin event-reporter: the binary owns rendering, refresh, and atomic writes.
 * Project-scoped, so it is enabled whenever a project is open.
 */
class DashboardAction : AnAction(), DumbAware {
    companion object {
        private val LOG = Logger.getInstance(DashboardAction::class.java)

        /** Project-root-relative projection path; mirrors the binary's default. */
        internal const val DASHBOARD_RELATIVE_PATH = ".agent-doc/dashboard.md"

        internal val DASHBOARD_COMMAND_ARGS = listOf("dashboard", "--write")

        internal fun dashboardPath(projectRoot: String): String =
            File(projectRoot, DASHBOARD_RELATIVE_PATH).path

        /** Pure enable gate (testable). */
        internal fun shouldEnable(hasProject: Boolean): Boolean = hasProject

        fun showDashboard(project: Project) {
            val projectRoot = TerminalUtil.cleanupProjectRoot(project)
            val agentDoc = TerminalUtil.resolveAgentDoc(projectRoot)
            TerminalUtil.showHint(project, "Dashboard: rendering agent-doc dashboard…")
            Thread {
                try {
                    val cmd = listOf(agentDoc) + DASHBOARD_COMMAND_ARGS
                    val result = SyncLayoutAction.runCommandWithTimeout(cmd, projectRoot)
                    LOG.info("[dashboard] exit=${result.exitCode} cmd=${cmd.joinToString(" ")}")
                    if (result.exitCode != 0) {
                        val reason = if (result.timedOut) "timed out" else "failed (exit ${result.exitCode})"
                        TerminalUtil.notifyError(project, "Dashboard $reason:\n${result.output}")
                        return@Thread
                    }
                    openDashboard(project, dashboardPath(projectRoot))
                } catch (e: Exception) {
                    TerminalUtil.notifyError(project, "Dashboard failed: ${e.message}")
                }
            }.start()
        }

        private fun openDashboard(project: Project, path: String) {
            ApplicationManager.getApplication().invokeLater {
                if (project.isDisposed) return@invokeLater
                val file = LocalFileSystem.getInstance().refreshAndFindFileByIoFile(File(path))
                if (file == null) {
                    TerminalUtil.notifyError(project, "Dashboard: $path was not written")
                    return@invokeLater
                }
                // The controller rewrites the file atomically; refresh so an
                // already-open tab shows the latest projection immediately.
                file.refresh(false, false)
                FileEditorManager.getInstance(project).openFile(file, true)
            }
        }
    }

    override fun actionPerformed(e: AnActionEvent) {
        val project = e.project ?: return
        showDashboard(project)
    }

    override fun update(e: AnActionEvent) {
        e.presentation.isEnabledAndVisible = shouldEnable(e.project != null)
    }

    override fun getActionUpdateThread(): ActionUpdateThread {
        return ActionUpdateThread.BGT
    }
}
