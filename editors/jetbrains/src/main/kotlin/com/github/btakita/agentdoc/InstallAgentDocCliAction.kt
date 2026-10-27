package com.github.btakita.agentdoc

import com.github.btakita.agentdoc.shell.ImplAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.project.Project
import com.intellij.openapi.ui.Messages

/** Installs or updates the agent-doc CLI through the checked official installer. */
class InstallAgentDocCliAction : ImplAction {
    companion object {
        private val LOG = Logger.getInstance(InstallAgentDocCliAction::class.java)

        internal const val INSTALLER_URL =
            "https://raw.githubusercontent.com/btakita/agent-doc/main/install.sh"
        internal const val INSTALL_COMMAND = "curl -fsSL $INSTALLER_URL | sh"
        internal val INSTALL_COMMAND_ARGS = listOf("sh", "-c", INSTALL_COMMAND)
        private const val INSTALL_TIMEOUT_MS = 5 * 60 * 1000L
        private const val VERIFY_TIMEOUT_MS = 10_000L
        private const val MAX_ERROR_CHARS = 2_000

        internal fun shouldEnable(hasProject: Boolean): Boolean = hasProject

        internal fun confirmationMessage(): String =
            """
            Install or update the agent-doc command-line tool for your user account?

            The plugin will run the official checksummed installer:
            $INSTALL_COMMAND

            The installer writes to ~/.cargo/bin when it exists, otherwise ~/.local/bin.
            """.trimIndent()

        internal fun conciseFailure(output: String): String =
            output.trim().let { text ->
                when {
                    text.isEmpty() -> "No installer output was returned."
                    text.length <= MAX_ERROR_CHARS -> text
                    else -> "…" + text.takeLast(MAX_ERROR_CHARS)
                }
            }

        internal fun install(project: Project) {
            val projectRoot = TerminalUtil.cleanupProjectRoot(project)
            TerminalUtil.showHint(project, "Installing agent-doc CLI…")
            GenerationWork.executeOnPooledThread {
                try {
                    val result =
                        SyncLayoutAction.runCommandWithTimeout(
                            INSTALL_COMMAND_ARGS,
                            projectRoot,
                            INSTALL_TIMEOUT_MS,
                        )
                    LOG.info("[install-cli] exit=${result.exitCode} timedOut=${result.timedOut}")
                    if (result.exitCode != 0) {
                        val reason = if (result.timedOut) "timed out" else "failed (exit ${result.exitCode})"
                        TerminalUtil.notifyError(
                            project,
                            "Agent Doc CLI installation $reason:\n${conciseFailure(result.output)}",
                        )
                        return@executeOnPooledThread
                    }

                    val executable = TerminalUtil.resolveAgentDoc(projectRoot)
                    val version =
                        SyncLayoutAction.runCommandWithTimeout(
                            listOf(executable, "--version"),
                            projectRoot,
                            VERIFY_TIMEOUT_MS,
                        )
                    if (version.exitCode != 0) {
                        TerminalUtil.notifyError(
                            project,
                            "The installer completed, but the plugin could not run agent-doc. " +
                                "Restart the IDE if your PATH changed, then use About Agent Doc to verify it.",
                        )
                        return@executeOnPooledThread
                    }

                    TerminalUtil.notifyInfo(
                        project,
                        "Installed ${version.output.trim().ifEmpty { "agent-doc" }} at $executable",
                    )
                } catch (error: Exception) {
                    LOG.warn("Agent Doc CLI installation failed", error)
                    val platformHint =
                        if (System.getProperty("os.name", "").lowercase().contains("windows")) {
                            " On Windows, run this from a POSIX shell or use `py -m pip install --user --upgrade agent-doc`."
                        } else {
                            ""
                        }
                    TerminalUtil.notifyError(
                        project,
                        "Agent Doc CLI installation failed: ${error.message ?: error.javaClass.simpleName}.$platformHint",
                    )
                }
            }
        }
    }

    override fun actionPerformed(e: AnActionEvent) {
        val project = e.project ?: return
        val decision =
            Messages.showYesNoDialog(
                project,
                confirmationMessage(),
                "Install Agent Doc CLI",
                "Install",
                "Cancel",
                Messages.getQuestionIcon(),
            )
        if (decision == Messages.YES) install(project)
    }

    override fun update(e: AnActionEvent) {
        e.presentation.isEnabledAndVisible = shouldEnable(e.project != null)
    }
}
