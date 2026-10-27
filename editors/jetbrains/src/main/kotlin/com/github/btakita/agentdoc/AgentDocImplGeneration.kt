package com.github.btakita.agentdoc

import com.github.btakita.agentdoc.shell.GenerationContext
import com.github.btakita.agentdoc.shell.ImplAction
import com.github.btakita.agentdoc.shell.ImplGeneration
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
    }

    override fun action(actionId: String): ImplAction? {
        handlers[actionId]?.let { return it }
        val factory = ACTION_HANDLERS[actionId] ?: return null
        return handlers.computeIfAbsent(actionId) { factory() }
    }

    override fun quiesce(): String {
        handlers.clear()
        return ""
    }

    override fun resume() = Unit

    companion object {
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
