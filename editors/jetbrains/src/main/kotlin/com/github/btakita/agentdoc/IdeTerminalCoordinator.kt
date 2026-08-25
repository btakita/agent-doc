package com.github.btakita.agentdoc

import com.google.gson.JsonParser
import com.intellij.notification.NotificationAction
import com.intellij.notification.NotificationGroupManager
import com.intellij.notification.NotificationType
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.extensions.PluginId
import com.intellij.openapi.ide.CopyPasteManager
import com.intellij.openapi.project.Project
import com.intellij.ide.plugins.PluginManagerCore
import java.awt.datatransfer.StringSelection

internal data class IdeTerminalEnsureOutcome(
    val sessionName: String,
    val paneId: String,
    val attachCommand: String,
    val created: Boolean,
    val attached: Boolean,
)

internal enum class IdeTerminalAttachDecision {
    NOOP_EXTERNAL_ATTACHED,
    FOCUS_EXISTING,
    ATTACH_EXISTING,
    CREATE_AND_ATTACH,
}

internal fun parseIdeTerminalEnsureOutcome(json: String): IdeTerminalEnsureOutcome {
    val value = JsonParser.parseString(json).asJsonObject
    return IdeTerminalEnsureOutcome(
        sessionName = value.get("session_name").asString,
        paneId = value.get("pane_id").asString,
        attachCommand = value.get("attach_command").asString,
        created = value.get("created").asBoolean,
        attached = value.get("attached").asBoolean,
    )
}

internal fun decideIdeTerminalAttach(
    sessionAttached: Boolean,
    existingTabAlive: Boolean,
): IdeTerminalAttachDecision = when {
    sessionAttached && existingTabAlive -> IdeTerminalAttachDecision.FOCUS_EXISTING
    sessionAttached -> IdeTerminalAttachDecision.NOOP_EXTERNAL_ATTACHED
    existingTabAlive -> IdeTerminalAttachDecision.ATTACH_EXISTING
    else -> IdeTerminalAttachDecision.CREATE_AND_ATTACH
}

/**
 * Headless tmux bootstrap shared by Run/Sync editor actions. The binary owns
 * session selection and creation; this coordinator only chooses the IDE
 * presentation after reading that receipt.
 */
internal object IdeTerminalCoordinator {
    private const val TERMINAL_PLUGIN_ID = "org.jetbrains.plugins.terminal"
    private const val ENSURE_TIMEOUT_MS = 10_000L

    fun ensureAndAttach(
        project: Project,
        cwd: String,
        relativePath: String,
        onReady: () -> Unit,
        onFailure: (String) -> Unit,
    ) {
        ApplicationManager.getApplication().executeOnPooledThread {
            val result = try {
                val agentDoc = TerminalUtil.resolveAgentDoc(cwd)
                SyncLayoutAction.runCommandWithTimeout(
                    listOf(agentDoc, "tmux", "ensure", relativePath, "--json"),
                    cwd,
                    ENSURE_TIMEOUT_MS,
                )
            } catch (failure: Throwable) {
                dispatchFailure(project, onFailure, failure.message ?: failure.javaClass.simpleName)
                return@executeOnPooledThread
            }
            if (result.exitCode != 0) {
                val detail = result.output.ifBlank { "agent-doc tmux ensure failed (exit ${result.exitCode})" }
                dispatchFailure(project, onFailure, detail)
                return@executeOnPooledThread
            }
            val outcome = try {
                parseIdeTerminalEnsureOutcome(result.output)
            } catch (failure: Throwable) {
                dispatchFailure(project, onFailure, "invalid tmux ensure receipt: ${failure.message}")
                return@executeOnPooledThread
            }

            ApplicationManager.getApplication().invokeLater {
                if (project.isDisposed) return@invokeLater
                val terminalAvailable =
                    PluginManagerCore.getPlugin(PluginId.getId(TERMINAL_PLUGIN_ID))?.isEnabled == true
                if (!terminalAvailable) {
                    notifyManualAttach(project, outcome.attachCommand, "JetBrains Terminal plugin is unavailable")
                    onReady()
                    return@invokeLater
                }

                try {
                    val existingAlive = IdeTerminalHost.hasLiveAgentDocTab(project)
                    when (decideIdeTerminalAttach(outcome.attached, existingAlive)) {
                        IdeTerminalAttachDecision.NOOP_EXTERNAL_ATTACHED -> Unit
                        IdeTerminalAttachDecision.FOCUS_EXISTING ->
                            IdeTerminalHost.focusExisting(project)
                        IdeTerminalAttachDecision.ATTACH_EXISTING ->
                            IdeTerminalHost.attachExisting(project, outcome.attachCommand)
                        IdeTerminalAttachDecision.CREATE_AND_ATTACH ->
                            IdeTerminalHost.createAndAttach(project, cwd, outcome.attachCommand)
                    }
                } catch (failure: Throwable) {
                    notifyManualAttach(
                        project,
                        outcome.attachCommand,
                        "JetBrains terminal could not open: ${failure.message ?: failure.javaClass.simpleName}",
                    )
                }
                onReady()
            }
        }
    }

    private fun dispatchFailure(project: Project, onFailure: (String) -> Unit, message: String) {
        ApplicationManager.getApplication().invokeLater {
            if (!project.isDisposed) onFailure(message)
        }
    }

    private fun notifyManualAttach(project: Project, attachCommand: String, reason: String) {
        val notification = NotificationGroupManager.getInstance()
            .getNotificationGroup("Agent Doc")
            .createNotification("$reason. Attach manually: $attachCommand", NotificationType.WARNING)
        notification.isImportant = true
        notification.addAction(NotificationAction.createSimple("Copy attach command") {
            CopyPasteManager.getInstance().setContents(StringSelection(attachCommand))
        })
        notification.notify(project)
    }
}
