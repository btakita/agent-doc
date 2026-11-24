package com.github.btakita.agentdoc.split.backend

import com.github.btakita.agentdoc.shell.receipt.ShellIdentity
import com.github.btakita.agentdoc.split.FrontendClientBuild
import com.github.btakita.agentdoc.split.FrontendReleaseGeneration
import java.util.concurrent.ConcurrentHashMap

/**
 * GH #435 shell: which JetBrains Client may receive the backend-selected frontend release, and when.
 *
 * A Client's release poll starts with the Client, before its ingress lease, and every shipped
 * Client up to 0.2.1052 asks [com.github.btakita.agentdoc.split.SurfaceSnapshotRpcApi.frontendImplementationRelease]
 * for the whole multi-megabyte ZIP every 30 s (0.2.1052 through 0.2.1054 ask its identity first). On Coder (2026-11-24) that transfer was in flight
 * while the Client's 5 s schema negotiation waited on the same Remote Dev link, the negotiation went
 * unanswered, and the lease published schema 1 for its lifetime. A 0.2.1046 Client also refused the
 * 0.2.1052 release (its shell differs) and downloaded it again every 30 s, 33 times.
 *
 * The gate therefore moves the package bytes to one Client only after
 *  1. the backend answered that Client's schema negotiation, so the bytes never race it;
 *  2. that Client proved its frontend shell (the schema-5 activation receipt, or the build it named
 *     in [com.github.btakita.agentdoc.split.FrontendReleaseRpcApi.frontendReleaseOffer]) and it is
 *     this backend's shell, the only shell that can load this backend's release;
 *  3. it has not already received the same generation within [refetchAfterMs].
 *
 * The per-Client evidence the #440 diagnostic reads stays in the shell's FrontendReleasePollLedger,
 * which the RPC layer feeds alongside this gate.
 *
 * It also decides when the backend peer activates ([claimPeerActivation]): only for a Client the
 * bytes may go to, i.e. one that negotiated and runs this backend's shell, and before that Client's
 * next snapshot reaches the ingress fence, so its not-yet-activated snapshots are refused before any
 * is accepted rather than revoked after one was. Nothing heavy runs for a Client before its
 * schema answer, so negotiation never waits behind release discovery or peer activation. A backend that activated for a Client on an older shell would hold that Client's
 * layout behind the activation fence for good; a backend no Client can follow stays shell-only,
 * which admits legacy and shell-only snapshots (editors/jetbrains/SPEC.md "Ingress fence").
 */
internal class FrontendReleaseDeliveryGate(
    private val backendShell: () -> String = { ShellIdentity.shellDigest },
    private val clock: () -> Long = System::currentTimeMillis,
    private val refetchAfterMs: Long = RELEASE_REFETCH_AFTER_MS,
) {
    enum class Admission {
        /** The bytes may cross the link now. */
        DELIVER,
        NOT_NEGOTIATED,
        SHELL_UNKNOWN,
        SHELL_MISMATCH,
    }

    /** What one shell observation means for the backend peer activation. */
    enum class ShellObservation {
        /** The first proof this Client runs the backend's shell. */
        FIRST_MATCH,
        /** The first proof this Client runs another shell: report it and never offer the bytes. */
        FIRST_MISMATCH,
        UNCHANGED,
    }

    private data class ClientState(
        val negotiated: Boolean = false,
        val shellDigest: String? = null,
        val build: FrontendClientBuild? = null,
        val servedKey: String? = null,
        val servedAtMs: Long = 0,
        val lastWithheld: Admission? = null,
        val peerActivationClaimed: Boolean = false,
        val touchedAtMs: Long = 0,
    )

    private val clients = ConcurrentHashMap<String, ClientState>()

    private fun update(clientId: String, change: (ClientState?) -> ClientState) {
        clients.compute(clientId) { _, state -> change(state).copy(touchedAtMs = clock()) }
    }

    /** The newest signed package URL the backend's installer recorded, for remedy text. */
    @Volatile
    var lastPackageUrl: String? = null
        private set

    fun rememberPackageUrl(url: String?) {
        if (!url.isNullOrBlank()) lastPackageUrl = url
    }

    /** The backend answered [clientId]'s snapshot schema negotiation. */
    fun negotiated(clientId: String) {
        update(clientId) { state -> (state ?: ClientState()).copy(negotiated = true) }
    }

    /** [clientId]'s frontend reported [shellDigest] (schema-5 receipt or its offer request). */
    fun observeShell(clientId: String, shellDigest: String): ShellObservation {
        var observation = ShellObservation.UNCHANGED
        update(clientId) { state ->
            val current = state ?: ClientState()
            if (current.shellDigest != shellDigest) {
                observation = if (shellDigest == backendShell()) {
                    ShellObservation.FIRST_MATCH
                } else {
                    ShellObservation.FIRST_MISMATCH
                }
            }
            current.copy(shellDigest = shellDigest)
        }
        return observation
    }

    /** [clientId] named its own build in an offer request (Clients after 0.2.1054). */
    fun observeBuild(clientId: String, build: FrontendClientBuild): ShellObservation {
        update(clientId) { state -> (state ?: ClientState()).copy(build = build) }
        return observeShell(clientId, build.shellDigest)
    }

    fun clientBuild(clientId: String): FrontendClientBuild? = clients[clientId]?.build

    fun clientShell(clientId: String): String? = clients[clientId]?.shellDigest

    fun backendShellDigest(): String = backendShell()

    /** Whether [clientId] may receive the package bytes now. */
    fun admission(clientId: String): Admission {
        val state = clients[clientId] ?: return Admission.NOT_NEGOTIATED
        return when {
            !state.negotiated -> Admission.NOT_NEGOTIATED
            state.shellDigest == null -> Admission.SHELL_UNKNOWN
            state.shellDigest != backendShell() -> Admission.SHELL_MISMATCH
            else -> Admission.DELIVER
        }
    }

    /**
     * True once per lease, when [clientId] first may receive the bytes: the caller activates the
     * backend peer now (idempotent across Clients on one generation).
     */
    fun claimPeerActivation(clientId: String): Boolean {
        if (admission(clientId) != Admission.DELIVER) return false
        var claimed = false
        update(clientId) { state ->
            val current = state ?: ClientState()
            claimed = !current.peerActivationClaimed
            current.copy(peerActivationClaimed = true)
        }
        return claimed
    }

    /**
     * Record a withheld delivery; true the first time [clientId] is withheld for [admission] in a
     * row, so the backend logs each reason once instead of every 30 s.
     */
    fun withheld(clientId: String, admission: Admission): Boolean {
        var changed = false
        update(clientId) { state ->
            val current = state ?: ClientState()
            changed = current.lastWithheld != admission
            current.copy(lastWithheld = admission)
        }
        return changed
    }

    /**
     * Claim the delivery of [identity] to [clientId]: false when the same generation already went
     * to that Client within [refetchAfterMs] (a Client up to 0.2.1054 that refused it, or that
     * activated it and still asks, then gets null and does nothing).
     */
    fun claimDelivery(clientId: String, identity: FrontendReleaseGeneration): Boolean {
        val key = "${identity.implSha256}@${identity.epoch}"
        val now = clock()
        var claimed = false
        update(clientId) { state ->
            val current = state ?: ClientState()
            if (current.servedKey == key && now - current.servedAtMs < refetchAfterMs) {
                current
            } else {
                claimed = true
                current.copy(servedKey = key, servedAtMs = now, lastWithheld = null)
            }
        }
        return claimed
    }

    /**
     * [clientId] opened a new ingress lease: the lease negotiates its schema again, so the bytes
     * wait for that answer. The proven shell stays (a new shell needs a Client restart).
     */
    fun leaseOpened(clientId: String) {
        update(clientId) { state -> (state ?: ClientState()).copy(negotiated = false, peerActivationClaimed = false) }
        evictBeyond(MAX_TRACKED_CLIENTS)
    }

    private fun evictBeyond(limit: Int) {
        if (clients.size <= limit) return
        clients.entries.sortedBy { it.value.touchedAtMs }.take(clients.size - limit).forEach { (id, state) ->
            clients.remove(id, state)
        }
    }

    companion object {
        /** A refused generation is offered again after this, so trust added later still activates. */
        const val RELEASE_REFETCH_AFTER_MS: Long = 10L * 60 * 1000

        /** Client ids are per Remote Dev connection; keep the newest ones only. */
        const val MAX_TRACKED_CLIENTS: Int = 64

        /** The one gate of this backend JVM; both Fleet RPC providers and the ingress read it. */
        val shared: FrontendReleaseDeliveryGate = FrontendReleaseDeliveryGate()
    }
}
