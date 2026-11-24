@file:Suppress("UnstableApiUsage")

package com.github.btakita.agentdoc.split.backend

import com.github.btakita.agentdoc.shell.ShellGenerationHost
import com.github.btakita.agentdoc.split.FrontendSurfaceSnapshot
import com.github.btakita.agentdoc.split.FrontendImplementationRelease
import com.github.btakita.agentdoc.split.FrontendImplementationReleaseIdentity
import com.github.btakita.agentdoc.split.FrontendPresentationReceipt
import com.github.btakita.agentdoc.split.SURFACE_SNAPSHOT_SCHEMA_VERSION
import com.github.btakita.agentdoc.split.SurfaceIngressAck
import com.github.btakita.agentdoc.split.SurfaceIngressLease
import com.github.btakita.agentdoc.split.SurfaceIngressStatus
import com.github.btakita.agentdoc.split.SurfaceSnapshotRpcApi
import com.github.btakita.agentdoc.split.identity
import com.intellij.codeWithMe.ClientId
import com.intellij.openapi.diagnostic.Logger
import com.intellij.platform.project.ProjectId
import com.intellij.platform.project.findProjectOrNull
import kotlinx.coroutines.awaitCancellation
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.flow

internal class BackendSurfaceSnapshotRpcApi(
    private val frontendRelease: () -> FrontendImplementationRelease? = {
        ShellGenerationHost.getInstance().activate()
            ?.moduleService(SurfaceIngressFactory::class.java)
            ?.currentFrontendRelease()
    },
    /** GH #440: records which release RPC each Client uses, so the ingress can name an old Client. */
    private val releasePolls: FrontendReleasePollLedger = FrontendReleasePollLedger.shared,
    private val callerClientId: () -> String = { ClientId.current.value },
) : SurfaceSnapshotRpcApi {
    private val log = Logger.getInstance(BackendSurfaceSnapshotRpcApi::class.java)

    override suspend fun openIngress(
        projectId: ProjectId,
        frontendInstanceId: String,
    ): Flow<SurfaceIngressLease> {
        val authenticatedClientId = ClientId.current.value
        return flow {
            val project = projectId.findProjectOrNull()
            if (project == null) {
                log.info(
                    "[split-surface] frontend/backend ingress deferred: backend project is not " +
                        "resolvable yet client=$authenticatedClientId retry_required=true",
                )
                return@flow
            }
            val ingress = AuthenticatedSurfaceIngressService.getInstance(project)
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
        val project = snapshot.projectId.findProjectOrNull()
            ?: return SurfaceIngressAck(
                status = SurfaceIngressStatus.PROJECT_MISMATCH,
                connectionGeneration = lease.connectionGeneration,
                acceptedSequence = null,
                diagnostic = "project is not available on the backend",
            )
        return AuthenticatedSurfaceIngressService.getInstance(project).publish(
            authenticatedClientId = ClientId.current.value,
            lease = lease,
            snapshot = snapshot,
        )
    }

    override suspend fun publishPresentationReceipt(
        lease: SurfaceIngressLease,
        receipt: FrontendPresentationReceipt,
    ): SurfaceIngressAck {
        val project = receipt.projectId.findProjectOrNull()
            ?: return SurfaceIngressAck(
                status = SurfaceIngressStatus.PROJECT_MISMATCH,
                connectionGeneration = lease.connectionGeneration,
                acceptedSequence = null,
                diagnostic = "no backend project owns the authenticated presentation lease",
            )
        return AuthenticatedSurfaceIngressService.getInstance(project).publishPresentationReceipt(
            authenticatedClientId = ClientId.current.value,
            lease = lease,
            receipt = receipt,
        )
    }

    override suspend fun supportedSnapshotSchemaVersion(): Int = SURFACE_SNAPSHOT_SCHEMA_VERSION

    /**
     * GH #440: a Client that fetches the whole package without asking for its identity first
     * predates 0.2.1052; the ledger lets the ingress say so instead of blaming the negotiation.
     */
    override suspend fun frontendImplementationRelease(): FrontendImplementationRelease? =
        frontendRelease()?.also { releasePolls.packageFetched(callerClientId()) }

    /**
     * GH #435: the same discovery (and backend peer activation) as [frontendImplementationRelease],
     * answered without the package bytes, which stay on this host.
     */
    override suspend fun frontendImplementationReleaseIdentity(): FrontendImplementationReleaseIdentity? {
        releasePolls.identityAsked(callerClientId())
        return frontendRelease()?.identity()
    }
}
