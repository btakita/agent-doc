package com.github.btakita.agentdoc

import com.google.gson.JsonObject
import com.google.gson.JsonParser

/**
 * How `Run Agent Doc` and `Sync Tmux Panes` name the editor layout they act on
 * (`specs/13-state-backbone.md` "State Propagation, Not Effect Inference").
 *
 * The editor surface is already a Source on the controller: [EditorTabSyncListener] publishes it
 * on every layout and selection change, and the controller derives the automatic pane layout from
 * it. These commands used to read the editor split again at click time and send it as
 * `--col a,b --col c`, a per-command snapshot beside that Source. Now a command publishes the
 * current surface on the same path, and when the controller's reply advertises
 * [SURFACE_LAYOUT_REFERENCE_CAPABILITY] it names that publication instead
 * ([SurfaceLayoutReference]); the controller derives the layout from its Source exactly as for
 * automatic capture.
 *
 * The `--col` snapshot remains only as the rollout fallback for a controller that predates the
 * capability, and for a surface this listener cannot publish to the command's controller (a
 * cross-root route, a frontend-owned Remote Dev surface, or a capture the editor deferred). Each
 * fallback is logged with its reason.
 */
internal object SurfaceCommandReference {
    const val SURFACE_LAYOUT_REFERENCE_CAPABILITY = "surface_layout_reference_v1"

    sealed interface Decision {
        /** Act by reference to the publication the controller just retained. */
        data class Reference(val reference: SurfaceLayoutReference) : Decision

        /** Send the explicit `--col` snapshot, and log [reason]. */
        data class SnapshotFallback(val reason: String) : Decision
    }

    /** Whether an `editor_surface_observe` reply advertises the layout-reference capability. */
    fun advertisesLayoutReference(receipt: JsonObject): Boolean =
        receipt.get("capabilities")
            ?.takeIf { it.isJsonArray }
            ?.asJsonArray
            ?.any { it.isJsonPrimitive && it.asString == SURFACE_LAYOUT_REFERENCE_CAPABILITY } == true

    /**
     * Decide from the publication's outcome. Pure, so the rollout fallback is pinned by tests.
     *
     * - [surfaceRoot] is the controller root the surface was published to; [commandRoot] is the
     *   root the command goes to. A reference is only meaningful to the controller that holds the
     *   Source, so a cross-root command keeps the snapshot.
     * - A rejected or failed publication was not retained, so there is nothing to name.
     * - A reply without the capability is an older controller: keep the snapshot so a new plugin
     *   still works against an old binary.
     */
    fun decide(
        surfaceRoot: String,
        commandRoot: String,
        surfaceId: String,
        publishExitCode: Int,
        receiptJson: String,
        publishedSequence: Long?,
    ): Decision {
        if (surfaceRoot != commandRoot) {
            return Decision.SnapshotFallback("cross_root surface_root=$surfaceRoot command_root=$commandRoot")
        }
        if (publishExitCode != 0) {
            return Decision.SnapshotFallback("surface_publish_failed")
        }
        val receipt =
            runCatching { JsonParser.parseString(receiptJson).asJsonObject }.getOrNull()
                ?: return Decision.SnapshotFallback("surface_receipt_unreadable")
        val rejected = jsonStringFieldOrNull(receipt, "rejected")
        if (rejected != null) {
            return Decision.SnapshotFallback("surface_publication_rejected=$rejected")
        }
        if (!advertisesLayoutReference(receipt)) {
            return Decision.SnapshotFallback("controller_lacks_$SURFACE_LAYOUT_REFERENCE_CAPABILITY")
        }
        val sequence = publishedSequence ?: return Decision.SnapshotFallback("surface_sequence_unknown")
        return Decision.Reference(SurfaceLayoutReference(surfaceId, sequence))
    }

    /**
     * The controller refuses a reference whose surface cannot carry the command's layout with a
     * `surface_reference_<reason>: <sentence>` diagnostic. The two layout refusals keep the exact
     * sentences the plugin showed when it made these decisions itself.
     */
    fun refusalMessage(output: String): String? =
        when {
            output.contains("surface_reference_unknown_layout") ->
                "Editor split could not be read; the retained tmux layout was not changed."
            output.contains("surface_reference_stale_focus") ->
                "Editor focus changed while the split layout was being read; retry after the selection settles."
            else -> null
        }
}
