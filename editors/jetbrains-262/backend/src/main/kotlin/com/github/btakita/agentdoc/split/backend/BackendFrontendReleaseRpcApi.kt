@file:Suppress("UnstableApiUsage")

package com.github.btakita.agentdoc.split.backend

import com.github.btakita.agentdoc.shell.receipt.ShellIdentity
import com.github.btakita.agentdoc.split.FrontendClientBuild
import com.github.btakita.agentdoc.split.FrontendImplementationRelease
import com.github.btakita.agentdoc.split.FrontendReleaseOffer
import com.github.btakita.agentdoc.split.FrontendReleaseRpcApi
import com.github.btakita.agentdoc.split.FrontendShellCompatibility
import com.github.btakita.agentdoc.split.agentDocPluginDescriptorFact
import com.github.btakita.agentdoc.split.frontendShellCompatibility
import com.github.btakita.agentdoc.split.generation
import com.intellij.codeWithMe.ClientId
import com.intellij.openapi.diagnostic.Logger
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/** GH #435: this backend's own Agent Doc build, as the frontend-release RPCs report it. */
internal data class BackendPluginBuild(val version: String, val pluginPath: String, val shellDigest: String) {
    companion object {
        fun current(): BackendPluginBuild = BackendPluginBuild(
            version = agentDocPluginDescriptorFact("getVersion") ?: "unknown",
            pluginPath = agentDocPluginDescriptorFact("getPluginPath") ?: "unknown",
            shellDigest = ShellIdentity.shellDigest,
        )
    }
}

/**
 * GH #435 shell: the additive [FrontendReleaseRpcApi]. A Client names its own build with every
 * release poll, so the backend answers with the selected generation's identity (never its bytes)
 * and its own build, and logs the facts of a Client whose shell is not this backend's. Only a
 * Client on this backend's shell activates the backend peer.
 */
internal class BackendFrontendReleaseRpcApi(
    private val currentRelease: () -> FrontendImplementationRelease? = {
        activeSurfaceIngressFactory()?.currentFrontendRelease()
    },
    private val selectedRelease: () -> FrontendImplementationRelease? = {
        activeSurfaceIngressFactory()?.selectedFrontendRelease()
    },
    private val backend: () -> BackendPluginBuild = BackendPluginBuild::current,
    private val gate: FrontendReleaseDeliveryGate = FrontendReleaseDeliveryGate.shared,
    /** GH #440: a Client that names its build is never a legacy release poller. */
    private val releasePolls: FrontendReleasePollLedger = FrontendReleasePollLedger.shared,
    private val callerClientId: () -> String = { ClientId.current.value },
) : FrontendReleaseRpcApi {
    private val log = Logger.getInstance(BackendFrontendReleaseRpcApi::class.java)

    override suspend fun frontendReleaseOffer(client: FrontendClientBuild): FrontendReleaseOffer {
        val clientId = callerClientId()
        val build = backend()
        val observation = gate.observeBuild(clientId, client)
        releasePolls.clientBuild(clientId, client.shellDigest, client.pluginVersion, client.pluginPath)
        val compatibility = frontendShellCompatibility(
            clientVersion = client.pluginVersion,
            clientShell = client.shellDigest,
            backendVersion = build.version,
            backendShell = build.shellDigest,
        )
        // Nothing heavy before this Client's schema answer: release discovery reads and verifies the
        // package on disk, and the Client asks again as soon as its negotiation returns. Only a
        // Client on this backend's shell activates the backend peer.
        val release = when {
            gate.admission(clientId) == FrontendReleaseDeliveryGate.Admission.NOT_NEGOTIATED -> null
            else -> runCatching {
                withContext(Dispatchers.IO) {
                    if (gate.claimPeerActivation(clientId) ||
                        compatibility == FrontendShellCompatibility.SAME_SHELL
                    ) {
                        currentRelease()
                    } else {
                        selectedRelease()
                    }
                }
            }.onFailure { log.warn("[frontend-impl] release discovery failed client=$clientId", it) }.getOrNull()
        }
        gate.rememberPackageUrl(release?.packageUrl)
        if (compatibility != FrontendShellCompatibility.SAME_SHELL &&
            observation != FrontendReleaseDeliveryGate.ShellObservation.UNCHANGED
        ) {
            // `agent-doc upgrade` reads this line (src/plugin.rs): keep its `key=value` facts.
            log.warn(
                "[frontend-impl] client plugin mismatch client=$clientId " +
                    "compatibility=${compatibility.name.lowercase()} client_plugin=${client.pluginVersion} " +
                    "client_plugin_path=${client.pluginPath} client_shell=${client.shellDigest.take(12)} " +
                    "backend_plugin=${build.version} backend_plugin_path=${build.pluginPath} " +
                    "backend_shell=${build.shellDigest.take(12)}",
            )
        }
        return FrontendReleaseOffer(
            backendPluginVersion = build.version,
            backendPluginPath = build.pluginPath,
            backendShellDigest = build.shellDigest,
            compatibility = compatibility,
            release = release?.generation(),
        )
    }
}
