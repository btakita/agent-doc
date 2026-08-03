package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class TurnStateBridgeTest {
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
