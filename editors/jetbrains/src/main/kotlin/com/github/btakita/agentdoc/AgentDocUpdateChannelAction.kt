package com.github.btakita.agentdoc

import com.github.btakita.agentdoc.shell.ImplAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.application.ApplicationInfo
import com.intellij.openapi.updateSettings.impl.UpdateSettings

/** Opts the IDE into Agent Doc's GitHub-hosted update feed, like IdeaVim's EAP toggle. */
class AgentDocUpdateChannelAction : ImplAction {
    companion object {
        internal const val REPOSITORY_PREFIX =
            "https://github.com/btakita/agent-doc/releases/latest/download/agent-doc-jetbrains-"
        internal const val CLASSIC_REPOSITORY_URL = "${REPOSITORY_PREFIX}classic.xml"
        internal const val EXACT_262_REPOSITORY_URL = "${REPOSITORY_PREFIX}262.xml"
        internal const val EXACT_263_REPOSITORY_URL = "${REPOSITORY_PREFIX}263.xml"
        internal val ALL_REPOSITORY_URLS =
            setOf(CLASSIC_REPOSITORY_URL, EXACT_262_REPOSITORY_URL, EXACT_263_REPOSITORY_URL)

        internal fun repositoryUrl(baselineVersion: Int): String? =
            when (baselineVersion) {
                in 242..261 -> CLASSIC_REPOSITORY_URL
                262 -> EXACT_262_REPOSITORY_URL
                263 -> EXACT_263_REPOSITORY_URL
                else -> null
            }

        internal fun toggledHosts(
            hosts: List<String>,
            repositoryUrl: String,
        ): Pair<List<String>, Boolean> {
            val subscribed = repositoryUrl in hosts
            val retained = hosts.filterNot { it in ALL_REPOSITORY_URLS }.toMutableList()
            if (!subscribed) retained += repositoryUrl
            return retained to !subscribed
        }
    }

    override fun actionPerformed(e: AnActionEvent) {
        val project = e.project ?: return
        val baseline = ApplicationInfo.getInstance().build.baselineVersion
        val repositoryUrl = repositoryUrl(baseline) ?: return
        val settings = UpdateSettings.getInstance()
        val (updated, enabled) = toggledHosts(settings.storedPluginHosts, repositoryUrl)
        settings.storedPluginHosts.clear()
        settings.storedPluginHosts.addAll(updated)
        val state = if (enabled) "enabled" else "disabled"
        TerminalUtil.notifyInfo(
            project,
            "Agent Doc GitHub updates $state for IDE build $baseline. " +
                if (enabled) "Check for plugin updates to use $repositoryUrl" else "The custom repository was removed.",
        )
    }

    override fun update(e: AnActionEvent) {
        val baseline = ApplicationInfo.getInstance().build.baselineVersion
        val repositoryUrl = repositoryUrl(baseline)
        e.presentation.isEnabledAndVisible = repositoryUrl != null && e.project != null
        if (repositoryUrl != null) {
            val subscribed = repositoryUrl in UpdateSettings.getInstance().storedPluginHosts
            e.presentation.text =
                if (subscribed) "Disable Agent Doc GitHub Updates" else "Enable Agent Doc GitHub Updates"
        }
    }
}
