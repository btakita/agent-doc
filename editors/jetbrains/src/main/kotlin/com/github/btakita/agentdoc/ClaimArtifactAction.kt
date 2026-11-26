package com.github.btakita.agentdoc

import com.github.btakita.agentdoc.shell.ImplAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.actionSystem.CommonDataKeys
import com.intellij.openapi.fileEditor.FileDocumentManager
import com.intellij.openapi.ui.Messages
import com.intellij.openapi.vfs.LocalFileSystem
import java.io.File

/**
 * Claims an artifact session for the focused markdown file.
 *
 * The focused file stays ordinary markdown. The operator confirms a control-session
 * path (default `<stem>.session.md`). A missing control file is created with
 * `agent-doc init <control> --artifact <artifact>`, which leaves an existing ordinary
 * artifact unchanged. The control file, not the artifact, is then claimed for this pane.
 * No new document element is introduced.
 */
class ClaimArtifactAction : ImplAction {

    companion object {
        internal const val DIALOG_TITLE = "Claim Artifact Session — Cross-Session"

        internal fun normalizeSessionPath(value: String): String =
            value.trim().replace('\\', '/').removePrefix("./")

        internal fun defaultControlSessionPath(artifactRel: String): String {
            val normalized = normalizeSessionPath(artifactRel)
            val stem = if (normalized.endsWith(".md", ignoreCase = true)) {
                normalized.dropLast(3)
            } else {
                normalized
            }
            return "$stem.session.md"
        }

        internal fun acceptableControlPath(controlRel: String, artifactRel: String): Boolean {
            val control = normalizeSessionPath(controlRel)
            val artifact = normalizeSessionPath(artifactRel)
            return control.isNotEmpty() &&
                control.endsWith(".md", ignoreCase = true) &&
                control != artifact &&
                !control.startsWith("/") &&
                !control.contains('\n') &&
                !control.contains('\r')
        }

        internal fun buildInitCommand(
            agentDoc: String,
            controlRel: String,
            artifactRel: String,
        ): List<String> = listOf(agentDoc, "init", controlRel, "--artifact", artifactRel)
    }

    override fun actionPerformed(e: AnActionEvent) {
        val project = e.project ?: return
        val file = e.getData(CommonDataKeys.VIRTUAL_FILE) ?: return
        if (file.extension?.lowercase() != "md") return

        val fileDocumentManager = FileDocumentManager.getInstance()
        fileDocumentManager.getDocument(file)?.let { document ->
            fileDocumentManager.saveDocument(document)
            if (fileDocumentManager.isDocumentUnsaved(document)) {
                TerminalUtil.notifyError(project, "Save the document before claiming an artifact session")
                return
            }
        }

        val (cwd, artifactRel) = TerminalUtil.resolveProject(project, file)
        val entered = Messages.showInputDialog(
            project,
            "Control session path. This file is claimed. The open markdown file stays an ordinary artifact.",
            "Claim Artifact Session",
            Messages.getQuestionIcon(),
            defaultControlSessionPath(artifactRel),
            null,
        ) ?: return
        val controlRel = normalizeSessionPath(entered)
        if (!acceptableControlPath(controlRel, artifactRel)) {
            TerminalUtil.notifyError(project, "Control session must be a different .md file")
            return
        }

        val position = ClaimAction.editorPosition(project, file)
        GenerationWork.executeOnPooledThread {
            try {
                val agentDoc = TerminalUtil.resolveAgentDoc(cwd)
                val controlFile = File(cwd, controlRel)
                if (!controlFile.exists()) {
                    val init = SyncLayoutAction.runCommandWithTimeout(
                        buildInitCommand(agentDoc, controlRel, artifactRel),
                        cwd,
                        ClaimAction.CLAIM_PROCESS_TIMEOUT_MS,
                    )
                    if (init.timedOut || init.exitCode != 0) {
                        val detail = init.output.ifEmpty {
                            if (init.timedOut) "timed out" else "exit ${init.exitCode}"
                        }
                        TerminalUtil.notifyError(project, "Claim artifact session failed:\n$detail")
                        return@executeOnPooledThread
                    }
                }
                val virtual = LocalFileSystem.getInstance().refreshAndFindFileByIoFile(controlFile)
                GenerationWork.invokeLater {
                    if (virtual == null) {
                        TerminalUtil.notifyError(
                            project,
                            "Control session was not found after init: $controlRel",
                        )
                        return@invokeLater
                    }
                    ClaimAction().launchClaim(
                        project,
                        virtual,
                        cwd,
                        controlRel,
                        position,
                        force = false,
                        newPane = false,
                        allowCrossSessionPrompt = true,
                        dialogTitle = DIALOG_TITLE,
                    )
                }
            } catch (ex: Exception) {
                TerminalUtil.notifyError(project, "Failed to claim artifact session: ${ex.message}")
            }
        }
    }

    override fun update(e: AnActionEvent) {
        val file = e.getData(CommonDataKeys.VIRTUAL_FILE)
        e.presentation.isEnabledAndVisible =
            file != null && file.extension?.lowercase() == "md"
    }
}
