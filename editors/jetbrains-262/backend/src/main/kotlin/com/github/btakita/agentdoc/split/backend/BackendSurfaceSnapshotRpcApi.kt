@file:Suppress("UnstableApiUsage")

package com.github.btakita.agentdoc.split.backend

import com.github.btakita.agentdoc.shell.ShellGenerationHost
import com.github.btakita.agentdoc.shell.receipt.ActivationReceipt
import com.github.btakita.agentdoc.split.ACTIVATION_RECEIPT_SURFACE_SNAPSHOT_SCHEMA_VERSION
import com.github.btakita.agentdoc.split.FrontendSurfaceSnapshot
import com.github.btakita.agentdoc.split.FrontendImplementationRelease
import com.github.btakita.agentdoc.split.FrontendImplementationReleaseIdentity
import com.github.btakita.agentdoc.split.FrontendPresentationReceipt
import com.github.btakita.agentdoc.split.SURFACE_SNAPSHOT_SCHEMA_VERSION
import com.github.btakita.agentdoc.split.SurfaceIngressAck
import com.github.btakita.agentdoc.split.SurfaceIngressLease
import com.github.btakita.agentdoc.split.SurfaceIngressStatus
import com.github.btakita.agentdoc.split.SurfaceSnapshotRpcApi
import com.github.btakita.agentdoc.split.generation
import com.intellij.codeWithMe.ClientId
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.project.Project
import com.intellij.platform.project.ProjectId
import com.intellij.platform.project.findProjectOrNull
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.awaitCancellation
import kotlinx.coroutines.withContext
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.flow

/** The active implementation generation's ingress factory, or null before the first project. */
internal fun activeSurfaceIngressFactory(): SurfaceIngressFactory? =
    ShellGenerationHost.getInstance().activate()?.moduleService(SurfaceIngressFactory::class.java)

/**
 * The frozen [SurfaceSnapshotRpcApi] (GH #435): every shipped JetBrains Client calls exactly these
 * methods, so this class keeps answering all of them for older Clients. New RPCs go to
 * [BackendFrontendReleaseRpcApi].
 */
internal class BackendSurfaceSnapshotRpcApi(
    private val frontendRelease: () -> FrontendImplementationRelease? = {
        activeSurfaceIngressFactory()?.currentFrontendRelease()
    },
    private val gate: FrontendReleaseDeliveryGate = FrontendReleaseDeliveryGate.shared,
    /** GH #440: records which release RPC each Client uses, so the ingress can name an old Client. */
    private val releasePolls: FrontendReleasePollLedger = FrontendReleasePollLedger.shared,
    /** GH #435: whether a signed release is selected, from its manifest alone (no bytes, no activation). */
    private val releaseSelected: () -> Boolean = {
        runCatching { activeSurfaceIngressFactory()?.selectedFrontendReleasePackageUrl() != null }.getOrDefault(false)
    },
    private val callerClientId: () -> String = { ClientId.current.value },
    private val resolveProject: (ProjectId) -> Project? = { it.findProjectOrNull() },
) : SurfaceSnapshotRpcApi {
    private val log = Logger.getInstance(BackendSurfaceSnapshotRpcApi::class.java)

    override suspend fun openIngress(
        projectId: ProjectId,
        frontendInstanceId: String,
    ): Flow<SurfaceIngressLease> {
        val authenticatedClientId = callerClientId()
        return flow {
            val project = resolveProject(projectId)
            if (project == null) {
                log.info(
                    "[split-surface] frontend/backend ingress deferred: backend project is not " +
                        "resolvable yet client=$authenticatedClientId retry_required=true",
                )
                return@flow
            }
            val ingress = AuthenticatedSurfaceIngressService.getInstance(project)
            gate.leaseOpened(authenticatedClientId)
            val lease = ingress.open(authenticatedClientId, frontendInstanceId)
            log.info(
                "[split-surface] frontend/backend ingress accepted client=$authenticatedClientId " +
                    "generation=${lease.connectionGeneration}",
            )
            try {
                emit(lease)
                awaitCancellation()
            } finally {
                ingress.retire(authenticatedClientId, lease)
            }
        }
    }

    override suspend fun publishSnapshot(
        lease: SurfaceIngressLease,
        snapshot: FrontendSurfaceSnapshot,
    ): SurfaceIngressAck {
        val authenticatedClientId = callerClientId()
        observeFrontendShell(authenticatedClientId, snapshot)
        val project = resolveProject(snapshot.projectId)
            ?: return SurfaceIngressAck(
                status = SurfaceIngressStatus.PROJECT_MISMATCH,
                connectionGeneration = lease.connectionGeneration,
                acceptedSequence = null,
                diagnostic = "project is not available on the backend",
            )
        return AuthenticatedSurfaceIngressService.getInstance(project).publish(
            authenticatedClientId = authenticatedClientId,
            lease = lease,
            snapshot = snapshot,
        )
    }

    /**
     * GH #435: a schema-5 snapshot proves the Client's frontend shell. Once the Client negotiated
     * and runs this backend's shell, the backend peer activates before the ingress fence reads this
     * snapshot, so the Client's not-yet-activated snapshots are refused before any is accepted,
     * never revoked after. A Client on another shell never activates it, so this backend stays
     * shell-only for that Client, which admits its shell-only snapshots.
     */
    private suspend fun observeFrontendShell(clientId: String, snapshot: FrontendSurfaceSnapshot) {
        if (snapshot.schemaVersion >= ACTIVATION_RECEIPT_SURFACE_SNAPSHOT_SCHEMA_VERSION) {
            ActivationReceipt.decodeOrNull(snapshot.activationReceipt)?.shellDigest?.let { shell ->
                if (gate.observeShell(clientId, shell) == FrontendReleaseDeliveryGate.ShellObservation.FIRST_MISMATCH) {
                    // `agent-doc upgrade` reads this line (src/plugin.rs): keep its `key=value` facts.
                    val backend = BackendPluginBuild.current()
                    log.warn(
                        "[frontend-impl] client shell mismatch client=$clientId client_shell=${shell.take(12)} " +
                            "backend_plugin=${backend.version} backend_shell=${backend.shellDigest.take(12)} " +
                            "backend_plugin_path=${backend.pluginPath}",
                    )
                }
            }
        }
        if (gate.claimPeerActivation(clientId)) {
            runCatching { withContext(Dispatchers.IO) { frontendRelease() } }
                .onSuccess { gate.rememberPackageUrl(it?.packageUrl) }
                .onFailure { log.warn("[frontend-impl] backend peer activation failed client=$clientId", it) }
        }
    }

    override suspend fun publishPresentationReceipt(
        lease: SurfaceIngressLease,
        receipt: FrontendPresentationReceipt,
    ): SurfaceIngressAck {
        val project = resolveProject(receipt.projectId)
            ?: return SurfaceIngressAck(
                status = SurfaceIngressStatus.PROJECT_MISMATCH,
                connectionGeneration = lease.connectionGeneration,
                acceptedSequence = null,
                diagnostic = "no backend project owns the authenticated presentation lease",
            )
        return AuthenticatedSurfaceIngressService.getInstance(project).publishPresentationReceipt(
            authenticatedClientId = callerClientId(),
            lease = lease,
            receipt = receipt,
        )
    }

    override suspend fun supportedSnapshotSchemaVersion(): Int {
        gate.negotiated(callerClientId())
        return SURFACE_SNAPSHOT_SCHEMA_VERSION
    }

    /**
     * GH #435: the whole package, only for a Client [FrontendReleaseDeliveryGate] admits. Every
     * other answer is null, which every shipped Client treats as "nothing to activate": before
     * this, a Client up to 0.2.1052 received the multi-megabyte ZIP on every 30 s poll, racing its
     * schema negotiation, and a 0.2.1046 Client on an older shell refused it 33 times in a row.
     */
    override suspend fun frontendImplementationRelease(): FrontendImplementationRelease? {
        val clientId = callerClientId()
        val admission = gate.admission(clientId)
        if (admission != FrontendReleaseDeliveryGate.Admission.DELIVER) {
            // GH #440: a whole-package poll the gate withholds from a Client that never asked for
            // the identity is still the legacy-poll proof the ingress names an old Client by, as
            // long as there is a package to withhold.
            if (withContext(Dispatchers.IO) { releaseSelected() }) releasePolls.packageFetched(clientId)
            // GH #440 needs why the package was kept from this Client (it no longer refuses it).
            if (gate.withheld(clientId, admission)) {
                log.info(
                    "[frontend-impl] release withheld client=$clientId reason=${admission.name.lowercase()} " +
                        "client_shell=${gate.clientShell(clientId)?.take(12) ?: "unproven"} " +
                        "backend_shell=${gate.backendShellDigest().take(12)} " +
                        "legacy_poll=${releasePolls.evidence(clientId)?.legacyReleasePoller ?: false}",
                )
            }
            return null
        }
        // Release discovery reads and verifies the package on disk; keep it off the RPC dispatcher.
        val release = withContext(Dispatchers.IO) { frontendRelease() } ?: return null
        releasePolls.packageFetched(clientId)
        gate.claimPeerActivation(clientId)
        gate.rememberPackageUrl(release.packageUrl)
        return release.takeIf { gate.claimDelivery(clientId, it.generation()) }
    }

    /**
     * GH #435: the shipped 0.2.1052 identity poll (0.2.1052 through 0.2.1054 Clients). Only a Client
     * the gate admits gets an answer, so this never triggers release discovery or peer activation
     * before that Client's schema answer, and a Client on another shell is told nothing to fetch.
     */
    override suspend fun frontendImplementationReleaseIdentity(): FrontendImplementationReleaseIdentity? {
        val clientId = callerClientId()
        releasePolls.identityAsked(clientId)
        if (gate.admission(clientId) != FrontendReleaseDeliveryGate.Admission.DELIVER) return null
        val release = withContext(Dispatchers.IO) { frontendRelease() } ?: return null
        gate.claimPeerActivation(clientId)
        gate.rememberPackageUrl(release.packageUrl)
        return FrontendImplementationReleaseIdentity(implSha256 = release.implSha256, epoch = release.epoch)
    }
}
