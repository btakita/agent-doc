package com.github.btakita.agentdoc

import com.intellij.codeInsight.hint.HintManager
import com.intellij.notification.NotificationGroupManager
import com.intellij.notification.NotificationType
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VirtualFile

object TerminalUtil {

    fun relativePath(project: Project, file: VirtualFile): String {
        val basePath = project.basePath ?: return file.path
        return if (file.path.startsWith(basePath)) {
            file.path.removePrefix("$basePath/")
        } else {
            file.path
        }
    }

    /**
     * Routes an /agent-doc command via `agent-doc route`.
     *
     * This calls `agent-doc route <path>` which:
     * 1. Reads the session UUID from the file's frontmatter
     * 2. Looks up the tmux pane for that session
     * 3. Sends the command via `tmux send-keys`
     * 4. Auto-starts a new Claude session if needed
     */
    fun sendToTerminal(project: Project, relativePath: String) {
        val basePath = project.basePath ?: return

        val agentDoc = resolveAgentDoc()
        try {
            val process = ProcessBuilder(agentDoc, "route", relativePath)
                .directory(java.io.File(basePath))
                .redirectErrorStream(true)
                .start()

            // Show quick inline hint near cursor
            showHint(project, "Routed $relativePath")

            // Read output in background thread to avoid blocking EDT
            Thread {
                val output = process.inputStream.bufferedReader().readText()
                val exitCode = process.waitFor()
                if (exitCode != 0) {
                    notifyError(project, "agent-doc route failed (exit $exitCode):\n$output")
                }
            }.start()
        } catch (e: Exception) {
            notifyError(project, "Failed to run agent-doc: ${e.message}\nLooked for: $agentDoc")
        }
    }

    fun resolveAgentDoc(): String {
        val candidates = listOf(
            System.getenv("HOME")?.let { "$it/bin/agent-doc" },
            System.getenv("HOME")?.let { "$it/.local/bin/agent-doc" },
            System.getenv("HOME")?.let { "$it/.cargo/bin/agent-doc" },
            "/usr/local/bin/agent-doc"
        )
        for (path in candidates) {
            if (path != null && java.io.File(path).canExecute()) {
                return path
            }
        }
        return "agent-doc"
    }

    fun showHint(project: Project, message: String) {
        ApplicationManager.getApplication().invokeLater {
            val editor = FileEditorManager.getInstance(project).selectedTextEditor ?: return@invokeLater
            HintManager.getInstance().showInformationHint(editor, message)
        }
    }

    fun notifyError(project: Project, content: String) {
        try {
            NotificationGroupManager.getInstance()
                .getNotificationGroup("Agent Doc")
                .createNotification(content, NotificationType.ERROR)
                .notify(project)
        } catch (_: Exception) {
            System.err.println("[agent-doc] $content")
        }
    }

    fun notifyInfo(project: Project, content: String) {
        try {
            val notification = NotificationGroupManager.getInstance()
                .getNotificationGroup("Agent Doc")
                .createNotification(content, NotificationType.INFORMATION)
            notification.notify(project)
            // Auto-expire after 3 seconds
            Thread {
                Thread.sleep(3000)
                notification.expire()
            }.start()
        } catch (_: Exception) {
            System.err.println("[agent-doc] $content")
        }
    }
}
