@file:Suppress("UnstableApiUsage")

package com.github.btakita.agentdoc.split.backend

import com.github.btakita.agentdoc.shell.ShellGenerationHost
import com.github.btakita.agentdoc.split.FrontendPresentationReceipt
import com.github.btakita.agentdoc.split.FrontendSurfaceSnapshot
import com.github.btakita.agentdoc.split.SurfaceIngressAck
import com.github.btakita.agentdoc.split.SurfaceIngressLease
import com.intellij.openapi.Disposable
import com.intellij.openapi.components.Service
import com.intellij.openapi.components.service
import com.intellij.openapi.project.Project
import kotlinx.coroutines.flow.StateFlow

/**
 * GH #339 R1 shell: the project service the Fleet RPC provider (`BackendSurfaceSnapshotRpcApi`)
 * calls. It holds no ingress logic; it delegates to the [SurfaceIngress] the active implementation
 * generation creates for this project (`AuthenticatedSurfaceIngressState.kt`), so a change to the
 * lease state machine, the snapshot validator or the controller publisher is an implementation
 * change.
 */
@Service(Service.Level.PROJECT)
internal class AuthenticatedSurfaceIngressService(project: Project) : Disposable {
    private val ingress: SurfaceIngress =
        checkNotNull(
            ShellGenerationHost.getInstance().activate()
                ?.moduleService(SurfaceIngressFactory::class.java),
        ) { "no implementation generation serves the surface ingress; the plugin is unloading" }
            .create(project)

    override fun dispose() {
        ingress.dispose()
    }

    /** True from authenticated lease open through explicit retirement, including pre-snapshot. */
    val frontendAuthorityActive: StateFlow<Boolean> get() = ingress.frontendAuthorityActive

    /** True only when an authenticated *remote* lease owns measured pane-layout authority. */
    val layoutAuthorityActive: StateFlow<Boolean> get() = ingress.layoutAuthorityActive

    /** Readiness stays blocked until every connected remote layout lease publishes successfully. */
    val layoutPublicationReadiness: StateFlow<LayoutPublicationReadiness>
        get() = ingress.layoutPublicationReadiness

    suspend fun open(authenticatedClientId: String, frontendInstanceId: String): SurfaceIngressLease =
        ingress.open(authenticatedClientId, frontendInstanceId)

    suspend fun publish(
        authenticatedClientId: String,
        lease: SurfaceIngressLease,
        snapshot: FrontendSurfaceSnapshot,
    ): SurfaceIngressAck = ingress.publish(authenticatedClientId, lease, snapshot)

    suspend fun retire(authenticatedClientId: String, lease: SurfaceIngressLease) =
        ingress.retire(authenticatedClientId, lease)

    suspend fun publishPresentationReceipt(
        authenticatedClientId: String,
        lease: SurfaceIngressLease,
        receipt: FrontendPresentationReceipt,
    ): SurfaceIngressAck = ingress.publishPresentationReceipt(authenticatedClientId, lease, receipt)

    companion object {
        fun getInstance(project: Project): AuthenticatedSurfaceIngressService = project.service()
    }
}
