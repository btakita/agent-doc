package com.github.btakita.agentdoc

internal enum class EditorCommandKind {
    RUN_AGENT_DOC,
    CLEAR_SESSION_CONTEXT,
}

internal enum class EditorCommandDecision {
    START_NOW,
    DEDUPE_ACTIVE_RUN,
    DEDUPE_ACTIVE_CLEAR,
    QUEUE_RUN_AFTER_CLEAR,
    PREEMPT_RUN_WITH_CLEAR,
    IGNORED,
}

internal enum class EditorCommandCompletion {
    IDLE,
    START_QUEUED_RUN,
    IGNORED,
}

internal data class EditorCommandState(
    val active: EditorCommandKind? = null,
    val queuedRunAfterClear: Boolean = false,
)

internal object EditorCommandStateMachine {
    fun onRequest(
        state: EditorCommandState,
        requested: EditorCommandKind,
    ): Pair<EditorCommandState, EditorCommandDecision> {
        val active = state.active
        if (active == null) {
            return state.copy(active = requested, queuedRunAfterClear = false) to
                EditorCommandDecision.START_NOW
        }
        if (active == requested) {
            return state to when (requested) {
                EditorCommandKind.RUN_AGENT_DOC -> EditorCommandDecision.DEDUPE_ACTIVE_RUN
                EditorCommandKind.CLEAR_SESSION_CONTEXT -> EditorCommandDecision.DEDUPE_ACTIVE_CLEAR
            }
        }
        return when {
            active == EditorCommandKind.CLEAR_SESSION_CONTEXT &&
                requested == EditorCommandKind.RUN_AGENT_DOC ->
                state.copy(queuedRunAfterClear = true) to EditorCommandDecision.QUEUE_RUN_AFTER_CLEAR
            active == EditorCommandKind.RUN_AGENT_DOC &&
                requested == EditorCommandKind.CLEAR_SESSION_CONTEXT ->
                EditorCommandState(active = EditorCommandKind.CLEAR_SESSION_CONTEXT) to
                    EditorCommandDecision.PREEMPT_RUN_WITH_CLEAR
            else -> state to EditorCommandDecision.IGNORED
        }
    }

    fun onComplete(
        state: EditorCommandState,
        completed: EditorCommandKind,
    ): Pair<EditorCommandState, EditorCommandCompletion> {
        if (state.active != completed) {
            return state to EditorCommandCompletion.IGNORED
        }
        if (
            completed == EditorCommandKind.CLEAR_SESSION_CONTEXT &&
            state.queuedRunAfterClear
        ) {
            return EditorCommandState(active = EditorCommandKind.RUN_AGENT_DOC) to
                EditorCommandCompletion.START_QUEUED_RUN
        }
        return EditorCommandState() to EditorCommandCompletion.IDLE
    }
}

internal class EditorCommandRegistry {
    private val states = mutableMapOf<String, EditorCommandState>()

    @Synchronized
    fun request(routeKey: String, requested: EditorCommandKind): EditorCommandDecision {
        val state = states[routeKey] ?: EditorCommandState()
        val (next, decision) = EditorCommandStateMachine.onRequest(state, requested)
        if (next.active == null) {
            states.remove(routeKey)
        } else {
            states[routeKey] = next
        }
        return decision
    }

    @Synchronized
    fun complete(routeKey: String, completed: EditorCommandKind): EditorCommandCompletion {
        val state = states[routeKey] ?: return EditorCommandCompletion.IGNORED
        val (next, completion) = EditorCommandStateMachine.onComplete(state, completed)
        if (next.active == null) {
            states.remove(routeKey)
        } else {
            states[routeKey] = next
        }
        return completion
    }

    @Synchronized
    fun resetForTest() {
        states.clear()
    }
}
