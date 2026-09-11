package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * `#jbfocusnoop`: "Focus Agent Terminal" did not navigate to anything.
 *
 * `IdeTerminalHost.focusExisting` returned `Unit` and `return`ed early on every
 * miss — no tool window, no `agent-doc` tab, a widget that was not a
 * `ShellTerminalWidget`, a null tty connector, or a disconnected one. Clicking the
 * notification action in any of those states did nothing at all and said nothing,
 * which is exactly what the operator reported.
 *
 * Focus now always resolves to an outcome the caller can act on, and the two
 * liveness conditions were dropped from the focus path entirely: a tab whose shell
 * died is still the thing the operator asked to look at. A connected tty remains a
 * precondition for `attachExisting`, which executes a command in the tab.
 */
class IdeTerminalFocusOutcomeTest {
    @Test
    fun `an existing agent-doc tab is focused`() {
        assertEquals(
            TerminalFocusOutcome.AGENT_TAB,
            IdeTerminalHost.decideTerminalFocus(hasToolWindow = true, hasAgentTab = true),
        )
    }

    @Test
    fun `a terminal tool window without an agent-doc tab is still activated`() {
        // The pre-fix behaviour here was a silent no-op. Landing the operator in
        // the terminal shows them the tab is missing, which is actionable.
        assertEquals(
            TerminalFocusOutcome.TOOL_WINDOW_ONLY,
            IdeTerminalHost.decideTerminalFocus(hasToolWindow = true, hasAgentTab = false),
        )
    }

    @Test
    fun `no terminal tool window is reported, never silently swallowed`() {
        assertEquals(
            TerminalFocusOutcome.NOTHING,
            IdeTerminalHost.decideTerminalFocus(hasToolWindow = false, hasAgentTab = false),
        )
    }

    @Test
    fun `every state resolves to an outcome so focus can never vanish`() {
        val outcomes = listOf(true, false).flatMap { hasToolWindow ->
            listOf(true, false).map { hasAgentTab ->
                IdeTerminalHost.decideTerminalFocus(hasToolWindow, hasAgentTab)
            }
        }
        assertEquals(4, outcomes.size)
        assertEquals(
            listOf(
                TerminalFocusOutcome.AGENT_TAB,
                TerminalFocusOutcome.TOOL_WINDOW_ONLY,
                TerminalFocusOutcome.NOTHING,
                TerminalFocusOutcome.NOTHING,
            ),
            outcomes,
        )
    }
}
