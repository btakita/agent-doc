package com.github.btakita.agentdoc

import com.google.gson.Gson
import com.google.gson.annotations.SerializedName
import com.intellij.openapi.diagnostic.Logger

/**
 * GH #326 phase 5c: the text shaping a CRDT replica registration feeds the native containment
 * proof and clean merge, folded into those native calls.
 *
 * `CrdtReplicaManager.forwarderFor` holds three generations: the settled shadow, the live buffer,
 * and the controller canonical. Before phase 5c the plugin shaped them itself before asking
 * `agent_doc_captured_splices_contained` / `agent_doc_rebase_captured_splices`: it removed the
 * binary-owned markers ([withoutBinaryOwnedMarkersUtil]) for the containment proof and built the
 * one splice from shadow to buffer ([singleSpliceBatchUtil]). Those rules changed only with a
 * plugin generation, which on the exact-263 modular package means an IDE restart.
 *
 * Now the plugin sends the raw shadow and canonical as before, plus the optional `registration`
 * input (`{"registration": {"buffer": ..., "operatorText": ...}}`) in place of a batch. Native
 * removes the markers when asked for operator text and shapes the splice, so a change to either
 * rule ships with a library hot reload. The order of side effects is unchanged: the same native
 * call at the same point, on the same replica worker.
 *
 * The plugin sends `registration` only to a library whose `capabilities` derivation lists it in
 * `captured_splice_inputs`. Otherwise [frozenOperatorEditsContained] and
 * [cleanRegistrationMergeUtil] shape the batch exactly as before and send it to the same symbols.
 * That floor composes the pre-#326 rules and is never extended. The shared parity file
 * `editor-derivations/registration_splice_shaping.json` is answered by both.
 */
internal object RegistrationSpliceShaping {
    private val LOG = Logger.getInstance(RegistrationSpliceShaping::class.java)
    private val GSON = Gson()

    /** The captured-splice FFI input this plugin generation sends (Rust `CAPTURED_SPLICE_INPUTS`). */
    internal const val REGISTRATION_INPUT = "registration"

    /** Wire shape of Rust `RegistrationShaping`. */
    internal data class Registration(
        @SerializedName("buffer") val buffer: String,
        @SerializedName("operatorText") val operatorText: Boolean,
    )

    /** The `edits_json` argument that carries [Registration] in place of a batch. */
    internal data class Request(
        @SerializedName("registration") val registration: Registration,
    )

    /** Which captured-splice symbol a request goes to. */
    internal enum class Call(val label: String) {
        CONTAINED("captured splice containment proof"),
        REBASE("captured splice rebase"),
    }

    /** A native answer: the call's text, or `null` when it refused or failed. */
    internal data class Answer(val text: String?)

    /**
     * The loaded native generation's captured-splice symbols. [ask] returns `null` when no loaded
     * generation advertises the `registration` input; the caller then takes the frozen floor.
     */
    internal fun interface Channel {
        fun ask(call: Call, base: String, canonical: String, requestJson: String): Answer?

        companion object {
            val Native = Channel { call, base, canonical, requestJson ->
                NativePatching.askCapturedSplicesWithRegistration(call, base, canonical, requestJson)
            }

            /** No native generation folds the shaping: callers take their frozen floor. */
            val Unavailable = Channel { _, _, _, _ -> null }
        }
    }

    internal fun requestJson(buffer: String, operatorText: Boolean): String =
        GSON.toJson(Request(Registration(buffer, operatorText)))

    /**
     * `#ambiguousholdforever` / `#ambiguousholdforever2`: true when [canonical] already carries
     * every operator edit from [shadow] to [buffer], compared as operator text. False when that is
     * not proven, including when no native library answers.
     */
    internal fun operatorEditsContained(
        shadow: String,
        buffer: String,
        canonical: String,
        channel: Channel = Channel.Native,
        frozenProof: (base: String, canonical: String, batch: PreparedLocalEditorBatch) -> Boolean? =
            NativePatching::capturedSplicesContained,
    ): Boolean {
        val answer =
            channel.ask(Call.CONTAINED, shadow, canonical, requestJson(buffer, operatorText = true))
                ?: return frozenOperatorEditsContained(shadow, buffer, canonical, frozenProof)
        return answer.text?.toBooleanStrictOrNull() ?: false
    }

    /**
     * `#editorauth2`: the clean three-way merge of a registration's generations, or null when any
     * is missing, they need no merge, no native library answers, or the rebase refuses. See
     * [cleanRegistrationMergeUtil].
     */
    internal fun cleanMerge(
        shadow: String?,
        buffer: String?,
        canonical: String?,
        channel: Channel = Channel.Native,
        frozenRebase: (base: String, canonical: String, batch: PreparedLocalEditorBatch) -> PreparedLocalEditorBatch? =
            NativePatching::rebaseCapturedSplices,
    ): String? {
        if (shadow == null || buffer == null || canonical == null) return null
        if (buffer == canonical || buffer == shadow || canonical == shadow) return null
        val answer =
            channel.ask(Call.REBASE, shadow, canonical, requestJson(buffer, operatorText = false))
                ?: return cleanRegistrationMergeUtil(shadow, buffer, canonical, frozenRebase)
        val json = answer.text ?: return null
        return try {
            GSON.fromJson(json, PreparedLocalEditorBatch::class.java)?.resultingText
        } catch (e: Exception) {
            LOG.warn("[native] captured splice rebase answer unreadable: ${e.message}")
            null
        }
    }

    /**
     * GH #326 compatibility floor: the pre-#326 operator-text containment shaping, frozen. Used
     * only when no native generation folds it. Never extend it; change the native
     * `registration_operator_text` / `registration_contained` and the parity file instead.
     */
    internal fun frozenOperatorEditsContained(
        shadow: String,
        buffer: String,
        canonical: String,
        proof: (base: String, canonical: String, batch: PreparedLocalEditorBatch) -> Boolean?,
    ): Boolean {
        // `#ambiguousholdforever2`: prove containment over operator text only. A controller
        // disk projection that the IDE reloaded carries binary-owned `(HEAD)` / boundary
        // placement canonical does not share; left in, that bookkeeping change can never be
        // "contained" and the hold had no exit (formal/tla/RetainedProjectionHold.tla).
        val shadowOperatorText = withoutBinaryOwnedMarkersUtil(shadow)
        val bufferOperatorText = withoutBinaryOwnedMarkersUtil(buffer)
        val canonicalOperatorText = withoutBinaryOwnedMarkersUtil(canonical)
        if (bufferOperatorText == shadowOperatorText || bufferOperatorText == canonicalOperatorText) {
            return true
        }
        return proof(
            shadowOperatorText,
            canonicalOperatorText,
            singleSpliceBatchUtil(shadowOperatorText, bufferOperatorText),
        ) ?: false
    }
}
