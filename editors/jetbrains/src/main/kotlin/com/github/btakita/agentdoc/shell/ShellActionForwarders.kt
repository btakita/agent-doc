package com.github.btakita.agentdoc.shell

import com.intellij.openapi.actionSystem.ActionUpdateThread
import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.project.DumbAware

/**
 * GH #339 R1: the one `AnAction` the platform holds for each `plugin.xml` action id. The shell
 * declares the id, text, shortcuts and groups; the forwarder delivers `update` and
 * `actionPerformed` to the active implementation generation's [ImplAction] for that id.
 *
 * Every agent-doc action reads markdown files and calls the native library, never the PSI
 * indexes, so every forwarder is [DumbAware] and updates on `BGT`. Both are registration facts
 * the shell owns: an implementation cannot choose another update thread.
 *
 * A new action id, or a new default shortcut, is a shell change (and an IDE restart): add a
 * forwarder here, its `<action>` in `plugin.xml`, and its handler in `AgentDocImplGeneration`.
 */
abstract class ShellActionForwarder(val actionId: String) : AnAction(), DumbAware {
    final override fun getActionUpdateThread(): ActionUpdateThread = ActionUpdateThread.BGT

    final override fun update(e: AnActionEvent) {
        val handler = ShellGenerationHost.getInstance().action(actionId)
        if (handler == null) {
            e.presentation.isEnabledAndVisible = false
            return
        }
        handler.update(e)
    }

    final override fun actionPerformed(e: AnActionEvent) {
        val handler = ShellGenerationHost.getInstance().action(actionId)
        if (handler == null) {
            LOG.warn("[shell] no implementation handler for $actionId; the action was not run")
            return
        }
        handler.actionPerformed(e)
    }

    private companion object {
        val LOG = Logger.getInstance(ShellActionForwarder::class.java)
    }
}

class PopupForwarder : ShellActionForwarder("AgentDoc.Popup")
class SubmitForwarder : ShellActionForwarder("AgentDoc.Submit")
class StartAgentDocForwarder : ShellActionForwarder("AgentDoc.StartAgentDoc")
class InitSessionForwarder : ShellActionForwarder("AgentDoc.InitSession")
class FixDocumentForwarder : ShellActionForwarder("AgentDoc.FixDocument")
class LintDocumentForwarder : ShellActionForwarder("AgentDoc.LintDocument")
class RunWithJunieForwarder : ShellActionForwarder("AgentDoc.RunWithJunie")
class ClaimForwarder : ShellActionForwarder("AgentDoc.Claim")
class CompactExchangeForwarder : ShellActionForwarder("AgentDoc.CompactExchange")
class ForceClaimForwarder : ShellActionForwarder("AgentDoc.ForceClaim")
class SyncLayoutForwarder : ShellActionForwarder("AgentDoc.SyncLayout")
class LoadTmuxWindowForwarder : ShellActionForwarder("AgentDoc.LoadTmuxWindow")
class ShowSessionStatusForwarder : ShellActionForwarder("AgentDoc.ShowSessionStatus")
class RestartSupervisorProcessForwarder : ShellActionForwarder("AgentDoc.RestartSupervisorProcess")
class RestartAgentForwarder : ShellActionForwarder("AgentDoc.RestartAgent")
class StopAgentForwarder : ShellActionForwarder("AgentDoc.StopAgent")
class CancelTurnForwarder : ShellActionForwarder("AgentDoc.CancelTurn")
class KillSupervisorForwarder : ShellActionForwarder("AgentDoc.KillSupervisor")
class ClearSessionContextForwarder : ShellActionForwarder("AgentDoc.ClearSessionContext")
class InterruptClearSessionContextForwarder : ShellActionForwarder("AgentDoc.InterruptClearSessionContext")
class CopySessionDiagnosticsForwarder : ShellActionForwarder("AgentDoc.CopySessionDiagnostics")
class RefreshEnvironmentForwarder : ShellActionForwarder("AgentDoc.RefreshEnvironment")
class InstallCliForwarder : ShellActionForwarder("AgentDoc.InstallCli")
class UpdateChannelForwarder : ShellActionForwarder("AgentDoc.UpdateChannel")
class ResyncFixSessionsForwarder : ShellActionForwarder("AgentDoc.ResyncFixSessions")
class GcStaleSessionsForwarder : ShellActionForwarder("AgentDoc.GcStaleSessions")
class AboutForwarder : ShellActionForwarder("AgentDoc.About")
class DashboardForwarder : ShellActionForwarder("AgentDoc.Dashboard")
