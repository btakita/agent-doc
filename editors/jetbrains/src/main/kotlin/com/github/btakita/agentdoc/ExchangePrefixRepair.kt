package com.github.btakita.agentdoc

import com.google.gson.Gson
import com.google.gson.JsonParser
import com.google.gson.annotations.SerializedName
import com.intellij.openapi.diagnostic.Logger
import javax.swing.SwingUtilities

/**
 * `#jbshrinkrestarts`: the patch watcher's exchange `❯ ` prompt-prefix repair, derived by the
 * hot-reloaded native library.
 *
 * A patch that names `normalize_prefix_lines` has its target operator lines prefixed after the
 * patch and boundary reposition (`PatchWatcher.computePatchOnWorker`). Which lines are operator
 * prompts, response labels, or response prose is a heuristic word list; it is the native
 * `exchange_prefix_repair` derivation now. The patch worker sends the patched document and the
 * targets ([evidence]) and takes the answer; an unchanged answer does not carry the document back.
 *
 * The patch worker asks off the EDT. [frozen] ([normalizeExchangePrefixesUtil], the pre-move rule)
 * answers only when no native generation does, and is never extended. A document the native
 * boundary cannot carry losslessly (an unpaired surrogate) is refused before it crosses and takes
 * the floor too.
 */
internal object ExchangePrefixRepair {
    private val LOG = Logger.getInstance(ExchangePrefixRepair::class.java)
    private val GSON = Gson()

    /** Wire shape of Rust `ExchangePrefixRepairEvidence`. */
    internal data class Evidence(
        @SerializedName("document") val document: String,
        @SerializedName("prefix_lines") val prefixLines: List<String>,
    )

    internal fun evidence(document: String, prefixLines: List<String>): Evidence = Evidence(document, prefixLines)

    /** Repair [document]'s exchange prefixes for [prefixLines]. Never on the EDT. */
    internal fun repair(
        document: String,
        prefixLines: List<String>,
        derivation: EditorDerivation = EditorDerivation.Native,
    ): String {
        if (prefixLines.isEmpty()) return document
        if (SwingUtilities.isEventDispatchThread()) {
            LOG.warn("[patch-watcher] exchange_prefix_repair asked on the EDT; using the compatibility floor")
            return frozen(document, prefixLines)
        }
        val answer =
            derivation.derive("exchange_prefix_repair", GSON.toJson(evidence(document, prefixLines)))
                ?: return frozen(document, prefixLines)
        return parseNative(answer, document) ?: frozen(document, prefixLines)
    }

    /** The repaired document, [document] itself for an unchanged answer, or `null` for the floor. */
    internal fun parseNative(json: String, document: String): String? =
        try {
            val root = JsonParser.parseString(json).asJsonObject
            val changed = root.get("changed")?.takeIf { it.isJsonPrimitive && it.asJsonPrimitive.isBoolean }?.asBoolean
            val repaired = root.get("document")?.takeIf { it.isJsonPrimitive && it.asJsonPrimitive.isString }?.asString
            when {
                changed == false -> document
                changed == true && repaired != null -> repaired
                else -> null
            }
        } catch (e: Exception) {
            LOG.warn("[patch-watcher] native exchange_prefix_repair unreadable: ${e.message}")
            null
        }

    /** Compatibility floor: the pre-move Kotlin rule, frozen. Never extended. */
    internal fun frozen(document: String, prefixLines: List<String>): String =
        normalizeExchangePrefixesUtil(document, prefixLines)
}
