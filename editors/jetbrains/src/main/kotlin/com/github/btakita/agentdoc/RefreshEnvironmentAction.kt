package com.github.btakita.agentdoc

import com.intellij.openapi.actionSystem.ActionUpdateThread
import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.actionSystem.CommonDataKeys
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.project.Project
import com.intellij.openapi.ui.popup.JBPopupFactory
import com.intellij.openapi.vfs.LocalFileSystem
import java.io.File

/**
 * Refreshes the Agent Doc environment without restarting the IDE.
 *
 * Workflow:
 *  1. Lists running tmux sessions and shows a JBPopup picker (most
 *     recently active first). Selecting one runs `agent-doc session set
 *     <name>` from the project root, rebinding the project to that
 *     tmux session and migrating any registered panes.
 *  2. Clears the slash command completion cache so the next autocomplete
 *     re-runs `agent-doc commands` and picks up newly installed skills.
 *  3. Forces IntelliJ's VFS to re-scan `~/.claude/` and `<project>/.claude/`
 *     (equivalent to Ctrl+Alt+Y / Synchronize), so externally written skill
 *     files are visible to all registered VirtualFileListeners.
 *
 * Triggered via the Alt+Space popup menu or Ctrl+Shift+Alt+R.
 */
class RefreshEnvironmentAction : AnAction() {

    private data class TmuxSession(
        val name: String,
        val attached: Int,
        val windows: Int,
        val activity: Long,
    ) {
        fun label(): String {
            val attachedMarker = if (attached > 0) "●" else "○"
            return "$attachedMarker $name  (${windows}w)"
        }
    }

    override fun actionPerformed(e: AnActionEvent) {
        val project = e.project ?: return

        val sessions = listTmuxSessions()
        if (sessions.isEmpty()) {
            // No tmux sessions (or tmux not running) — skip bind step and go
            // straight to the refresh so this still works outside tmux.
            refreshCachesAndVfs(project)
            TerminalUtil.showHint(project, "Agent Doc environment refreshed (no tmux sessions)")
            return
        }

        val sorted = sessions.sortedWith(
            compareByDescending<TmuxSession> { it.attached > 0 }.thenByDescending { it.activity }
        )

        val popup = JBPopupFactory.getInstance()
            .createPopupChooserBuilder(sorted)
            .setTitle("Bind Project to Tmux Session")
            .setItemChosenCallback { selected ->
                bindSession(project, selected.name)
                refreshCachesAndVfs(project)
            }
            .setRenderer { _, value, _, _, _ ->
                javax.swing.JLabel(value.label())
            }
            .createPopup()

        val editor = e.getData(CommonDataKeys.EDITOR)
        if (editor != null) {
            popup.showInBestPositionFor(editor)
        } else {
            popup.showCenteredInCurrentWindow(project)
        }
    }

    private fun listTmuxSessions(): List<TmuxSession> {
        return try {
            val process = ProcessBuilder(
                "tmux", "list-sessions",
                "-F", "#{session_name}|#{session_attached}|#{session_windows}|#{session_activity}"
            ).redirectErrorStream(false).start()
            val output = process.inputStream.bufferedReader().readText()
            process.waitFor()
            output.lines()
                .mapNotNull { line ->
                    val parts = line.split("|")
                    if (parts.size < 4) return@mapNotNull null
                    TmuxSession(
                        name = parts[0],
                        attached = parts[1].toIntOrNull() ?: 0,
                        windows = parts[2].toIntOrNull() ?: 0,
                        activity = parts[3].toLongOrNull() ?: 0L,
                    )
                }
        } catch (_: Exception) {
            emptyList()
        }
    }

    private fun bindSession(project: Project, sessionName: String) {
        val basePath = project.basePath ?: return
        val agentDoc = TerminalUtil.resolveAgentDoc(basePath)
        Thread {
            try {
                val process = ProcessBuilder(agentDoc, "session", "set", sessionName)
                    .directory(File(basePath))
                    .redirectErrorStream(true)
                    .start()
                val output = process.inputStream.bufferedReader().readText()
                val exit = process.waitFor()
                ApplicationManager.getApplication().invokeLater {
                    if (exit == 0) {
                        TerminalUtil.showHint(project, "Bound to tmux session: $sessionName")
                    } else {
                        TerminalUtil.notifyError(
                            project,
                            "agent-doc session set $sessionName failed (exit $exit):\n$output"
                        )
                    }
                }
            } catch (ex: Exception) {
                ApplicationManager.getApplication().invokeLater {
                    TerminalUtil.notifyError(project, "Failed to bind tmux session: ${ex.message}")
                }
            }
        }.start()
    }

    private fun refreshCachesAndVfs(project: Project) {
        SlashCommandCompletionContributor.clearCache()

        val toRefresh = buildList {
            add(File(System.getProperty("user.home"), ".claude"))
            project.basePath?.let { add(File(it, ".claude")) }
        }
        for (dir in toRefresh) {
            if (dir.exists()) {
                LocalFileSystem.getInstance().findFileByIoFile(dir)?.refresh(true, true)
            }
        }
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
