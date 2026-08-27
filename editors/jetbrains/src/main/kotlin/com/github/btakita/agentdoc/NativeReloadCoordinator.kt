package com.github.btakita.agentdoc

import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.project.ProjectManager
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicReference

internal class NativeReloadGate {
    internal class Handoff internal constructor(
        internal val completion: CountDownLatch = CountDownLatch(1),
    )

    private val active = AtomicReference<Handoff?>(null)

    fun begin(): Handoff? {
        val handoff = Handoff()
        return if (active.compareAndSet(null, handoff)) handoff else null
    }

    fun awaitReady(timeoutMs: Long): Boolean {
        val handoff = active.get() ?: return true
        return try {
            handoff.completion.await(timeoutMs.coerceAtLeast(0L), TimeUnit.MILLISECONDS)
        } catch (_: InterruptedException) {
            Thread.currentThread().interrupt()
            false
        }
    }

    fun complete(handoff: Handoff) {
        active.compareAndSet(handoff, null)
        handoff.completion.countDown()
    }
}

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
    private val reloadGate = NativeReloadGate()

    internal const val USER_ACTION_AWAIT_MS = 30_000L

    fun awaitReady(timeoutMs: Long = USER_ACTION_AWAIT_MS): Boolean =
        reloadGate.awaitReady(timeoutMs)

    fun requestReload(libVersion: String? = null) {
        val handoff = reloadGate.begin() ?: return
        try {
            ApplicationManager.getApplication().executeOnPooledThread {
                var replicaProjects = emptyList<com.intellij.openapi.project.Project>()
                var watchers = emptyList<PatchWatcher>()
                var surfaceProjects = emptyList<com.intellij.openapi.project.Project>()
                try {
                    surfaceProjects =
                        ProjectManager.getInstance().openProjects.filterNot { it.isDisposed }.toList()
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
                    try {
                        try {
                            watchers.forEach { it.restartNativeEndpointsAfterReload() }
                        } catch (error: Throwable) {
                            log.warn("[native] reload watcher restart failed", error)
                        }
                        try {
                            CrdtReplicaManager.restartAfterNativeReload(replicaProjects)
                        } catch (error: Throwable) {
                            log.warn("[native] reload replica restart failed", error)
                        }
                        // The retired generation deliberately discarded every editor
                        // surface and document-authority subscription. Republish the
                        // current open-tab surface into whichever generation won the
                        // handoff so inactive tabs are warm again without waiting for
                        // an operator focus or selection event.
                        try {
                            surfaceProjects
                                .filterNot { it.isDisposed }
                                .forEach { project ->
                                    EditorTabSyncListener.install(project).onEditorLayoutChanged(project)
                                }
                        } catch (error: Throwable) {
                            log.warn("[native] reload surface republish failed", error)
                        }
                    } finally {
                        reloadGate.complete(handoff)
                    }
                }
            }
        } catch (error: Throwable) {
            reloadGate.complete(handoff)
            throw error
        }
    }
}
