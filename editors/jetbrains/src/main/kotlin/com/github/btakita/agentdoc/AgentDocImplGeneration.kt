package com.github.btakita.agentdoc

import com.github.btakita.agentdoc.shell.GenerationContext
import com.github.btakita.agentdoc.shell.ImplAction
import com.github.btakita.agentdoc.shell.ImplEditorExtensions
import com.github.btakita.agentdoc.shell.ImplGeneration
import com.github.btakita.agentdoc.shell.ShellGenerationHost
import com.intellij.openapi.Disposable
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.editor.event.DocumentListener
import com.intellij.openapi.editor.event.EditorFactoryListener
import com.intellij.openapi.editor.event.EditorMouseListener
import com.intellij.openapi.project.Project
import com.intellij.util.messages.Topic
import java.util.concurrent.ConcurrentHashMap

/**
 * GH #339 R1: the implementation generation the shell instantiates by name
 * (`IMPL_GENERATION_CLASS`). It serves the handler for every shell-declared action id.
 *
 * R1 keeps the generation in the plugin classloader and carries no handoff state, so [quiesce]
 * exports nothing and a cold start is the only start. Subsystems move behind this class one at a
 * time (SPEC.md "Migration plan"), each with its own handoff schema.
 */
class AgentDocImplGeneration : ImplGeneration {
    private val handlers = ConcurrentHashMap<String, ImplAction>()

    @Volatile private var context: GenerationContext? = null

    override val implVersion: String
        get() = pluginVersion()

    override fun prepare(context: GenerationContext, importedState: String?) {
        this.context = context
        activeContext = context
    }

    override fun action(actionId: String): ImplAction? {
        handlers[actionId]?.let { return it }
        val factory = ACTION_HANDLERS[actionId] ?: return null
        return handlers.computeIfAbsent(actionId) { factory() }
    }

    override fun projectOpened(project: Project) = AgentDocProjectLifecycle.projectOpened(project)

    override fun disposeProject(project: Project) = AgentDocProjectLifecycle.disposeProjectResources(project)

    override fun initializeOpenProjectsAfterDynamicLoad(): String =
        AgentDocProjectLifecycle.initializeOpenProjectsAfterDynamicLoad()

    override fun disposeOpenProjectsForDynamicUnload(): Int =
        AgentDocProjectLifecycle.disposeOpenProjectsForDynamicUnload()

    override fun editorExtensions(): ImplEditorExtensions = AgentDocEditorExtensions

    private val moduleServices = ConcurrentHashMap<String, Any>()

    override fun <T : Any> moduleService(type: Class<T>): T? {
        val implementation = MODULE_SERVICES[type.name] ?: return null
        val service = moduleServices.computeIfAbsent(type.name) {
            // R1: the plugin (or content-module) classloader defines both; by name, so the
            // classic package, which has no such module, compiles without it.
            val loaded = try {
                Class.forName(implementation, true, AgentDocImplGeneration::class.java.classLoader)
            } catch (_: ClassNotFoundException) {
                return@computeIfAbsent ABSENT
            }
            loaded.getDeclaredConstructor().newInstance()
        }
        return if (service === ABSENT) null else type.cast(service)
    }

    override fun quiesce(): String {
        handlers.clear()
        return ""
    }

    override fun resume() = Unit

    override fun retire() = AgentDocProjectLifecycle.retire()

    companion object {
        private val ABSENT = Any()

        /**
         * Shell-declared module interfaces and this generation's implementation of each, by
         * class name: the 262/263 backend's Fleet RPC surface ingress
         * (`editors/jetbrains-262/backend/.../SurfaceIngressSpi.kt`).
         */
        internal val MODULE_SERVICES: Map<String, String> = mapOf(
            "com.github.btakita.agentdoc.split.backend.SurfaceIngressFactory" to
                "com.github.btakita.agentdoc.split.backend.ControllerSurfaceIngressFactory",
        )

        /**
         * The context the shell lent this classloader's generation. R1 defines exactly one
         * generation per plugin classloader, so implementation code reaches it statically.
         */
        @Volatile internal var activeContext: GenerationContext? = null
            private set

        /**
         * One handler per `plugin.xml` action id, in descriptor order. Handlers are created on
         * first use and live as long as this generation, the way the platform kept one instance
         * of each action class before R1.
         */
        internal val ACTION_HANDLERS: Map<String, () -> ImplAction> = linkedMapOf(
            "AgentDoc.Popup" to ::AgentDocPopupAction,
            "AgentDoc.Submit" to ::SubmitAction,
            "AgentDoc.StartAgentDoc" to ::StartAgentDocAction,
            "AgentDoc.InitSession" to ::InitSessionAction,
            "AgentDoc.FixDocument" to ::FixDocumentAction,
            "AgentDoc.LintDocument" to ::LintDocumentAction,
            "AgentDoc.RunWithJunie" to ::RunWithJunieAction,
            "AgentDoc.Claim" to ::ClaimAction,
            "AgentDoc.CompactExchange" to ::CompactExchangeAction,
            "AgentDoc.ForceClaim" to ::ForceClaimAction,
            "AgentDoc.SyncLayout" to ::SyncLayoutAction,
            "AgentDoc.LoadTmuxWindow" to ::LoadTmuxWindowAction,
            "AgentDoc.ShowSessionStatus" to ::ShowSessionStatusAction,
            "AgentDoc.RestartSupervisorProcess" to ::RestartSupervisorProcessAction,
            "AgentDoc.RestartAgent" to ::RestartAgentAction,
            "AgentDoc.StopAgent" to ::StopAgentAction,
            "AgentDoc.CancelTurn" to ::CancelTurnAction,
            "AgentDoc.KillSupervisor" to ::KillSupervisorAction,
            "AgentDoc.ClearSessionContext" to ::ClearSessionContextAction,
            "AgentDoc.InterruptClearSessionContext" to ::InterruptClearSessionContextAction,
            "AgentDoc.CopySessionDiagnostics" to ::CopySessionDiagnosticsAction,
            "AgentDoc.RefreshEnvironment" to ::RefreshEnvironmentAction,
            "AgentDoc.InstallCli" to ::InstallAgentDocCliAction,
            "AgentDoc.UpdateChannel" to ::AgentDocUpdateChannelAction,
            "AgentDoc.ResyncFixSessions" to ::ResyncFixSessionsAction,
            "AgentDoc.GcStaleSessions" to ::GcStaleSessionsAction,
            "AgentDoc.About" to ::AboutAgentDocAction,
            "AgentDoc.Dashboard" to ::DashboardAction,
        )
    }
}

/**
 * GH #339 R1 Tier A: implementation code stages every programmatic platform listener here, never
 * on a message bus, the editor event multicaster or the editor factory directly. Each call goes
 * to the shell's forwarder for that topic through the active [GenerationContext]; `parent` keeps
 * the meaning it had for the direct registration.
 */
internal object GenerationListeners {
    private val LOG = Logger.getInstance(GenerationListeners::class.java)

    /** The active generation's context, activating it on first use; null once unloading. */
    private fun context(what: String): GenerationContext? {
        AgentDocImplGeneration.activeContext?.let { return it }
        ShellGenerationHost.getInstance().activate()
        return AgentDocImplGeneration.activeContext.also {
            if (it == null) LOG.info("[shell] no implementation generation to stage $what; the plugin is unloading")
        }
    }

    fun <L : Any> subscribe(topic: Topic<L>, handler: L, parent: Disposable) {
        context(topic.displayName)?.subscribe(topic, handler, parent)
    }

    fun <L : Any> subscribe(project: Project, topic: Topic<L>, handler: L, parent: Disposable) {
        context(topic.displayName)?.subscribe(project, topic, handler, parent)
    }

    fun addDocumentListener(listener: DocumentListener, parent: Disposable) {
        context("document listener")?.addDocumentListener(listener, parent)
    }

    fun addEditorMouseListener(listener: EditorMouseListener, parent: Disposable) {
        context("editor mouse listener")?.addEditorMouseListener(listener, parent)
    }

    fun addEditorFactoryListener(listener: EditorFactoryListener, parent: Disposable) {
        context("editor factory listener")?.addEditorFactoryListener(listener, parent)
    }
}

/**
 * GH #339 R1, EDT and pooled work: implementation code queues EDT tasks, pooled and scheduled
 * work, and creates executors, threads and alarms only here, never through `Application`,
 * `AppExecutorUtil`, `Executors` or `Thread` directly. Each call goes to the active generation's
 * shell work owner (`GenerationContext.invokeLater` / `GenerationContext.work`).
 *
 * With no generation (the plugin is unloading and the host already retired it), each call falls
 * back to the platform call it replaced, so late work behaves exactly as it did before R1.
 */
internal object GenerationWork {
    /** The active generation's context, activating it on first use; null without an application. */
    private fun context(): GenerationContext? {
        AgentDocImplGeneration.activeContext?.let { return it }
        com.intellij.openapi.application.ApplicationManager.getApplication()
            ?.getService(ShellGenerationHost::class.java)
            ?.activate()
        return AgentDocImplGeneration.activeContext
    }

    fun invokeLater(task: Runnable) {
        val context = context()
        if (context != null) {
            context.invokeLater(task)
        } else {
            com.intellij.openapi.application.ApplicationManager.getApplication().invokeLater(task)
        }
    }

    fun executeOnPooledThread(task: Runnable): java.util.concurrent.Future<*> =
        context()?.work?.executeOnPooledThread(task)
            ?: com.intellij.openapi.application.ApplicationManager.getApplication().executeOnPooledThread(task)

    fun schedule(task: Runnable, delay: Long, unit: java.util.concurrent.TimeUnit): java.util.concurrent.ScheduledFuture<*> =
        context()?.work?.schedule(task, delay, unit)
            ?: com.intellij.util.concurrency.AppExecutorUtil.getAppScheduledExecutorService().schedule(task, delay, unit)

    fun scheduleWithFixedDelay(
        task: Runnable,
        initialDelay: Long,
        delay: Long,
        unit: java.util.concurrent.TimeUnit,
    ): java.util.concurrent.ScheduledFuture<*> =
        context()?.work?.scheduleWithFixedDelay(task, initialDelay, delay, unit)
            ?: com.intellij.util.concurrency.AppExecutorUtil.getAppScheduledExecutorService()
                .scheduleWithFixedDelay(task, initialDelay, delay, unit)

    fun submit(task: Runnable): java.util.concurrent.Future<*> =
        context()?.work?.submit(task)
            ?: com.intellij.util.concurrency.AppExecutorUtil.getAppExecutorService().submit(task)

    fun newSingleThreadScheduledExecutor(threadName: String): java.util.concurrent.ScheduledExecutorService =
        context()?.work?.newSingleThreadScheduledExecutor(threadName)
            ?: java.util.concurrent.Executors.newSingleThreadScheduledExecutor(daemonThreads { threadName })

    fun newSingleThreadExecutor(threadName: String): java.util.concurrent.ExecutorService =
        context()?.work?.newSingleThreadExecutor(threadName)
            ?: java.util.concurrent.Executors.newSingleThreadExecutor(daemonThreads { threadName })

    fun threadFactory(threadName: () -> String): java.util.concurrent.ThreadFactory =
        context()?.work?.threadFactory(threadName) ?: daemonThreads(threadName)

    /** An unstarted thread, as `Thread(task)`. */
    fun newThread(task: Runnable): java.lang.Thread =
        context()?.work?.newThread(task) ?: java.lang.Thread(task)

    /** An unstarted thread, as `Thread(task, threadName)`. */
    fun newThread(task: Runnable, threadName: String): java.lang.Thread =
        context()?.work?.newThread(task, threadName) ?: java.lang.Thread(task, threadName)

    fun newPooledAlarm(parent: Disposable): com.intellij.util.Alarm =
        context()?.work?.newPooledAlarm(parent)
            ?: com.intellij.util.Alarm(com.intellij.util.Alarm.ThreadToUse.POOLED_THREAD, parent)

    private fun daemonThreads(threadName: () -> String) = java.util.concurrent.ThreadFactory { runnable ->
        java.lang.Thread(runnable, threadName()).apply { isDaemon = true }
    }
}

/**
 * GH #339 R1, per-platform-object state: implementation code creates every user-data `Key`
 * here, never with `Key.create`. The shell creates the key (`GenerationContext.userDataKey`), so it
 * outlives this generation; store only shell or JDK values under it.
 */
internal object GenerationKeys {
    fun <T> create(name: String): com.intellij.openapi.util.Key<T> {
        val context = AgentDocImplGeneration.activeContext ?: run {
            com.intellij.openapi.application.ApplicationManager.getApplication()
                ?.getService(ShellGenerationHost::class.java)
                ?.activate()
            AgentDocImplGeneration.activeContext
        }
        return context?.userDataKey(name) ?: com.github.btakita.agentdoc.shell.ShellUserDataKeys.key(name)
    }
}
