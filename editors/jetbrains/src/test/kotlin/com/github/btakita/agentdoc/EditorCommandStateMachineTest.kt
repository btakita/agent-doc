package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Test

class EditorCommandStateMachineTest {
    @Test
    fun `idle document starts requested command`() {
        val (state, decision) = EditorCommandStateMachine.onRequest(
            EditorCommandState(),
            EditorCommandKind.RUN_AGENT_DOC,
        )

        assertEquals(EditorCommandDecision.START_NOW, decision)
        assertEquals(EditorCommandKind.RUN_AGENT_DOC, state.active)
    }

    @Test
    fun `duplicate run is deduped while route is active`() {
        val state = EditorCommandState(active = EditorCommandKind.RUN_AGENT_DOC)

        val (next, decision) = EditorCommandStateMachine.onRequest(
            state,
            EditorCommandKind.RUN_AGENT_DOC,
        )

        assertEquals(EditorCommandDecision.DEDUPE_ACTIVE_RUN, decision)
        assertEquals(state, next)
    }

    @Test
    fun `normal clear is rejected while run dispatch is active`() {
        val state = EditorCommandState(active = EditorCommandKind.RUN_AGENT_DOC)

        val (next, decision) = EditorCommandStateMachine.onRequest(
            state,
            EditorCommandKind.CLEAR_SESSION_CONTEXT,
        )

        assertEquals(EditorCommandDecision.REJECT_CLEAR_DURING_RUN, decision)
        assertEquals(state, next)
    }

    @Test
    fun `run queues behind active clear and starts after clear completes`() {
        val clearing = EditorCommandState(active = EditorCommandKind.CLEAR_SESSION_CONTEXT)
        val (queued, requestDecision) = EditorCommandStateMachine.onRequest(
            clearing,
            EditorCommandKind.RUN_AGENT_DOC,
        )

        assertEquals(EditorCommandDecision.QUEUE_RUN_AFTER_CLEAR, requestDecision)
        assertEquals(
            EditorCommandState(
                active = EditorCommandKind.CLEAR_SESSION_CONTEXT,
                queuedRunAfterClear = true,
            ),
            queued,
        )

        val (running, completion) = EditorCommandStateMachine.onComplete(
            queued,
            EditorCommandKind.CLEAR_SESSION_CONTEXT,
        )

        assertEquals(EditorCommandCompletion.START_QUEUED_RUN, completion)
        assertEquals(EditorCommandState(active = EditorCommandKind.RUN_AGENT_DOC), running)
    }

    @Test
    fun `wrong completion does not mutate active state`() {
        val state = EditorCommandState(active = EditorCommandKind.RUN_AGENT_DOC)

        val (next, completion) = EditorCommandStateMachine.onComplete(
            state,
            EditorCommandKind.CLEAR_SESSION_CONTEXT,
        )

        assertEquals(EditorCommandCompletion.IGNORED, completion)
        assertEquals(state, next)
    }
}
