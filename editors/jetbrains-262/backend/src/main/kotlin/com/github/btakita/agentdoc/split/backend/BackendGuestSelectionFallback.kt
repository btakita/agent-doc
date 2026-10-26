@file:Suppress("UnstableApiUsage")

package com.github.btakita.agentdoc.split.backend

import com.intellij.openapi.Disposable
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.project.Project
import com.github.btakita.agentdoc.EditorTabSyncListener

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
    type: String,
    readinessReason: String = FRONTEND_AUTHORITY_UNAVAILABLE_READINESS,
): String =
    "[layout-sync] frontend layout publisher unavailable client=$clientId type=$type " +
        "readiness=blocked readiness_reason=$readinessReason " +
        "geometry_reason=flat_evidence_has_no_column_geometry " +
        "structural_routing=blocked fallback=unavailable remedy=$FRONTEND_AUTHORITY_UNAVAILABLE_REMEDY"

/**
 * Records that the former backend-only selection fallback is unavailable.
 *
 * Marketplace plugin synchronization normally installs the modular frontend, but a local ZIP or
 * custom-repository install can leave only the backend module present. JetBrains exposes its
 * client-scoped selection flow through private APIs, so Marketplace builds fail closed and require
 * the matching frontend module instead of linking that fallback.
 */
internal object BackendGuestSelectionFallback {
    private val log = Logger.getInstance(BackendGuestSelectionFallback::class.java)

    fun install(project: Project, lifecycle: Disposable, listener: EditorTabSyncListener) {
        log.info(
            "[layout-sync] backend selection fallback unavailable project=${project.name}; " +
                "the Marketplace-compatible split frontend is required",
        )
    }
}

/**
 * Pure string-valued session-kind gate used after the optional runtime capability lookup.
 *
 * A Remote Dev client is `CONTROLLER`, not `GUEST`, so the gate admits both remote values and
 * excludes local and frontend-process sessions.
 */
internal object BackendRemoteSessionGate {
    fun observes(type: String): Boolean = type == "CONTROLLER" || type == "GUEST"

    /** A lease owns measured layout only when its authenticated id names a remote session. */
    fun ownsLayout(clientId: String, sessions: List<Pair<String, String>>): Boolean =
        sessions.any { (id, type) -> id == clientId && observes(type) }

    fun selectionProvesFocus(type: String): Boolean = type == "CONTROLLER"

    fun describe(sessions: List<Pair<String, String>>): String =
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
