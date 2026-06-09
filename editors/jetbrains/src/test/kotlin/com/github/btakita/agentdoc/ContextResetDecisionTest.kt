package com.github.btakita.agentdoc

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ContextResetDecisionTest {
    @Test
    fun `clears when opted in and usage at or above threshold`() {
        assertTrue(ContextResetDecision.shouldClearBeforeDispatch(50, 50, optIn = true))
        assertTrue(ContextResetDecision.shouldClearBeforeDispatch(80, 50, optIn = true))
    }

    @Test
    fun `does not clear below threshold`() {
        assertFalse(ContextResetDecision.shouldClearBeforeDispatch(49, 50, optIn = true))
    }

    @Test
    fun `does not clear when opt-in disabled`() {
        assertFalse(ContextResetDecision.shouldClearBeforeDispatch(90, 50, optIn = false))
    }

    @Test
    fun `unknown usage never clears`() {
        assertFalse(ContextResetDecision.shouldClearBeforeDispatch(null, 50, optIn = true))
    }

    @Test
    fun `threshold of zero disables the gate`() {
        assertFalse(ContextResetDecision.shouldClearBeforeDispatch(100, 0, optIn = true))
    }
}
