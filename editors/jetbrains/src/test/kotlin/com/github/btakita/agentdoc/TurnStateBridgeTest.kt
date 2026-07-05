package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
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
                  "transition_authority":"cpc",
                  "realtime_steering":{
                    "state":"prompt_deleted",
                    "preview":"removed prompt"
                  }
                }
            """.trimIndent(),
        )

        assertEquals("⟳ agent-doc: awaiting response · prompt deleted", presentation.label)
        assertTrue(presentation.guardPromptForwarding)
    }
}
