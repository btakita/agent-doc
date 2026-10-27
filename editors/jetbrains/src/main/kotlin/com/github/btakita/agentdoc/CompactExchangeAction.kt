package com.github.btakita.agentdoc

import com.github.btakita.agentdoc.shell.ImplAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.actionSystem.CommonDataKeys
import com.intellij.openapi.fileEditor.FileDocumentManager

class CompactExchangeAction : ImplAction {
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
        GenerationWork.invokeLater {
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
                GenerationWork.executeOnPooledThread {
                    val reloadReady = NativeReloadCoordinator.awaitReady()
                    val attached = reloadReady &&
                        CrdtReplicaManager.ensureReplicaForOpenDocument(
                            project,
                            file.path,
                            document,
                            editorText = editorText,
                            await = true,
                        )
                    // Snapshot the cause on the same worker that observed the
                    // refusal. A scheduled registration retry may otherwise
                    // clear it before the EDT formats the notification.
                    val attachFailureReason = if (attached) {
                        null
                    } else if (reloadReady) {
                        CrdtReplicaManager.lastAttachFailureReason(file.path)
                            ?: "unknown-attach-refusal"
                    } else {
                        "native-handoff-timeout"
                    }
                    // GH #326 phase 5b: diagnose the cause and format the refusal on this
                    // worker. The diagnosis is a native derivation, so it never runs on the
                    // EDT; the EDT only shows the message.
                    val attachFailureDiagnosis =
                        attachFailureReason?.let { reason ->
                            AttachFailureDiagnosis.diagnose(reason).also { diagnosis ->
                                recordAttachRefusal(project.basePath, file.path, reason, diagnosis.statusToken)
                            }
                        }
                    // `#replicarefusalreason`: report the refusal CAUSE and its remedy.
                    // "Could not be attached" alone points the operator at the controller,
                    // which is usually healthy and reports ready — observed twice on
                    // 2026-08-11, where the real cause was an IDE running a plugin
                    // generation older than the installed jar.
                    val refusalMessage =
                        if (attached) {
                            null
                        } else {
                            attachRefusalMessage(attachFailureReason, attachFailureDiagnosis?.remedy)
                        }
                    GenerationWork.invokeLater {
                        if (project.isDisposed) {
                            refresher.clearTransientStatus(file.path, statusToken)
                            return@invokeLater
                        }
                        if (refusalMessage != null) {
                            refresher.clearTransientStatus(file.path, statusToken)
                            TerminalUtil.notifyError(project, refusalMessage)
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

    private fun attachRefusalMessage(reason: String?, remedy: String?): String =
        buildString {
            append(
                "Compact Exchange was not started because the open editor replica " +
                    "could not be attached to its owning project controller.",
            )
            if (reason != null) append(" Cause: $reason.")
            if (remedy != null) append(" $remedy")
            append(" The editor buffer and disk were left unchanged.")
        }

    private fun recordAttachRefusal(projectRoot: String?, filePath: String, reason: String, statusToken: String) {
        if (projectRoot == null) {
            log.warn("[compact] cannot record replica attach refusal: project root unavailable")
            return
        }
        val lib = AgentDocLib.get()
        if (lib == null) {
            log.warn("[compact] cannot record replica attach refusal: native library unavailable; reason=$reason")
            return
        }
        val status = "attach_refused_$statusToken"
        try {
            if (!lib.agent_doc_record_editor_surface_event(
                    projectRoot,
                    "jetbrains",
                    filePath,
                    "compact_exchange",
                    "replica_attach",
                    "compact_exchange",
                    null,
                    status,
                )) {
                log.warn("[compact] native replica attach refusal event rejected: status=$status")
            }
        } catch (t: Throwable) {
            log.warn("[compact] replica attach refusal event ABI failed: ${t.message}", t)
        }
    }

    private companion object {
        const val COMPACTING_EXCHANGE_LABEL = "⟳ agent-doc: Compacting Exchange"
    }
}
