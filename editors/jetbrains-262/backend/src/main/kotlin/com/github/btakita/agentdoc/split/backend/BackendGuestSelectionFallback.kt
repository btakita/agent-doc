@file:Suppress("UnstableApiUsage")

package com.github.btakita.agentdoc.split.backend

import com.github.btakita.agentdoc.EditorTabSyncListener
import com.intellij.openapi.Disposable
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.client.ClientKind
import com.intellij.openapi.client.ClientProjectSession
import com.intellij.openapi.client.ClientSessionsManager
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.fileEditor.ClientFileEditorManager
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.fileEditor.FileEditorManagerEvent
import com.intellij.openapi.fileEditor.ex.FileEditorWithProvider
import com.intellij.openapi.project.Project
import com.intellij.openapi.util.Disposer
import com.jetbrains.rd.util.lifetime.Lifetime
import com.jetbrains.rdserver.core.RemoteClientSessionListener
import java.util.concurrent.ConcurrentHashMap
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.filter
import kotlinx.coroutines.launch

/**
 * Restores the classic editor-selection edge when a Remote Dev frontend module is unavailable.
 *
 * Marketplace plugin synchronization normally installs the modular frontend, but a local ZIP or
 * custom-repository install can leave only the backend module present. Guest selection lives in
 * [ClientFileEditorManager] and does not publish the project [FileEditorManager] topic in that
 * topology. The frontend snapshot remains authoritative whenever it has delivered an accepted
 * generation; this fallback observes the guest flow only while that authority is absent.
 */
internal object BackendGuestSelectionFallback {
    private val log = Logger.getInstance(BackendGuestSelectionFallback::class.java)

    fun install(project: Project, lifecycle: Disposable, listener: EditorTabSyncListener) {
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
        val jobs = ConcurrentHashMap<String, Job>()
        val ingress = AuthenticatedSurfaceIngressService.getInstance(project)
        Disposer.register(lifecycle) {
            jobs.clear()
            scope.cancel()
        }
        scope.launch {
            ingress.frontendSnapshotAvailable
                .filter { it }
                .collect {
                    log.info("[layout-sync] frontend surface authority accepted; retiring backend fallback")
                    listener.retireControllerProjection()
                }
        }

        fun observe(session: ClientProjectSession) {
            if (session.project !== project || !session.isGuest) return
            val sessionId = session.clientId.toString()
            jobs.computeIfAbsent(sessionId) {
                scope.launch {
                    val manager = try {
                        session.getService(ClientFileEditorManager::class.java)
                    } catch (error: Throwable) {
                        log.warn("[layout-sync] guest editor manager unavailable client=$sessionId", error)
                        return@launch
                    }
                    val gate = BackendGuestSelectionGate<FileEditorWithProvider>()
                    manager.getSelectedEditorWithProviderFlow()
                        .combine(ingress.frontendSnapshotAvailable) { selection, frontendAvailable ->
                            selection to frontendAvailable
                        }
                        .collect { (selection, frontendAvailable) ->
                            val edge = gate.observe(selection, frontendAvailable) ?: return@collect
                            ApplicationManager.getApplication().invokeLater {
                                // One-shot effect-time fence: an accepted frontend snapshot may
                                // arrive after the flow edge but before this EDT delivery.
                                if (project.isDisposed || ingress.frontendSnapshotAvailable.value) {
                                    return@invokeLater
                                }
                                log.info(
                                    "[layout-sync] backend guest selection fallback client=$sessionId " +
                                        "file=${edge.second.fileEditor.file?.name ?: "<none>"}",
                                )
                                listener.selectionChanged(
                                    FileEditorManagerEvent(
                                        FileEditorManager.getInstance(project),
                                        edge.first,
                                        edge.second,
                                    ),
                                )
                            }
                        }
                }
            }
        }

        ClientSessionsManager.getProjectSessions(project, ClientKind.GUEST).forEach(::observe)
        ApplicationManager.getApplication()
            .messageBus
            .connect(lifecycle)
            .subscribe(
                RemoteClientSessionListener.TOPIC,
                object : RemoteClientSessionListener {
                    override fun projectSessionInitialized(lifetime: Lifetime, session: ClientProjectSession) {
                        observe(session)
                    }

                    override fun beforeProjectSessionDestroyed(session: ClientProjectSession) {
                        if (session.project === project) jobs.remove(session.clientId.toString())?.cancel()
                    }
                },
            )
    }
}

/** Pure authority gate kept separate so frontend/fallback handoff is regression-testable. */
internal class BackendGuestSelectionGate<T : Any> {
    private var lastForwarded: T? = null

    fun observe(selection: T?, frontendAvailable: Boolean): Pair<T?, T>? {
        if (frontendAvailable) {
            lastForwarded = null
            return null
        }
        if (selection == null) {
            lastForwarded = null
            return null
        }
        if (selection == lastForwarded) return null
        return (lastForwarded to selection).also { lastForwarded = selection }
    }
}
