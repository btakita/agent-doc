package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Test

class PluginLifecycleListenerTest {

    @Test
    fun `startup resync command stays non-destructive`() {
        val cmd = PluginLifecycleListener().buildStartupResyncCommand("0")

        assertEquals(listOf("agent-doc", "resync"), cmd)
        assertFalse(cmd.contains("--fix"))
    }
}
