package com.github.btakita.agentdoc

import com.intellij.openapi.actionSystem.ActionUpdateThread
import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.actionSystem.CommonDataKeys
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.fileEditor.FileDocumentManager

class CompactExchangeAction : AnAction() {
    private val log = com.intellij.openapi.diagnostic.Logger.getInstance(CompactExchangeAction::class.java)

    override fun actionPerformed(e: AnActionEvent) {
        val project = e.project ?: return
        val file = e.getData(CommonDataKeys.VIRTUAL_FILE) ?: return
        val refresher = TurnStateBannerRefresher.getInstance(project)
        val statusToken = refresher.showTransientStatus(
            file.path,
            COMPACTING_EXCHANGE_LABEL,
            "Compacting the exchange and committing the authoritative document state",
        )
        ApplicationManager.getApplication().invokeLater {
            try {
                // Compact Exchange is document-scoped. Saving every open
                // document can synchronously wake a retained ACK recovery for
                // an unrelated session and make this command fail with that
                // other file's error. The live target document/CRDT remains
                // authoritative even if IntelliJ's best-effort save fails.
                val fdm = FileDocumentManager.getInstance()
                val document = fdm.getDocument(file)
                document?.let {
                    try {
                        fdm.saveDocument(it)
                    } catch (t: Throwable) {
                        log.warn(
                            "[compact] active document save failed; continuing with editor authority: ${t.message}",
                            t,
                        )
                    }
                }
                if (document == null) {
                    TerminalUtil.compactExchange(project, file) {
                        refresher.clearTransientStatus(file.path, statusToken)
                    }
                    return@invokeLater
                }

                // Compact Exchange must never race ahead of this open
                // document's controller registration. Capture the exact editor
                // cut on the EDT, then perform the bounded native/controller
                // registration wait on a pooled thread. If attachment cannot be
                // proven, fail before launching the command; the binary must not
                // infer detached disk authority behind an open IntelliJ buffer.
                val editorText = document.text
                ApplicationManager.getApplication().executeOnPooledThread {
                    val attached =
                        CrdtReplicaManager.ensureReplicaForOpenDocument(
                            file.path,
                            document,
                            editorText = editorText,
                            await = true,
                        )
                    ApplicationManager.getApplication().invokeLater {
                        if (project.isDisposed) {
                            refresher.clearTransientStatus(file.path, statusToken)
                            return@invokeLater
                        }
                        if (!attached) {
                            refresher.clearTransientStatus(file.path, statusToken)
                            TerminalUtil.notifyError(
                                project,
                                "Compact Exchange was not started because the open editor replica " +
                                    "could not be attached to its owning project controller. " +
                                    "The editor buffer and disk were left unchanged.",
                            )
                            return@invokeLater
                        }
                        TerminalUtil.compactExchange(project, file) {
                            refresher.clearTransientStatus(file.path, statusToken)
                        }
                    }
                }
            } catch (t: Throwable) {
                refresher.clearTransientStatus(file.path, statusToken)
                throw t
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

    private companion object {
        const val COMPACTING_EXCHANGE_LABEL = "⟳ agent-doc: Compacting Exchange"
    }
}
