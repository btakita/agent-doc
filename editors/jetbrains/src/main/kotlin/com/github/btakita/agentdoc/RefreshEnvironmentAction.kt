package com.github.btakita.agentdoc

import com.intellij.openapi.actionSystem.ActionUpdateThread
import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.actionSystem.CommonDataKeys
import com.intellij.openapi.vfs.LocalFileSystem
import java.io.File

/**
 * Refreshes the Agent Doc environment without restarting the IDE.
 *
 * - Clears the slash command completion cache so the next autocomplete
 *   re-runs `agent-doc commands` and picks up newly installed skills.
 * - Forces IntelliJ's VFS to re-scan `~/.claude/` and `<project>/.claude/`
 *   (equivalent to Ctrl+Alt+Y / Synchronize), so externally written skill
 *   files are visible to all registered VirtualFileListeners.
 *
 * Triggered via the Alt+Space popup menu or Ctrl+Shift+Alt+R.
 */
class RefreshEnvironmentAction : AnAction() {

    override fun actionPerformed(e: AnActionEvent) {
        val project = e.project ?: return

        // 1. Clear the slash command cache — next completion re-runs agent-doc commands
        SlashCommandCompletionContributor.clearCache()

        // 2. Force VFS rescan on global ~/.claude/ and project-local .claude/
        val toRefresh = buildList {
            add(File(System.getProperty("user.home"), ".claude"))
            project.basePath?.let { add(File(it, ".claude")) }
        }
        for (dir in toRefresh) {
            if (dir.exists()) {
                LocalFileSystem.getInstance().findFileByIoFile(dir)?.refresh(true, true)
            }
        }

        TerminalUtil.showHint(project, "Agent Doc environment refreshed")
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
