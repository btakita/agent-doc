package com.github.btakita.agentdoc

import com.google.gson.Gson
import com.google.gson.JsonParser
import com.google.gson.annotations.SerializedName
import com.intellij.openapi.diagnostic.Logger
import javax.swing.SwingUtilities

/**
 * GH #326 phase 5: the CRDT replica registration direction, derived by the hot-reloaded native
 * library.
 *
 * `CrdtReplicaManager.forwarderFor` registers a candidate endpoint, then must choose how the three
 * text generations it holds meet: adopt the controller canonical, publish the live buffer from its
 * settled shadow, merge it forward, defer to a captured local delta, or hold the buffer exactly as
 * the operator left it. That choice was the most frequently changed pure decision in
 * `CrdtReplicaManager`, so it is the native `registration_projection` derivation now.
 *
 * The plugin compares the texts itself, over the exact UTF-16 text it holds, and reports only
 * which are present and which are equal ([evidence]). No document crosses the boundary and no
 * encoding can make two different buffers compare equal. The containment proof and the clean merge
 * are already native calls; their answers are facts too. The derivation decides only *which*
 * action; executing it and the order of its effects stay in `forwarderFor`.
 *
 * [frozen] answers only when no native generation does, or when the answer names an action this
 * plugin cannot execute. It composes the pre-#326 rules
 * ([retainedRegistrationProjectionActionForAttachUtil], [retainedRegistrationHoldReasonUtil], and
 * the re-register hold reason) and is never extended.
 */
internal object RegistrationProjection {
    private val LOG = Logger.getInstance(RegistrationProjection::class.java)
    private val GSON = Gson()

    /** The hold reason of an ordinary re-register over text typed after the settled shadow. */
    internal const val AMBIGUOUS_REREGISTER_PROJECTION = "ambiguous-reregister-projection"

    /** What `forwarderFor` holds when it decides. Never crosses the native boundary. */
    internal data class Observation(
        val deferCanonicalProjectionForPendingLocal: Boolean,
        val canonicalProjectionRetained: Boolean,
        val retainedReplicaReseedPending: Boolean,
        val canonicalCoversRetainedFrontier: Boolean?,
        val canonicalSupersedesRetainedFrontier: Boolean = false,
        val publishedShadow: String?,
        val bufferText: String?,
        val canonicalText: String?,
        val canonicalContainsOperatorEdits: Boolean?,
        val cleanMergeAvailable: Boolean?,
    )

    /**
     * Wire shape of Rust `RegistrationProjectionEvidence`: presence and equality facts, never text.
     * Gson writes a null optional fact as an absent field, which the derivation reads as unknown.
     */
    internal data class Evidence(
        @SerializedName("defer_canonical_projection_for_pending_local")
        val deferCanonicalProjectionForPendingLocal: Boolean,
        @SerializedName("canonical_projection_retained") val canonicalProjectionRetained: Boolean,
        @SerializedName("retained_replica_reseed_pending") val retainedReplicaReseedPending: Boolean,
        @SerializedName("canonical_covers_retained_frontier") val canonicalCoversRetainedFrontier: Boolean?,
        @SerializedName("canonical_supersedes_retained_frontier") val canonicalSupersedesRetainedFrontier: Boolean?,
        @SerializedName("has_published_shadow") val hasPublishedShadow: Boolean,
        @SerializedName("has_buffer") val hasBuffer: Boolean,
        @SerializedName("has_canonical") val hasCanonical: Boolean,
        @SerializedName("buffer_equals_canonical") val bufferEqualsCanonical: Boolean,
        @SerializedName("buffer_equals_shadow") val bufferEqualsShadow: Boolean,
        @SerializedName("canonical_equals_shadow") val canonicalEqualsShadow: Boolean,
        @SerializedName("canonical_contains_operator_edits") val canonicalContainsOperatorEdits: Boolean?,
        @SerializedName("clean_merge_available") val cleanMergeAvailable: Boolean?,
    )

    /** The answer. [holdReason] is non-null exactly when [action] is a hold. */
    internal data class Decision(
        val action: RetainedRegistrationProjectionAction,
        val holdReason: String?,
    )

    /** The facts of [observation], compared over its exact text. */
    internal fun evidence(observation: Observation): Evidence {
        val shadow = observation.publishedShadow
        val buffer = observation.bufferText
        val canonical = observation.canonicalText
        return Evidence(
            deferCanonicalProjectionForPendingLocal = observation.deferCanonicalProjectionForPendingLocal,
            canonicalProjectionRetained = observation.canonicalProjectionRetained,
            retainedReplicaReseedPending = observation.retainedReplicaReseedPending,
            canonicalCoversRetainedFrontier = observation.canonicalCoversRetainedFrontier,
            // Omit false for compatibility with older native generations and
            // the frozen exhaustive fixture; absence remains fail-closed.
            canonicalSupersedesRetainedFrontier =
                observation.canonicalSupersedesRetainedFrontier.takeIf { it },
            hasPublishedShadow = shadow != null,
            hasBuffer = buffer != null,
            hasCanonical = canonical != null,
            bufferEqualsCanonical = buffer != null && buffer == canonical,
            bufferEqualsShadow = buffer != null && buffer == shadow,
            canonicalEqualsShadow = canonical != null && canonical == shadow,
            canonicalContainsOperatorEdits = observation.canonicalContainsOperatorEdits,
            cleanMergeAvailable = observation.cleanMergeAvailable,
        )
    }

    /**
     * Decide [observation]. Never on the EDT: `forwarderFor` runs on the replica workers, beside
     * the socket registration it decides for. An EDT caller is refused the native call and gets the
     * frozen floor.
     */
    internal fun decide(
        observation: Observation,
        derivation: EditorDerivation = EditorDerivation.Native,
    ): Decision {
        val native =
            if (SwingUtilities.isEventDispatchThread()) {
                LOG.warn("[crdt-replica] registration_projection asked on the EDT; using the compatibility floor")
                null
            } else {
                derivation.derive("registration_projection", GSON.toJson(evidence(observation)))
                    ?.let { parseNative(it, observation) }
            }
        return native ?: frozen(observation)
    }

    /** Parse the native output; `null` takes the frozen floor. */
    internal fun parseNative(json: String, observation: Observation): Decision? =
        try {
            val root = JsonParser.parseString(json).asJsonObject
            val token = root.get("action")?.takeIf { !it.isJsonNull }?.asString
            val action =
                when (token) {
                    "apply_canonical" -> RetainedRegistrationProjectionAction.ApplyCanonical
                    "publish_operator_buffer" -> RetainedRegistrationProjectionAction.PublishOperatorBuffer
                    "hold_operator_buffer" -> RetainedRegistrationProjectionAction.HoldOperatorBuffer
                    "defer_canonical_projection" -> RetainedRegistrationProjectionAction.DeferCanonicalProjection
                    "merge_forward" -> RetainedRegistrationProjectionAction.MergeForward
                    else -> null
                }
            if (action == null) {
                // A newer library may name an action this generation cannot execute.
                LOG.warn("[crdt-replica] native registration_projection action unknown to this plugin: $token")
                null
            } else {
                val holdReason =
                    if (action == RetainedRegistrationProjectionAction.HoldOperatorBuffer) {
                        root.get("hold_reason")?.takeIf { !it.isJsonNull }?.asString?.takeIf { it.isNotBlank() }
                            ?: frozenHoldReason(observation)
                    } else {
                        null
                    }
                Decision(action, holdReason)
            }
        } catch (e: Exception) {
            LOG.warn("[crdt-replica] native registration_projection unreadable: ${e.message}")
            null
        }

    /**
     * GH #326 compatibility floor: the pre-#326 registration rules, frozen. Used only when no native
     * generation answers. Never extend it; change the native derivation instead.
     */
    internal fun frozen(observation: Observation): Decision {
        val action =
            retainedRegistrationProjectionActionForAttachUtil(
                deferCanonicalProjectionForPendingLocal = observation.deferCanonicalProjectionForPendingLocal,
                canonicalProjectionRetained = observation.canonicalProjectionRetained,
                retainedReplicaReseedPending = observation.retainedReplicaReseedPending,
                canonicalCoversRetainedFrontier = observation.canonicalCoversRetainedFrontier,
                publishedShadow = observation.publishedShadow,
                bufferText = observation.bufferText,
                canonicalText = observation.canonicalText,
                canonicalContainsOperatorEdits = observation.canonicalContainsOperatorEdits,
                cleanMergeAvailable = observation.cleanMergeAvailable,
            )
        val holdReason =
            if (action == RetainedRegistrationProjectionAction.HoldOperatorBuffer) {
                frozenHoldReason(observation)
            } else {
                null
            }
        return Decision(action, holdReason)
    }

    /** The pre-#326 hold reasons: the retained/reseed reason, else the re-register reason. */
    private fun frozenHoldReason(observation: Observation): String =
        if (observation.canonicalProjectionRetained || observation.retainedReplicaReseedPending) {
            retainedRegistrationHoldReasonUtil(
                retainedReplicaReseedPending = observation.retainedReplicaReseedPending,
                publishedShadow = observation.publishedShadow,
            )
        } else {
            AMBIGUOUS_REREGISTER_PROJECTION
        }
}
