package com.github.btakita.agentdoc

import com.intellij.openapi.actionSystem.ActionPromoter
import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.CommonDataKeys
import com.intellij.openapi.actionSystem.DataContext

/**
 * Promotes AgentDocPopupAction over the built-in ShowIntentionActions
 * when Alt+Enter is pressed in a .md file. This prevents a disambiguation
 * dialog when both actions are bound to the same shortcut.
 */
class AgentDocActionPromoter : ActionPromoter {
    override fun promote(
        actions: List<AnAction>,
        context: DataContext
    ): List<AnAction> {
        val file = CommonDataKeys.VIRTUAL_FILE.getData(context)
        if (file?.extension?.lowercase() == "md") {
            return actions.sortedByDescending { it is AgentDocPopupAction }
        }
        return emptyList()
    }
}
