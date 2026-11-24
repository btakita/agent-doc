@file:Suppress("UnstableApiUsage")

package com.github.btakita.agentdoc.split.backend

import com.github.btakita.agentdoc.split.FrontendImplementationRelease
import com.github.btakita.agentdoc.split.FrontendPresentationReceipt
import com.github.btakita.agentdoc.split.FrontendSurfaceSnapshot
import com.github.btakita.agentdoc.split.SurfaceIngressAck
import com.github.btakita.agentdoc.split.SurfaceIngressLease
import com.intellij.openapi.Disposable
import com.intellij.openapi.project.Project
import java.util.concurrent.ConcurrentHashMap
import kotlinx.coroutines.flow.StateFlow

/*
 * GH #339 R1: the shell SPI of the Fleet RPC surface ingress (editors/jetbrains/SPEC.md "R1
 * status", modular extensions and Fleet RPC). The RPC provider and its project service are
 * platform-held shell objects; they reach the implementation only through these types, which the
 * active implementation generation serves (`ImplGeneration.moduleService`).
 */

/** Creates one project's [SurfaceIngress]; served by the implementation generation. */
internal interface SurfaceIngressFactory {
    fun create(project: Project): SurfaceIngress

    /** Discovers and activates the backend peer of the selected frontend package. */
    fun currentFrontendRelease(): FrontendImplementationRelease?
}

/** The authenticated frontend lease, snapshot and presentation-receipt ingress of one project. */
internal interface SurfaceIngress : Disposable {
    /** True from authenticated lease open through explicit retirement, including pre-snapshot. */
    val frontendAuthorityActive: StateFlow<Boolean>

    /** True only when an authenticated *remote* lease owns measured pane-layout authority. */
    val layoutAuthorityActive: StateFlow<Boolean>

    /** Readiness stays blocked until every connected remote layout lease publishes successfully. */
    val layoutPublicationReadiness: StateFlow<LayoutPublicationReadiness>

    suspend fun open(authenticatedClientId: String, frontendInstanceId: String): SurfaceIngressLease

    suspend fun publish(
        authenticatedClientId: String,
        lease: SurfaceIngressLease,
        snapshot: FrontendSurfaceSnapshot,
    ): SurfaceIngressAck

    suspend fun retire(authenticatedClientId: String, lease: SurfaceIngressLease)

    suspend fun publishPresentationReceipt(
        authenticatedClientId: String,
        lease: SurfaceIngressLease,
        receipt: FrontendPresentationReceipt,
    ): SurfaceIngressAck
}

internal enum class LayoutPublicationReadiness {
    INACTIVE,
    CONNECTED_BLOCKED,
    ACCEPTED,
}

/**
 * GH #440: what the shell's Fleet RPC layer learned about one JetBrains Client's Agent Doc
 * frontend outside its snapshots. A schema-1 snapshot carries no activation receipt, so without
 * this the backend cannot tell a Client that refuses its release (an older shell) from a Client
 * whose schema negotiation merely went unanswered.
 */
internal data class FrontendClientShellEvidence(
    /** The Client's own shell digest, when the Client reported its build. */
    val shellDigest: String? = null,
    /** The Client's Agent Doc plugin version, when the Client reported its build. */
    val pluginVersion: String? = null,
    /** The Client's loaded plugin folder, when the Client reported its build. */
    val pluginPath: String? = null,
    /**
     * Whole-package fetches of the backend-selected release by a Client that never asked for the
     * release identity first. Every Client from 0.2.1052 on asks for the identity before the
     * package; only an older Client polls the whole package, and every such Client runs an older
     * shell than a backend that serves the identity, so it refuses the release on every poll.
     */
    val legacyPackagePolls: Int = 0,
) {
    /** True when this evidence proves the Client predates the release-identity poll (0.2.1052). */
    val legacyReleasePoller: Boolean get() = legacyPackagePolls > 0 && shellDigest == null
}

/** GH #440: the shell-held [FrontendClientShellEvidence] of each authenticated Client id. */
internal fun interface FrontendClientShellEvidenceSource {
    fun evidence(clientId: String): FrontendClientShellEvidence?
}

/**
 * GH #440: the shell RPC layer's per-Client release-poll ledger. The Fleet RPC implementation
 * records each Client's identity and package requests (and, from a Client that reports it, its
 * build); the implementation generation reads [evidence] when it explains a refused snapshot.
 * It lives in the shell so the record survives an implementation swap.
 */
internal class FrontendReleasePollLedger(
    private val clock: () -> Long = System::currentTimeMillis,
    private val maxClients: Int = MAX_TRACKED_CLIENTS,
) : FrontendClientShellEvidenceSource {
    private data class Entry(
        val identityAsked: Boolean = false,
        val packageFetches: Int = 0,
        val build: FrontendClientShellEvidence? = null,
        val touchedAtMs: Long = 0,
    )

    private val clients = ConcurrentHashMap<String, Entry>()

    private fun update(clientId: String, change: (Entry) -> Entry) {
        clients.compute(clientId) { _, entry -> change(entry ?: Entry()).copy(touchedAtMs = clock()) }
        if (clients.size > maxClients) {
            clients.entries.sortedBy { it.value.touchedAtMs }.take(clients.size - maxClients)
                .forEach { (id, entry) -> clients.remove(id, entry) }
        }
    }

    /** [clientId] asked which generation the backend selected (Clients from 0.2.1052 on). */
    fun identityAsked(clientId: String) = update(clientId) { it.copy(identityAsked = true) }

    /** [clientId] received the whole release package. */
    fun packageFetched(clientId: String) = update(clientId) { it.copy(packageFetches = it.packageFetches + 1) }

    /** [clientId] named its own build (shell digest, plugin version and folder). */
    fun clientBuild(clientId: String, shellDigest: String, pluginVersion: String?, pluginPath: String?) =
        update(clientId) {
            it.copy(
                identityAsked = true,
                build = FrontendClientShellEvidence(shellDigest, pluginVersion, pluginPath),
            )
        }

    override fun evidence(clientId: String): FrontendClientShellEvidence? {
        val entry = clients[clientId] ?: return null
        entry.build?.let { return it }
        if (entry.identityAsked || entry.packageFetches == 0) return null
        return FrontendClientShellEvidence(legacyPackagePolls = entry.packageFetches)
    }

    companion object {
        /** Client ids are per Remote Dev connection; keep the newest ones only. */
        const val MAX_TRACKED_CLIENTS: Int = 64

        /** The one ledger of this backend JVM: the RPC records into it, the ingress reads it. */
        val shared: FrontendReleasePollLedger = FrontendReleasePollLedger()
    }
}
