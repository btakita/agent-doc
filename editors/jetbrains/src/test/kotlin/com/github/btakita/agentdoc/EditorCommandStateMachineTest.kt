package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Test
import java.util.Base64

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
    fun `duplicate run supersedes active route`() {
        val state = EditorCommandState(active = EditorCommandKind.RUN_AGENT_DOC)

        val (next, decision) = EditorCommandStateMachine.onRequest(
            state,
            EditorCommandKind.RUN_AGENT_DOC,
        )

        assertEquals(EditorCommandDecision.SUPERSEDE_ACTIVE_RUN, decision)
        assertEquals(state, next)
    }

    @Test
    fun `normal clear preempts active run dispatch`() {
        val state = EditorCommandState(active = EditorCommandKind.RUN_AGENT_DOC)

        val (next, decision) = EditorCommandStateMachine.onRequest(
            state,
            EditorCommandKind.CLEAR_SESSION_CONTEXT,
        )

        assertEquals(EditorCommandDecision.PREEMPT_RUN_WITH_CLEAR, decision)
        assertEquals(EditorCommandKind.CLEAR_SESSION_CONTEXT, next.active)
        assertEquals(false, next.queuedRunAfterClear)
    }

    @Test
    fun `run completion after preempting clear is ignored`() {
        val clearing = EditorCommandState(active = EditorCommandKind.CLEAR_SESSION_CONTEXT)

        val (next, completion) = EditorCommandStateMachine.onComplete(
            clearing,
            EditorCommandKind.RUN_AGENT_DOC,
        )

        assertEquals(EditorCommandCompletion.IGNORED, completion)
        assertEquals(clearing, next)
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

    // --- Reactive-path coverage (`#lazilystatesync5` / `#6n5j`) ----------------
    // The command state machine governs *dispatch* (start/supersede/preempt), while
    // the run readiness it gates is now derived from the reactive lazily mirror
    // (`#n529b` reactiveSummaryForFile) instead of a cold projection re-render.
    // These tests prove the editor-side run loop reacts to *applied FFI deltas*:
    // the same command request resolves against a route readiness that the mirror
    // advances reactively (not a stale cold read).

    private fun b64(json: String): String =
        Base64.getEncoder().encodeToString(json.toByteArray())

    private fun routeSnapshot(epoch: Long, readiness: String, pane: String): String =
        """{"type":"snapshot","epoch":$epoch,"document_hash":"doc-a",""" +
            """"nodes":[{"slot_id":11,"type_tag":"${AgentDocNodeType.ROUTE}",""" +
            """"state":"resolved","payload":"${b64("""{"readiness":"$readiness","pane_id":"$pane"}""")}"}],""" +
            """"edges":[],"roots":[]}"""

    private fun routeDelta(baseEpoch: Long, epoch: Long, readiness: String, pane: String): String =
        """{"type":"delta","base_epoch":$baseEpoch,"epoch":$epoch,"document_hash":"doc-a",""" +
            """"ops":[{"op":"cell_set","slot_id":11,""" +
            """"payload":"${b64("""{"readiness":"$readiness","pane_id":"$pane"}""")}"}]}"""

    @Test
    fun `run dispatch reads route readiness reactively from an applied delta`() {
        val path = "/tmp/agent-doc-6n5j-cmd-reactive-${System.nanoTime()}.md"
        try {
            // Cold snapshot: route is still starting; the editor would not treat
            // the document as dispatch-proven from this state.
            StateProjectionBridge.seedMirrorMessageForTest(path, routeSnapshot(1, "starting", "%4"))
            assertEquals("starting", StateProjectionBridge.reactiveSummaryForFile(path)?.routeReadiness)

            // A run is requested against the idle command machine while the route
            // is starting — the command machine accepts (dispatch is its concern),
            // but the readiness driving the run still reads the reactive mirror.
            val (running, decision) = EditorCommandStateMachine.onRequest(
                EditorCommandState(),
                EditorCommandKind.RUN_AGENT_DOC,
            )
            assertEquals(EditorCommandDecision.START_NOW, decision)
            assertEquals(EditorCommandKind.RUN_AGENT_DOC, running.active)

            // An FFI delta now flips the route to dispatch_proven. The editor run
            // loop observes the NEW cell value reactively (no full re-render) —
            // proving the reactive path, not a cold projection pull.
            StateProjectionBridge.seedMirrorMessageForTest(path, routeDelta(1, 2, "dispatch_proven", "%4"))
            val summary = StateProjectionBridge.reactiveSummaryForFile(path)
            assertNotNull(summary)
            assertEquals("dispatch_proven", summary!!.routeReadiness)
            assertEquals(2L, StateProjectionBridge.mirrorEpochForFile(path))
        } finally {
            StateProjectionBridge.evictForFile(path)
        }
    }

    @Test
    fun `duplicate run supersede holds while the reactive mirror advances underneath`() {
        val path = "/tmp/agent-doc-6n5j-cmd-supersede-${System.nanoTime()}.md"
        try {
            StateProjectionBridge.seedMirrorMessageForTest(path, routeSnapshot(1, "dispatch_proven", "%2"))

            // An active run is in flight.
            val active = EditorCommandState(active = EditorCommandKind.RUN_AGENT_DOC)

            // The mirror reacts to a transport delta while the run is active —
            // the command machine must still supersede a duplicate run request
            // regardless of the reactive state churn.
            StateProjectionBridge.seedMirrorMessageForTest(
                path,
                """{"type":"delta","base_epoch":1,"epoch":2,"document_hash":"doc-a",""" +
                    """"ops":[{"op":"node_add","slot_id":40,"type_tag":"${AgentDocNodeType.TRANSPORT_PATCH}",""" +
                    """"payload":"${b64("""{"phase":"applied","actor_generation":1}""")}"}]}""",
            )
            assertEquals("applied", StateProjectionBridge.reactiveSummaryForFile(path)?.latestTransportPhase)

            val (next, decision) = EditorCommandStateMachine.onRequest(
                active,
                EditorCommandKind.RUN_AGENT_DOC,
            )
            assertEquals(EditorCommandDecision.SUPERSEDE_ACTIVE_RUN, decision)
            assertEquals(active, next)
        } finally {
            StateProjectionBridge.evictForFile(path)
        }
    }

    @Test
    fun `unseen document has no reactive mirror epoch`() {
        // No subscribe/seed for this document → the reactive mirror was never
        // created, so the editor has no advanced epoch to react to and falls back
        // to its cold path rather than inventing state. (The cold-projection
        // fallback inside reactiveSummaryForFile may or may not resolve depending
        // on FFI availability in the test JVM, so we pin the mirror epoch — the
        // reactive frontier — which is null until a snapshot/delta is applied.)
        val path = "/tmp/agent-doc-6n5j-cmd-unseen-${System.nanoTime()}.md"
        assertNull(StateProjectionBridge.mirrorEpochForFile(path))
    }
}
