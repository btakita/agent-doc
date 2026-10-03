package com.github.btakita.agentdoc

import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.project.ProjectManager
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicReference

/** A reload fired because the library file under the running IDE changed (an install). */
internal const val NATIVE_RELOAD_TRIGGER_MTIME = "mtime"

/** A reload fired because a `reload_library` IPC intent arrived. */
internal const val NATIVE_RELOAD_TRIGGER_IPC = "ipc"

/**
 * `#hotreloadversion` (#59): the hot-reload log line. The version is the one the replacement
 * library reported when it was validated, never the trigger; an IPC intent's announced version is
 * added only when it disagrees with what actually loaded.
 */
internal fun hotReloadLogLineUtil(
    loadedVersion: String?,
    trigger: String,
    announcedLibVersion: String?,
    path: String,
): String {
    val version = loadedVersion?.takeIf { it.isNotBlank() }?.let { "v$it" } ?: "version=unknown"
    val announced =
        announcedLibVersion
            ?.takeIf { it.isNotBlank() && it != loadedVersion }
            ?.let { " announced=v$it" }
            .orEmpty()
    return "[native] hot-reloaded libagent_doc $version (trigger=$trigger$announced) from $path " +
        "after quiesce/close handoff"
}

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
    private val retiredReloadLogged = java.util.concurrent.atomic.AtomicBoolean(false)

    internal const val USER_ACTION_AWAIT_MS = 30_000L

    fun awaitReady(timeoutMs: Long = USER_ACTION_AWAIT_MS): Boolean =
        reloadGate.awaitReady(timeoutMs)

    /**
     * [trigger] says why the reload fired (`mtime`, `ipc`, ...); [announcedLibVersion] is the version
     * an IPC `reload_library` intent named, when there is one. They used to share one `libVersion`
     * parameter, so an mtime reload logged `libagent_doc vmtime` (#59).
     */
    fun requestReload(trigger: String, announcedLibVersion: String? = null) {
        if (PluginGeneration.retired) {
            // `#pluginunloadresurrect`: the replacement generation owns reloads.
            // `#reloadignorestorm`: log once, with the caller, so a leaked caller that keeps
            // this generation alive is identifiable instead of flooding idea.log.
            if (retiredReloadLogged.compareAndSet(false, true)) {
                log.info(
                    "[native] reload ignored by an unloaded plugin generation trigger=$trigger (logged once)",
                    Throwable("retired-generation reload caller"),
                )
            }
            return
        }
        val handoff = reloadGate.begin() ?: return
        if (AgentDocLib.reloadWouldKeepCurrentGeneration()) {
            // `#steerreplicachurn`: nothing would load, so nothing may be torn
            // down. Quiescing first used to deregister every open document's
            // replica and re-register it from a fresh cut, once per repeated
            // `reload_library` request, while the generation never changed.
            log.info("[native] reload intent already satisfied trigger=$trigger; replicas untouched")
            reloadGate.complete(handoff)
            return
        }
        try {
            ApplicationManager.getApplication().executeOnPooledThread {
                var replicaHandoff: NativeReloadReplicaHandoff? = null
                var replicaQuiesceAttempted = false
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
                    replicaQuiesceAttempted = true
                    val replicaQuiesce = CrdtReplicaManager.quiesceAllForNativeReload()
                    replicaHandoff = replicaQuiesce
                    if (!replicaQuiesce.reloadSafe) {
                        log.warn(
                            "[native] reload failed closed; every attached CRDT replica could not be " +
                                "checkpointed and quiesced",
                        )
                        return@executeOnPooledThread
                    }
                    when (val outcome = AgentDocLib.hotReload(trigger, announcedLibVersion)) {
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
                            val registrations =
                                ReliableSyncLivenessListener.republishOpenDocumentsAfterNativeReload(
                                    surfaceProjects,
                                )
                            log.info(
                                "[native] republished reliable-liveness registrations " +
                                    "before replica restart count=$registrations",
                            )
                        } catch (error: Throwable) {
                            log.warn("[native] reload liveness republish failed", error)
                        }
                        try {
                            val report =
                                if (
                                    nativeReloadReplicaRestartRequiredUtil(
                                        replicaQuiesceAttempted,
                                        replicaHandoff,
                                    )
                                ) {
                                    CrdtReplicaManager.restartAfterNativeReload(
                                        replicaHandoff
                                            ?: NativeReloadReplicaHandoff(emptyMap(), reloadSafe = false),
                                        surfaceProjects,
                                    )
                                } else {
                                    null
                                }
                            if (report == null) {
                                log.info("[native] replica restart skipped; the live replicas were never torn down")
                            } else if (report.expected == 0) {
                                log.warn(
                                    "[native] replica restart observed no open markdown documents " +
                                        "attached=0/0 live_projects=${report.liveProjects}",
                                )
                            } else if (report.converged) {
                                log.info(
                                    "[native] replica restart converged " +
                                        "attached=${report.attached}/${report.expected}",
                                )
                            } else {
                                log.warn(
                                    "[native] replica restart incomplete " +
                                        "attached=${report.attached}/${report.expected} " +
                                        "failed=${report.failedPaths.joinToString(",")}",
                                )
                            }
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
