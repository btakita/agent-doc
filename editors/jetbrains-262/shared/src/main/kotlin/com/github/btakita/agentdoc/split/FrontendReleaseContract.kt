@file:Suppress("UnstableApiUsage")

package com.github.btakita.agentdoc.split

import com.intellij.openapi.extensions.PluginId
import com.intellij.platform.rpc.RemoteApiProviderService
import fleet.rpc.RemoteApi
import fleet.rpc.Rpc
import fleet.rpc.remoteApiDescriptor
import kotlinx.serialization.Serializable

/**
 * GH #435: the frontend-release RPCs added after [SurfaceSnapshotRpcApi] was frozen.
 *
 * It is a separate, additive `RemoteApi`: a backend that predates it fails the call, and a Client
 * that predates it never makes the call, while both keep the frozen [SurfaceSnapshotRpcApi]. It is
 * frozen the same way once shipped (`SharedRpcContractGuardTest`).
 */
@Rpc
interface FrontendReleaseRpcApi : RemoteApi<Unit> {
    companion object {
        suspend fun getInstance(): FrontendReleaseRpcApi =
            RemoteApiProviderService.resolve(remoteApiDescriptor<FrontendReleaseRpcApi>())
    }

    /**
     * The backend's answer to one Client's 30 s release poll: the frontend generation it selected,
     * named without its multi-megabyte package, and whether [client]'s shell can load it. [client]
     * names this Client's own build, so both sides can tell which one is stale.
     */
    suspend fun frontendReleaseOffer(client: FrontendClientBuild): FrontendReleaseOffer
}

/** GH #435: one JetBrains Client's Agent Doc frontend build, as its descriptor and shell report it. */
@Serializable
data class FrontendClientBuild(
    val pluginVersion: String,
    /** The loaded plugin folder on the Client machine (`.../plugins/frontend/agent-doc-jetbrains-262`). */
    val pluginPath: String,
    val shellDigest: String,
    val spiVersion: Int,
)

/** GH #435: one backend-selected frontend generation, without its package bytes. */
@Serializable
data class FrontendReleaseGeneration(
    val releaseVersion: String,
    val packageUrl: String,
    val implSha256: String,
    val shellDigest: String,
    val spiVersion: Int,
    val epoch: Long,
)

/** GH #435: how a Client's frontend shell relates to the backend's. */
@Serializable
enum class FrontendShellCompatibility {
    SAME_SHELL,
    CLIENT_OLDER,
    CLIENT_NEWER,
    DIFFERENT_BUILD,
}

/** GH #435: the backend's answer to [FrontendReleaseRpcApi.frontendReleaseOffer]. */
@Serializable
data class FrontendReleaseOffer(
    val backendPluginVersion: String,
    val backendPluginPath: String,
    val backendShellDigest: String,
    val compatibility: FrontendShellCompatibility,
    /**
     * Null while the backend has no signed release to offer (it stays shell-only), and before this
     * Client's schema negotiation was answered (the Client asks again as soon as it returns).
     */
    val release: FrontendReleaseGeneration? = null,
)

/** GH #435: the generation [this] release selects, without its bytes. */
fun FrontendImplementationRelease.generation(): FrontendReleaseGeneration =
    FrontendReleaseGeneration(
        releaseVersion = releaseVersion,
        packageUrl = packageUrl,
        implSha256 = implSha256,
        shellDigest = shellDigest,
        spiVersion = spiVersion,
        epoch = epoch,
    )

/** The shell identity a build reports when `generateShellIdentity` did not run. */
private const val UNKNOWN_SHELL = "unknown"

/**
 * GH #435: dotted numeric order of two Agent Doc plugin versions (`0.2.1046` < `0.2.1052`), or
 * null when either is missing or not a plain dotted version.
 */
fun compareAgentDocPluginVersions(left: String?, right: String?): Int? {
    fun parts(value: String?): List<Long>? = value?.trim()?.removePrefix("v")
        ?.split('.')
        ?.map { it.toLongOrNull() ?: return null }
        ?.takeIf { it.isNotEmpty() }
    val l = parts(left) ?: return null
    val r = parts(right) ?: return null
    for (index in 0 until maxOf(l.size, r.size)) {
        val order = l.getOrElse(index) { 0 }.compareTo(r.getOrElse(index) { 0 })
        if (order != 0) return order
    }
    return 0
}

/**
 * GH #435: how a Client whose frontend reports [clientShell] (and, when known, [clientVersion])
 * relates to a backend on [backendShell] / [backendVersion]. Equal shells are compatible whatever
 * the versions say; otherwise the plugin versions decide which side is stale.
 */
fun frontendShellCompatibility(
    clientVersion: String?,
    clientShell: String?,
    backendVersion: String?,
    backendShell: String,
): FrontendShellCompatibility {
    val provable = clientShell != null && clientShell != UNKNOWN_SHELL && backendShell != UNKNOWN_SHELL
    if (provable && clientShell == backendShell) return FrontendShellCompatibility.SAME_SHELL
    val order = compareAgentDocPluginVersions(clientVersion, backendVersion)
    return when {
        order == null -> FrontendShellCompatibility.DIFFERENT_BUILD
        order < 0 -> FrontendShellCompatibility.CLIENT_OLDER
        order > 0 -> FrontendShellCompatibility.CLIENT_NEWER
        // Same version: a shell that cannot be proven is the same build; two proven shells differ.
        provable -> FrontendShellCompatibility.DIFFERENT_BUILD
        else -> FrontendShellCompatibility.SAME_SHELL
    }
}

/**
 * GH #435: one fact (`getVersion`, `getPluginPath`) of this JVM's loaded Agent Doc descriptor, read
 * reflectively, so a log line names the plugin version and directory an upgrade must replace.
 */
fun agentDocPluginDescriptorFact(getter: String): String? = runCatching {
    val manager = Class.forName("com.intellij.ide.plugins.PluginManagerCore")
    val descriptor = manager.getMethod("getPlugin", PluginId::class.java)
        .invoke(null, PluginId.getId(AGENT_DOC_PLUGIN_ID)) ?: return null
    descriptor.javaClass.methods.first { it.name == getter && it.parameterCount == 0 }
        .invoke(descriptor)?.toString()
}.getOrNull()

const val AGENT_DOC_PLUGIN_ID: String = "com.github.btakita.agent-doc"
