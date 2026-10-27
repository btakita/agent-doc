@file:Suppress("UnstableApiUsage")

package com.github.btakita.agentdoc.split.backend

import com.github.btakita.agentdoc.split.FrontendPresentationReceipt
import com.github.btakita.agentdoc.split.FrontendSurfaceSnapshot
import com.github.btakita.agentdoc.split.SurfaceIngressAck
import com.github.btakita.agentdoc.split.SurfaceIngressLease
import com.intellij.openapi.Disposable
import com.intellij.openapi.project.Project
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
