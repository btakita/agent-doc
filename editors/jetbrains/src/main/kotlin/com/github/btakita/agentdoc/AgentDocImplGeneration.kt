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

    override fun quiesce(): String {
        handlers.clear()
        return ""
    }

    override fun resume() = Unit

    override fun retire() = AgentDocProjectLifecycle.retire()

    companion object {
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
