package com.github.btakita.agentdoc

import com.google.gson.Gson
import com.google.gson.JsonElement
import com.google.gson.JsonObject
import com.google.gson.JsonParser
import com.google.gson.annotations.SerializedName
import com.intellij.openapi.diagnostic.Logger
import javax.swing.SwingUtilities

/**
 * GH #326 phase 5b: what a CRDT replica attach failure means for the operator, derived by the
 * hot-reloaded native library.
 *
 * A failed registration records a reason. `CrdtReplicaManager.recordRegisterFailure` and Compact
 * Exchange turn that reason into an operator remedy, the `attach_refused_<token>` ops-log status,
 * whether the hold is surfaced without waiting for an editor action, whether reliable liveness is
 * republished, whether another attach may be scheduled without new evidence, and the notification
 * text. Those rules are the native `attach_failure_diagnosis`
 * derivation now. The plugin sends the reason and the document's file name ([evidence]) and
 * executes the answer itself in the prescribed effect order.
 *
 * Every caller asks off the EDT. [frozen] answers only when no native generation does, and fills
 * an output field a library leaves out. It composes the pre-#326 rules
 * ([CrdtReplicaManager.attachFailureRemedy], [attachFailureStatusToken],
 * [retainedProjectionHoldNeedsOperatorSurfaceUtil], [registerFailureNeedsLivenessRepublishUtil],
 * [retainedProjectionHoldMessageUtil]) and is never extended.
 */
internal object AttachFailureDiagnosis {
    private val LOG = Logger.getInstance(AttachFailureDiagnosis::class.java)
    private val GSON = Gson()

    /**
     * Wire shape of Rust `AttachFailureEvidence`. Gson writes a null field as an absent one, so a
     * failure without an attached replica sends exactly what a generation
     * older than the live-replica check sent.
     */
    internal data class Evidence(
        @SerializedName("reason") val reason: String,
        @SerializedName("file_name") val fileName: String?,
        @SerializedName("live_replica_attached") val liveReplicaAttached: Boolean? = null,
    )

    /** The answer. [holdMessage] is non-null exactly when a file name was asked about. */
    internal data class Diagnosis(
        val remedy: String?,
        val statusToken: String,
        val needsOperatorSurface: Boolean,
        val needsLivenessRepublish: Boolean,
        val retryDisposition: RetryDisposition,
        val holdMessage: String?,
        val holdNotification: HoldNotification = HoldNotification.Error,
    )

    internal enum class RetryDisposition {
        Automatic,
        AwaitOperatorEvidence,
    }

    /**
     * How a surfaced hold reaches the operator. [LogOnly] records the
     * hold (ops-log event, IDE log) without a notification, because an attached replica still owns
     * and syncs the document and only a replacement registration was refused.
     */
    internal enum class HoldNotification {
        Error,
        LogOnly,
    }

    internal fun evidence(reason: String, fileName: String?, liveReplicaAttached: Boolean = false): Evidence =
        Evidence(reason, fileName, liveReplicaAttached.takeIf { it })

    /**
     * Diagnose [reason]. Never on the EDT: every caller asks on a worker. An EDT caller is refused
     * the native call and gets the frozen floor.
     */
    internal fun diagnose(
        reason: String,
        fileName: String? = null,
        liveReplicaAttached: Boolean = false,
        derivation: EditorDerivation = EditorDerivation.Native,
    ): Diagnosis {
        val floor = frozen(reason, fileName, liveReplicaAttached)
        if (SwingUtilities.isEventDispatchThread()) {
            LOG.warn("[crdt-replica] attach_failure_diagnosis asked on the EDT; using the compatibility floor")
            return floor
        }
        val answer =
            derivation.derive(
                "attach_failure_diagnosis",
                GSON.toJson(evidence(reason, fileName, liveReplicaAttached)),
            )
            ?: return floor
        return parseNative(answer, floor, fileName != null) ?: floor
    }

    /**
     * Parse the native output; `null` takes the frozen floor. A field the answer leaves out (an
     * older library) or cannot be read takes the floor's value for that field.
     */
    internal fun parseNative(json: String, floor: Diagnosis, wantsHoldMessage: Boolean): Diagnosis? =
        try {
            val root = JsonParser.parseString(json).asJsonObject
            Diagnosis(
                remedy = if (root.has("remedy")) root.get("remedy").stringOrNull() else floor.remedy,
                statusToken = root.get("status_token").stringOrNull()?.takeIf { it.isNotBlank() } ?: floor.statusToken,
                needsOperatorSurface = root.flag("needs_operator_surface") ?: floor.needsOperatorSurface,
                needsLivenessRepublish = root.flag("needs_liveness_republish") ?: floor.needsLivenessRepublish,
                retryDisposition =
                    root.get("retry_disposition").stringOrNull()?.let(::retryDisposition)
                        ?: floor.retryDisposition,
                holdMessage =
                    if (wantsHoldMessage) {
                        root.get("hold_message").stringOrNull() ?: floor.holdMessage
                    } else {
                        null
                    },
                holdNotification =
                    root.get("hold_notification").stringOrNull()?.let(::holdNotification)
                        ?: floor.holdNotification,
            )
        } catch (e: Exception) {
            LOG.warn("[crdt-replica] native attach_failure_diagnosis unreadable: ${e.message}")
            null
        }

    private fun JsonElement?.stringOrNull(): String? =
        this?.takeIf { it.isJsonPrimitive && it.asJsonPrimitive.isString }?.asString

    private fun JsonObject.flag(name: String): Boolean? =
        get(name)?.takeIf { it.isJsonPrimitive && it.asJsonPrimitive.isBoolean }?.asBoolean

    private fun retryDisposition(token: String): RetryDisposition? = when (token) {
        "automatic" -> RetryDisposition.Automatic
        "await_operator_evidence" -> RetryDisposition.AwaitOperatorEvidence
        else -> null
    }

    private fun holdNotification(token: String): HoldNotification? = when (token) {
        "error" -> HoldNotification.Error
        "log_only" -> HoldNotification.LogOnly
        else -> null
    }

    /**
     * GH #326 compatibility floor: the pre-#326 attach-failure rules, frozen. Used only when no
     * native generation answers. A newer output field may compose those same frozen facts (as the
     * retry disposition does), but reason-specific policy is never added here; change the native
     * derivation instead. [HoldNotification] composes one such fact: with an attached replica the
     * floor's hold is only logged, so a floor answer never tells the operator to detach a healthy
     * replica. Its remedy and message stay frozen.
     */
    internal fun frozen(reason: String, fileName: String?, liveReplicaAttached: Boolean = false): Diagnosis {
        val remedy = CrdtReplicaManager.attachFailureRemedy(reason)
        val needsOperatorSurface = retainedProjectionHoldNeedsOperatorSurfaceUtil(reason)
        return Diagnosis(
            remedy = remedy,
            statusToken = attachFailureStatusToken(reason),
            needsOperatorSurface = needsOperatorSurface,
            needsLivenessRepublish = registerFailureNeedsLivenessRepublishUtil(reason),
            retryDisposition =
                if (needsOperatorSurface) RetryDisposition.AwaitOperatorEvidence
                else RetryDisposition.Automatic,
            holdMessage = fileName?.let { frozenHoldMessage(it, reason) },
            holdNotification = if (liveReplicaAttached) HoldNotification.LogOnly else HoldNotification.Error,
        )
    }

    /** The frozen floor's notification for a hold on [fileName]. */
    internal fun frozenHoldMessage(fileName: String, reason: String): String =
        retainedProjectionHoldMessageUtil(fileName, reason, CrdtReplicaManager.attachFailureRemedy(reason))
}
