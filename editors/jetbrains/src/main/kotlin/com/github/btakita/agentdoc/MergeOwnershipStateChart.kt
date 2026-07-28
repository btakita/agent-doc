package com.github.btakita.agentdoc

import io.github.lazily.ChartBuilder
import io.github.lazily.StateBuilder
import io.github.lazily.ThreadSafeContext
import io.github.lazily.ThreadSafeStateChart

/**
 * JetBrains-side projection of the binary-owned merge-ownership state machine.
 *
 * The controller remains authoritative for disk-write permission. This chart
 * gives the plugin one thread-safe lifecycle fact instead of an independently
 * mutated `attached` flag while preserving the transition vocabulary used by
 * `agent_doc_merge::ownership`.
 */
internal enum class MergeOwnershipPhase(val stateId: String) {
    Detached("detached"),
    Attached("attached"),
    EditorOwnsBuffer("editor_owns_buffer"),
    BinaryWriteRequested("binary_write_requested"),
    LazilyPatchAppliedProven("lazily_patch_applied_proven"),
    Committed("committed"),
}

internal enum class MergeOwnershipEvent(val eventId: String) {
    EditorAttached("editor_attached"),
    EditorBufferObserved("editor_buffer_observed"),
    EditorDetached("editor_detached"),
    HeartbeatStale("heartbeat_stale"),
    BinaryWriteRequested("binary_write_requested"),
    LazilyPatchAppliedObserved("lazily_patch_applied_observed"),
    Committed("committed"),
}

internal class MergeOwnershipStateChart(context: ThreadSafeContext) {
    private val chart = ThreadSafeStateChart(DEFINITION, context)

    val phase: MergeOwnershipPhase
        get() {
            val activeLeaf = chart.activeLeaves().single()
            return MergeOwnershipPhase.entries.single { it.stateId == activeLeaf }
        }

    val editorAttached: Boolean
        get() =
            when (phase) {
                MergeOwnershipPhase.Detached,
                MergeOwnershipPhase.Committed,
                -> false

                MergeOwnershipPhase.Attached,
                MergeOwnershipPhase.EditorOwnsBuffer,
                MergeOwnershipPhase.BinaryWriteRequested,
                MergeOwnershipPhase.LazilyPatchAppliedProven,
                -> true
            }

    fun send(event: MergeOwnershipEvent): Boolean = chart.send(event.eventId)

    private companion object {
        const val ROOT = "merge_ownership"

        val DEFINITION =
            ChartBuilder()
                .state(StateBuilder.compound(ROOT, MergeOwnershipPhase.Detached.stateId))
                .state(
                    StateBuilder.atomic(MergeOwnershipPhase.Detached.stateId)
                        .parent(ROOT)
                        .on(
                            MergeOwnershipEvent.EditorAttached.eventId,
                            MergeOwnershipPhase.Attached.stateId,
                        )
                        .on(
                            MergeOwnershipEvent.EditorBufferObserved.eventId,
                            MergeOwnershipPhase.EditorOwnsBuffer.stateId,
                        )
                        .on(
                            MergeOwnershipEvent.EditorDetached.eventId,
                            MergeOwnershipPhase.Detached.stateId,
                        )
                        .on(
                            MergeOwnershipEvent.Committed.eventId,
                            MergeOwnershipPhase.Committed.stateId,
                        ),
                )
                .state(
                    StateBuilder.atomic(MergeOwnershipPhase.Attached.stateId)
                        .parent(ROOT)
                        .on(
                            MergeOwnershipEvent.EditorAttached.eventId,
                            MergeOwnershipPhase.Attached.stateId,
                        )
                        .on(
                            MergeOwnershipEvent.EditorBufferObserved.eventId,
                            MergeOwnershipPhase.EditorOwnsBuffer.stateId,
                        )
                        .on(
                            MergeOwnershipEvent.EditorDetached.eventId,
                            MergeOwnershipPhase.Detached.stateId,
                        )
                        .on(
                            MergeOwnershipEvent.HeartbeatStale.eventId,
                            MergeOwnershipPhase.Detached.stateId,
                        ),
                )
                .state(
                    StateBuilder.atomic(MergeOwnershipPhase.EditorOwnsBuffer.stateId)
                        .parent(ROOT)
                        .on(
                            MergeOwnershipEvent.EditorAttached.eventId,
                            MergeOwnershipPhase.Attached.stateId,
                        )
                        .on(
                            MergeOwnershipEvent.EditorBufferObserved.eventId,
                            MergeOwnershipPhase.EditorOwnsBuffer.stateId,
                        )
                        .on(
                            MergeOwnershipEvent.EditorDetached.eventId,
                            MergeOwnershipPhase.Detached.stateId,
                        )
                        .on(
                            MergeOwnershipEvent.BinaryWriteRequested.eventId,
                            MergeOwnershipPhase.BinaryWriteRequested.stateId,
                        ),
                )
                .state(
                    StateBuilder.atomic(MergeOwnershipPhase.BinaryWriteRequested.stateId)
                        .parent(ROOT)
                        .on(
                            MergeOwnershipEvent.BinaryWriteRequested.eventId,
                            MergeOwnershipPhase.BinaryWriteRequested.stateId,
                        )
                        .on(
                            MergeOwnershipEvent.LazilyPatchAppliedObserved.eventId,
                            MergeOwnershipPhase.LazilyPatchAppliedProven.stateId,
                        ),
                )
                .state(
                    StateBuilder.atomic(MergeOwnershipPhase.LazilyPatchAppliedProven.stateId)
                        .parent(ROOT)
                        .on(
                            MergeOwnershipEvent.LazilyPatchAppliedObserved.eventId,
                            MergeOwnershipPhase.LazilyPatchAppliedProven.stateId,
                        )
                        .on(
                            MergeOwnershipEvent.Committed.eventId,
                            MergeOwnershipPhase.Committed.stateId,
                        ),
                )
                .state(
                    StateBuilder.final(MergeOwnershipPhase.Committed.stateId)
                        .parent(ROOT)
                        .on(
                            MergeOwnershipEvent.Committed.eventId,
                            MergeOwnershipPhase.Committed.stateId,
                        ),
                )
                .build()
    }
}
