package com.github.btakita.agentdoc.split.frontend

import com.intellij.openapi.actionSystem.ActionUpdateThread
import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.application.ApplicationInfo
import com.intellij.openapi.project.DumbAware
import com.intellij.openapi.ui.Messages
import com.intellij.openapi.updateSettings.impl.UpdateSettings

/**
 * Configures the GitHub update feed in the process that owns the JetBrains Client UI.
 *
 * [UpdateSettings] is process-local in Remote Development. Registering the classic action in the
 * backend content module changed only the backend repository list, leaving an older frontend
 * generation loaded and causing JetBrains' paired-plugin version fence to disable the bridge.
 */
class FrontendAgentDocUpdateChannelAction : AnAction(), DumbAware {
    override fun actionPerformed(e: AnActionEvent) {
        val project = e.project ?: return
        val baseline = ApplicationInfo.getInstance().build.baselineVersion
        val repositoryUrl = FrontendUpdateChannelPolicy.repositoryUrl(baseline) ?: return
        val settings = UpdateSettings.getInstance()
        val (updated, enabled) =
            FrontendUpdateChannelPolicy.toggledHosts(settings.storedPluginHosts, repositoryUrl)
        settings.storedPluginHosts.clear()
        settings.storedPluginHosts.addAll(updated)
        val state = if (enabled) "enabled" else "disabled"
        Messages.showInfoMessage(
            project,
            "Agent Doc GitHub updates $state for this JetBrains Client (IDE build $baseline). " +
                if (enabled) {
                    "Check for plugin updates, install the matching update on the Remote Dev backend, " +
                        "then restart both processes."
                } else {
                    "The custom repository was removed from this JetBrains Client."
                },
            "Agent Doc Update Channel",
        )
    }

    override fun update(e: AnActionEvent) {
        val baseline = ApplicationInfo.getInstance().build.baselineVersion
        val repositoryUrl = FrontendUpdateChannelPolicy.repositoryUrl(baseline)
        e.presentation.isEnabledAndVisible = repositoryUrl != null && e.project != null
        if (repositoryUrl != null) {
            val subscribed = repositoryUrl in UpdateSettings.getInstance().storedPluginHosts
            e.presentation.text =
                if (subscribed) "Disable Agent Doc GitHub Updates" else "Enable Agent Doc GitHub Updates"
        }
    }

    override fun getActionUpdateThread(): ActionUpdateThread = ActionUpdateThread.BGT
}

internal object FrontendUpdateChannelPolicy {
    const val REPOSITORY_PREFIX: String =
        "https://github.com/btakita/agent-doc/releases/latest/download/agent-doc-jetbrains-"
    const val CLASSIC_REPOSITORY_URL: String = "${REPOSITORY_PREFIX}classic.xml"
    const val EXACT_262_REPOSITORY_URL: String = "${REPOSITORY_PREFIX}262.xml"
    const val EXACT_263_REPOSITORY_URL: String = "${REPOSITORY_PREFIX}263.xml"
    val ALL_REPOSITORY_URLS: Set<String> =
        setOf(CLASSIC_REPOSITORY_URL, EXACT_262_REPOSITORY_URL, EXACT_263_REPOSITORY_URL)

    fun repositoryUrl(baselineVersion: Int): String? =
        when (baselineVersion) {
            in 242..261 -> CLASSIC_REPOSITORY_URL
            262 -> EXACT_262_REPOSITORY_URL
            263 -> EXACT_263_REPOSITORY_URL
            else -> null
        }

    fun toggledHosts(hosts: List<String>, repositoryUrl: String): Pair<List<String>, Boolean> {
        val subscribed = repositoryUrl in hosts
        val retained = hosts.filterNot { it in ALL_REPOSITORY_URLS }.toMutableList()
        if (!subscribed) retained += repositoryUrl
        return retained to !subscribed
    }
}
