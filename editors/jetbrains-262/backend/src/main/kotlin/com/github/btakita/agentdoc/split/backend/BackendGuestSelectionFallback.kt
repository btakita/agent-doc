@file:Suppress("UnstableApiUsage")

package com.github.btakita.agentdoc.split.backend

import com.github.btakita.agentdoc.EditorTabSyncListener
import com.intellij.openapi.Disposable
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.client.ClientKind
import com.intellij.openapi.client.ClientProjectSession
import com.intellij.openapi.client.ClientSessionsManager
import com.intellij.openapi.client.ClientType
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
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.filter
import kotlinx.coroutines.launch

internal const val FRONTEND_AUTHORITY_DIAGNOSTIC_DELAY_MS = 5_000L
internal const val FRONTEND_AUTHORITY_UNAVAILABLE_READINESS =
    "frontend_authority_absent_or_generation_mismatched"
internal const val FRONTEND_AUTHORITY_CONNECTED_WITHOUT_PUBLICATION =
    "frontend_connected_without_accepted_publication"
internal const val FRONTEND_AUTHORITY_UNAVAILABLE_REMEDY =
    "install the same Agent Doc modular plugin ZIP in JetBrains Client/Gateway and the Remote Dev backend, " +
        "verify both sides report that exact plugin version, restart both (including the JetBrains Client/Gateway " +
        "process; restarting only the Remote Dev backend cannot replace its already-loaded frontend module), " +
        "then retry Run Agent Doc"

internal fun frontendAuthorityUnavailableDiagnostic(
    clientId: String,
    type: ClientType,
    readinessReason: String = FRONTEND_AUTHORITY_UNAVAILABLE_READINESS,
): String =
    "[layout-sync] frontend layout publisher unavailable client=$clientId type=$type " +
        "readiness=blocked readiness_reason=$readinessReason " +
        "geometry_reason=flat_evidence_has_no_column_geometry " +
        "structural_routing=blocked fallback=focus_only remedy=$FRONTEND_AUTHORITY_UNAVAILABLE_REMEDY"

/**
 * Restores the classic editor-selection edge when a Remote Dev frontend module is unavailable.
 *
 * Marketplace plugin synchronization normally installs the modular frontend, but a local ZIP or
 * custom-repository install can leave only the backend module present. Remote client selection
 * lives in [ClientFileEditorManager] (the platform's `GuestFileEditorManager`) and does not publish
 * the project [FileEditorManager] topic in that topology. An authenticated frontend lease is
 * authoritative before its first snapshot; this fallback observes the remote selection flow only
 * while no frontend lease owns the project.
 *
 * "Remote" means every session [BackendRemoteSessionGate] admits: a JetBrains Remote Dev client
 * (`ClientType.CONTROLLER`) as well as a Code With Me guest (`ClientType.GUEST`). Observing only
 * guests left Remote Dev with no selection edge at all (GH #262).
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
            ingress.layoutAuthorityActive
                .filter { it }
                .collect {
                    log.info(
                        "[layout-sync] frontend surface authority connected; " +
                            "retiring backend structural fallback pending accepted publication",
                    )
                    listener.retireControllerProjection()
                }
        }

        fun observe(session: ClientProjectSession): Boolean {
            if (session.project !== project || !BackendRemoteSessionGate.observes(session.type)) return false
            val sessionId = session.clientId.toString()
            jobs.computeIfAbsent(sessionId) {
                log.info("[layout-sync] backend selection fallback observing client=$sessionId type=${session.type}")
                scope.launch {
                    // A healthy split frontend establishes authority before its first snapshot.
                    // After the schema-negotiation window, a still-inactive authority means this
                    // process has only the backend fallback. Name the effective two-sided install
                    // remedy instead of suggesting another backend-only IDE restart (GH #352).
                    launch {
                        delay(FRONTEND_AUTHORITY_DIAGNOSTIC_DELAY_MS)
                        when (ingress.layoutPublicationReadiness.value) {
                            LayoutPublicationReadiness.ACCEPTED -> Unit
                            LayoutPublicationReadiness.CONNECTED_BLOCKED -> log.warn(
                                frontendAuthorityUnavailableDiagnostic(
                                    sessionId,
                                    session.type,
                                    FRONTEND_AUTHORITY_CONNECTED_WITHOUT_PUBLICATION,
                                ),
                            )
                            LayoutPublicationReadiness.INACTIVE -> log.warn(
                                frontendAuthorityUnavailableDiagnostic(sessionId, session.type),
                            )
                        }
                    }
                    val manager = try {
                        session.getService(ClientFileEditorManager::class.java)
                    } catch (error: Throwable) {
                        log.warn(
                            "[layout-sync] guest editor manager unavailable client=$sessionId type=${session.type}",
                            error,
                        )
                        return@launch
                    }
                    val gate = BackendGuestSelectionGate<FileEditorWithProvider>()
                    manager.getSelectedEditorWithProviderFlow()
                        .combine(ingress.layoutAuthorityActive) { selection, frontendActive ->
                            selection to frontendActive
                        }
                        .collect { (selection, frontendActive) ->
                            val edge = gate.observe(selection, frontendActive) ?: return@collect
                            ApplicationManager.getApplication().invokeLater {
                                // One-shot effect-time fence: an accepted frontend snapshot may
                                // arrive after the flow edge but before this EDT delivery.
                                if (project.isDisposed || ingress.layoutAuthorityActive.value) {
                                    return@invokeLater
                                }
                                log.info(
                                    "[layout-sync] backend guest selection fallback client=$sessionId " +
                                        "type=${session.type} " +
                                        "file=${edge.second.fileEditor.file?.name ?: "<none>"}",
                                )
                                // The fallback owns the selection edge again: frames the earlier
                                // frontend handoff retired are live, so a retired-endpoint receipt
                                // must re-register instead of muting auto-sync at this process's
                                // generation (`#surfaceretireformal`, model action `Fallback`).
                                listener.resumeControllerProjection()
                                listener.selectionChanged(
                                    FileEditorManagerEvent(
                                        FileEditorManager.getInstance(project),
                                        edge.first,
                                        edge.second,
                                    ),
                                    remoteControllerSelectionFocusEvidence =
                                        BackendRemoteSessionGate.selectionProvesFocus(session.type),
                                )
                            }
                        }
                }
            }
            return true
        }

        // Subscribe before enumerating so a session initialized in between is not lost; the
        // per-client job map makes the overlap idempotent.
        ApplicationManager.getApplication()
            .messageBus
            .connect(lifecycle)
            .subscribe(
                RemoteClientSessionListener.TOPIC,
                object : RemoteClientSessionListener {
                    override fun projectSessionInitialized(lifetime: Lifetime, session: ClientProjectSession) {
                        if (session.project === project && !observe(session)) {
                            log.info(
                                "[layout-sync] backend selection fallback skipped client=${session.clientId} " +
                                    "type=${session.type}",
                            )
                        }
                    }

                    override fun beforeProjectSessionDestroyed(session: ClientProjectSession) {
                        if (session.project === project) jobs.remove(session.clientId.toString())?.cancel()
                    }
                },
            )
        // Enumerate every session, not only the observed kind, so the install line distinguishes
        // "installed but filtered everything out" from "never installed" (GH #262).
        val existing = ClientSessionsManager.getProjectSessions(project, ClientKind.ALL)
        val observed = existing.filter(::observe)
        log.info(
            "[layout-sync] backend selection fallback installed project=${project.name} " +
                "observing=${BackendRemoteSessionGate.describe(observed.map { it.clientId.toString() to it.type })} " +
                "skipped=${BackendRemoteSessionGate.describe((existing - observed.toSet()).map { it.clientId.toString() to it.type })}",
        )
    }
}

/**
 * Pure session-kind gate for [BackendGuestSelectionFallback] (GH #262).
 *
 * A Remote Dev client session is `ClientType.CONTROLLER`, not `GUEST`, so the gate admits every
 * remote type (`ClientType.isRemote` = controller or guest) and excludes the local session, whose
 * selection already reaches the project [FileEditorManager] topic, and the frontend-process type.
 */
internal object BackendRemoteSessionGate {
    fun observes(type: ClientType): Boolean = type.isRemote

    /** A lease owns measured layout only when its authenticated id names a remote session. */
    fun ownsLayout(clientId: String, sessions: List<Pair<String, ClientType>>): Boolean =
        sessions.any { (id, type) -> id == clientId && observes(type) }

    fun selectionProvesFocus(type: ClientType): Boolean = type == ClientType.CONTROLLER

    fun describe(sessions: List<Pair<String, ClientType>>): String =
        if (sessions.isEmpty()) "none" else sessions.joinToString(",", "[", "]") { (id, type) -> "$id:$type" }
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
