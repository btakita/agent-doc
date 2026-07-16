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
                fdm.getDocument(file)?.let { document ->
                    try {
                        fdm.saveDocument(document)
                    } catch (t: Throwable) {
                        log.warn(
                            "[compact] active document save failed; continuing with editor authority: ${t.message}",
                            t,
                        )
                    }
                }
                TerminalUtil.compactExchange(project, file) {
                    refresher.clearTransientStatus(file.path, statusToken)
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
