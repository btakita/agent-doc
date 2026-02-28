package com.github.btakita.agentdoc

import com.intellij.openapi.actionSystem.ActionPromoter
import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.CommonDataKeys
import com.intellij.openapi.actionSystem.DataContext

/**
 * Promotes AgentDocPopupAction to the front of the action list for .md files
 * so it wins Alt+Enter over ShowIntentionActions.
 */
class AgentDocActionPromoter : ActionPromoter {

    override fun promote(
        actions: MutableList<out AnAction>,
        context: DataContext
    ): List<AnAction>? {
        val file = context.getData(CommonDataKeys.VIRTUAL_FILE) ?: return null
        if (file.extension?.lowercase() != "md") return null

        val agentDocAction = actions.filterIsInstance<AgentDocPopupAction>()
        if (agentDocAction.isEmpty()) return null

        // Put AgentDocPopupAction first, then everything else
        return agentDocAction + actions.filter { it !is AgentDocPopupAction }
    }
}
