package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class TurnStateBridgeTest {
    @Test
    fun `presentation shows the running task as one truncated line`() {
        // `#turntasklabel`
        val task = "do [#jbunloadsig]: " + "x".repeat(80)
        val presentation = TurnStateBridge.presentation(
            """{"state":"awaiting_response","turn_in_flight":true,""" +
                """"transition_authority":"project_controller","task":"$task"}""",
        )
        val parts = presentation.label.split(" · ")
        assertEquals("⟳ agent-doc: awaiting response", parts[0])
        assertEquals(60, parts[1].codePointCount(0, parts[1].length))
        assertTrue(parts[1].startsWith("do [#jbunloadsig]: x"))
        assertTrue(parts[1].endsWith("…"))
        assertEquals("Task: $task", presentation.tooltip)
        assertTrue(presentation.showBanner)

        val short = TurnStateBridge.presentation(
            """{"state":"awaiting_response","turn_in_flight":true,""" +
                """"transition_authority":"project_controller","task":"do [#a]"}""",
        )
        assertEquals("⟳ agent-doc: awaiting response · do [#a]", short.label)

        val idle = TurnStateBridge.presentation(
            """{"state":"idle","turn_in_flight":false,"transition_authority":"project_controller"}""",
        )
        assertEquals("", idle.label)
    }

    @Test
    fun `presentation projects realtime steering onto in-flight label`() {
        val presentation = TurnStateBridge.presentation(
            """
                {
                  "state":"awaiting_response",
                  "turn_in_flight":true,
                  "transition_authority":"project_controller",
                "realtime_steering":{
                  "state":"prompt_deleted",
                  "count":2,
                  "preview":"removed prompt",
                  "verbatim":"first removal\n\nsecond removal"
                }
                }
            """.trimIndent(),
        )

        assertEquals("⟳ agent-doc: awaiting response · prompt deleted (2 edits)", presentation.label)
        assertEquals("first removal\n\nsecond removal", presentation.tooltip)
        assertTrue(presentation.guardPromptForwarding)
    }

    @Test
    fun `presentation projects merge conflict without requesting another turn`() {
        val presentation =
            TurnStateBridge.presentation(
                """
                    {
                      "state":"idle",
                      "turn_in_flight":false,
                      "transition_authority":"project_controller",
                      "semantic_merge_conflicts":[{
                        "component":"exchange",
                        "id":"node-1",
                        "reason":"same_node_operator_override",
                        "detail":"operator value won"
                      }]
                    }
                """.trimIndent(),
            )

        assertEquals("agent-doc: ⚠ merge conflict", presentation.label)
        assertEquals("exchange:node-1 — operator value won", presentation.tooltip)
        assertFalse(presentation.guardPromptForwarding)
    }

    @Test
    fun `presentation surfaces harness input required`() {
        val presentation =
            TurnStateBridge.presentation(
                """
                    {
                      "state":"awaiting_response",
                      "turn_in_flight":true,
                      "input_required":true,
                      "transition_authority":"project_controller"
                    }
                """.trimIndent(),
            )

        assertEquals("⚠ agent-doc: input required", presentation.label)
        assertTrue(presentation.inputRequired)
        assertTrue(presentation.guardPromptForwarding)
    }

    @Test
    fun `route failure presentation explains start-session pane crash`() {
        val presentation = TurnStateBridge.routeFailurePresentation(
            """
                Error: project controller command start_session failed: refusing start_session cross-document actor pane alias: pane %4 is already claimed by /repo/tasks/professional/sampleportal.md session=62fe1f41 generation=1131 state=ready
            """.trimIndent(),
        )!!

        assertEquals(
            "⚠ agent-doc: start failed: pane %4 was still claimed by generation 1131 (ready)",
            presentation.label,
        )
        assertFalse(presentation.guardPromptForwarding)
        assertFalse(presentation.showBanner)
        assertTrue(presentation.tooltip!!.contains("cross-document actor pane alias"))
    }
}
