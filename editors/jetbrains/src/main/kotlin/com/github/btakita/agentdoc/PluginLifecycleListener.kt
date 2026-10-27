package com.github.btakita.agentdoc

import com.github.btakita.agentdoc.shell.ShellGenerationHost
import com.github.btakita.agentdoc.shell.ShellListenerHub
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.Disposable
import com.intellij.openapi.project.Project
import com.intellij.openapi.project.ProjectManagerListener
import com.intellij.openapi.util.Disposer
import java.util.LinkedHashMap
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicReference

/**
 * GH #339 R1 shell: the `<projectListeners>` owner. It forwards `projectOpened`/`projectClosed`
 * to the active implementation generation, whose `AgentDocProjectLifecycle` owns per-project
 * startup and cleanup for both project close and dynamic plugin unload.
 *
 * Registered in plugin.xml as a projectListener so IntelliJ manages the project lifecycle. The
 * application-level [PluginUnloadCleanupService] retires the generation while open projects
 * survive a dynamic plugin replacement.
 */
class PluginLifecycleListener : ProjectManagerListener {
    override fun projectOpened(project: Project) {
        // Force creation of the application service whose Disposable boundary is plugin unload.
        ApplicationManager.getApplication().getService(PluginUnloadCleanupService::class.java)
        // GH #339 R1: activate the implementation generation behind the shell forwarders (which
        // logs this JVM's `agent_doc_activation_receipt/v1` line once per plugin load) and hand
        // it the project. Null only once the plugin is unloading.
        ShellGenerationHost.getInstance().activate()?.projectOpened(project)
    }

    override fun projectClosed(project: Project) {
        ShellGenerationHost.getInstance().activate()?.disposeProject(project)
    }

    companion object {
        /**
         * The classic attach bridge's hook (`JetBrainsPluginUpgradeAction.reattachOpenDocuments`
         * resolves it by name on this class): rebuild plugin-owned project services after a
         * dynamic package replacement and return the one-line reattach receipt.
         */
        @JvmStatic
        fun initializeOpenProjectsAfterDynamicLoad(): String =
            ShellGenerationHost.getInstance().activate()?.initializeOpenProjectsAfterDynamicLoad()
                ?: "documents=0/0:reattach_error=implementation_generation_retired"

        /**
         * The classic attach bridge's hook (`cleanupOutgoingGeneration`): stop every
         * outgoing-generation listener before IntelliJ unloads its classloader.
         */
        @JvmStatic
        fun disposeOpenProjectsForDynamicUnload(): Int =
            ShellGenerationHost.getInstance().activate()?.disposeOpenProjectsForDynamicUnload() ?: 0
    }
}

/** Project-owned parent disposable for programmatic listeners installed at startup. */
class ProjectPluginLifecycleService(
    private val project: Project,
) : Disposable {
    /**
     * GH #339 R1 Tier A: this project's message-bus forwarders. Their connections end with this
     * service, so they outlive a dynamic-unload `endInitialization` exactly like the service.
     */
    internal val listeners = ShellListenerHub(this, "project") { project.messageBus }

    private val scope = InitializationScope<Disposable>(
        open = { Disposer.newDisposable("agent-doc project lifecycle").also { Disposer.register(this, it) } },
        close = Disposer::dispose,
    )

    /** This initialization's listener scope, or null when the project is already initialized. */
    fun beginInitialization(): Disposable? = scope.begin()

    /** Drop the current initialization's listeners so a later [beginInitialization] succeeds. */
    fun endInitialization() {
        scope.end()
    }

    override fun dispose() {
        scope.forget()
        ShellGenerationHost.getInstance().activate()?.disposeProject(project)
    }
}

/**
 * One-at-a-time initialization scope (GH #94).
 *
 * The project service used to hold a one-shot `initialized` flag, so after a dynamic-unload
 * release the still-live generation could never re-initialize the project: an upgrade that then
 * aborted left the replica transport deregistered until the IDE restarted. A scope can be ended
 * and begun again; [end] closes the listeners bound to the scope it hands out.
 */
internal class InitializationScope<S : Any>(
    private val open: () -> S,
    private val close: (S) -> Unit,
) {
    private val current = AtomicReference<S?>(null)
    private val opening = AtomicBoolean(false)

    fun begin(): S? {
        if (!opening.compareAndSet(false, true)) return null
        return try {
            if (current.get() != null) null else open().also { current.set(it) }
        } finally {
            opening.set(false)
        }
    }

    fun end() {
        current.getAndSet(null)?.let(close)
    }

    /** The owner is disposing and closes the scope itself; just stop tracking it. */
    fun forget() {
        current.set(null)
    }

    fun isOpen(): Boolean = current.get() != null
}

/**
 * Plugin-owned application service whose disposal boundary is a dynamic plugin unload.
 *
 * Projects remain open while JetBrains swaps the plugin classloader, so their project-close
 * listeners do not run. Clearing every static per-project registry here prevents the old
 * classloader from being retained and lets the replacement generation initialize cleanly.
 */
class PluginUnloadCleanupService : Disposable {
    override fun dispose() {
        // Retire first: anything still queued in this classloader (a reload
        // intent, an EDT runnable) must find the generation retired before the
        // managers it would otherwise recreate are gone.
        PluginGeneration.retire()
        // GH #339 R1: the implementation generation releases every open project's resources and
        // the turn-state strips. IntelliJ may dispose ShellGenerationHost before or after this
        // service; whichever runs first retires the generation and the other finds nothing left.
        ShellGenerationHost.getInstanceIfCreated()?.retireActiveGeneration()
        // `#jbclassloaderthreads`: application-global Kotlin objects belong to this plugin
        // classloader, not to a project. Dynamic unload used to leave their executors and the
        // native-library shutdown hook alive forever: after eleven upgrades IDEA owned 44 native
        // workers plus eleven current-document and socket-watchdog workers. Close only resources
        // that this generation actually created, and do it after project teardown has used them
        // for its final deregistration.
        PluginGeneration.closeResources()
    }
}

/**
 * `#pluginunloadresurrect`: whether this classloader's plugin generation has been
 * unloaded. A dynamic upgrade disposes the old generation's managers, but a
 * `reload_library` intent still reaching the old classloader ran its native-reload
 * restart, which recreated a `CrdtReplicaManager` there. That resurrected
 * generation kept attaching documents under the retired editor identity while
 * the replacement had published liveness under its own, so the controller
 * refused every registration as a stale endpoint and those documents stayed
 * detached until an IDE restart. Once retired, a generation never builds
 * replicas or reloads native code again.
 */
internal object PluginGeneration {
    private val resourceLock = Any()
    private val resources = LinkedHashMap<String, () -> Unit>()

    @Volatile
    var resourcesClosed: Boolean = false
        private set

    @Volatile
    var retired: Boolean = false
        private set

    fun retire() {
        retired = true
    }

    /** Register one application-global resource owned by this plugin classloader. */
    fun registerResource(name: String, close: () -> Unit) {
        val closeImmediately =
            synchronized(resourceLock) {
                if (resourcesClosed) {
                    true
                } else {
                    resources.putIfAbsent(name, close)
                    false
                }
            }
        if (closeImmediately) {
            close()
        }
    }

    /** Close in reverse creation order; every closer must be idempotent and non-throwing. */
    fun closeResources() {
        val closers = synchronized(resourceLock) {
            resourcesClosed = true
            resources.values.toList().asReversed().also { resources.clear() }
        }
        closers.forEach { close ->
            try {
                close()
            } catch (error: Throwable) {
                com.intellij.openapi.diagnostic.Logger
                    .getInstance(PluginGeneration::class.java)
                    .warn("[plugin-lifecycle] generation resource cleanup failed", error)
            }
        }
    }

    /** Tests share one classloader; production never un-retires a generation. */
    internal fun resetForTest() {
        retired = false
        resourcesClosed = false
    }
}
