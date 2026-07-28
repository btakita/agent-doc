package com.github.btakita.agentdoc

import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.diagnostic.Logger
import java.util.concurrent.atomic.AtomicBoolean

/**
 * Application-wide native generation handoff.
 *
 * Every project shares the same JNA-loaded cdylib. A reload therefore pauses
 * every project adapter, closes its replicas/listeners against the old
 * generation, publishes one replacement generation, and then re-registers the
 * projects. Duplicate typed intents coalesce behind the active handoff.
 */
internal object NativeReloadCoordinator {
    private val log = Logger.getInstance(NativeReloadCoordinator::class.java)
    private val scheduled = AtomicBoolean(false)

    fun requestReload(libVersion: String? = null) {
        if (!scheduled.compareAndSet(false, true)) return
        ApplicationManager.getApplication().executeOnPooledThread {
            var replicaProjects = emptyList<com.intellij.openapi.project.Project>()
            var watchers = emptyList<PatchWatcher>()
            try {
                // Stop inbound callbacks before tearing down the CRDT managers
                // they call. The reverse order used to dispose every manager,
                // discover one busy listener, then rebuild all open replicas in
                // `finally`; that turned a failed reload into a read-lock convoy.
                val watcherQuiesce = PatchWatcher.quiesceAllForNativeReload()
                watchers = watcherQuiesce.first
                if (!watcherQuiesce.second) {
                    log.warn("[native] reload failed closed; an IPC listener did not terminate")
                    return@executeOnPooledThread
                }
                val replicaQuiesce = CrdtReplicaManager.quiesceAllForNativeReload()
                replicaProjects = replicaQuiesce.first
                if (!replicaQuiesce.second) {
                    log.warn("[native] reload failed closed; a CRDT worker did not terminate")
                    return@executeOnPooledThread
                }
                when (val outcome = AgentDocLib.hotReload(libVersion)) {
                    NativeReloadOutcome.AlreadyCurrent ->
                        log.debug("[native] reload intent already satisfied")
                    is NativeReloadOutcome.Reloaded ->
                        log.info("[native] published native generation mtime=${outcome.mtime}")
                    is NativeReloadOutcome.RetainedOld ->
                        log.warn("[native] reload failed closed; retained old generation: ${outcome.reason}")
                    is NativeReloadOutcome.RestartRequired ->
                        log.warn("[native] reload requires IDE restart: ${outcome.reason}")
                }
            } catch (error: Throwable) {
                log.warn("[native] reload coordinator failed closed", error)
            } finally {
                watchers.forEach { it.restartNativeEndpointsAfterReload() }
                CrdtReplicaManager.restartAfterNativeReload(replicaProjects)
                scheduled.set(false)
            }
        }
    }
}
