package com.github.btakita.agentdoc

import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.atomic.AtomicLong
import javax.swing.SwingUtilities

/**
 * `#implementlazilyowned` (plan step 7): the native failure-ledger ABI
 * (`agent_doc_effect_failure_*`, `agent_doc_effect_cas::failure_ledger`).
 *
 * [generation] names the loaded native library generation, so a generation that
 * predates the ABI is remembered and a later hot-reloaded one is asked again.
 * Every call may throw [UnsatisfiedLinkError] when the loaded library lacks the symbol.
 */
internal interface EffectFailureLedgerNative {
    val generation: Any

    fun record(scope: String, key: String, reason: String): Int

    fun clear(scope: String, key: String): Int

    fun clearScope(scope: String): Int
}

/** The loaded native library's failure ledger, or null when no library is loaded. */
internal fun loadedNativeEffectFailureLedger(): EffectFailureLedgerNative? {
    val lib = AgentDocLib.get() ?: return null
    return object : EffectFailureLedgerNative {
        override val generation: Any = lib

        override fun record(scope: String, key: String, reason: String): Int =
            lib.agent_doc_effect_failure_record(scope, key, reason)

        override fun clear(scope: String, key: String): Int =
            lib.agent_doc_effect_failure_clear(scope, key)

        override fun clearScope(scope: String): Int = lib.agent_doc_effect_failure_clear_scope(scope)
    }
}

/**
 * `#implementlazilyowned` (plan step 7): one editor retry family's refusal counts, owned by
 * the Lazily failure ledger in the native core.
 *
 * Each [recordFailure] is one Lazily-claimed attempt of the key's retained intent recorded
 * `Retryable`; the count it returns is Lazily's attempt number, and the caller derives only its
 * pace curve and give-up bound from it. [clear] receipts the retained intent, so the next
 * refusal is attempt 1 again.
 *
 * [count] reads the last count Lazily returned, so it is safe on the EDT, where native calls are
 * forbidden. A record or clear that runs on the EDT is owed to the native ledger, in order, and
 * applied before the key's next native call, so the Lazily count stays exact.
 *
 * Compatibility: when the loaded native library predates the ABI (an older
 * `libagent_doc` loaded by a newer plugin), or no library is loaded, the ledger falls back to
 * the plugin's previous Kotlin-side count. It never fails or throws on that path.
 *
 * Each instance owns a unique native scope (`family@n`), so two projects' managers never share
 * counts and a disposed manager's scope is dropped by [clearAll].
 */
internal class EffectFailureLedger(
    family: String,
    private val native: () -> EffectFailureLedgerNative? = ::loadedNativeEffectFailureLedger,
    private val nativeCallAllowed: () -> Boolean = { !SwingUtilities.isEventDispatchThread() },
) {
    val scope: String = "$family@${SCOPE_SEQUENCE.incrementAndGet()}"

    /** The last count Lazily (or, on the compatibility path, this class) returned per key. */
    private val observed = ConcurrentHashMap<String, Int>()

    /** Work the EDT could not hand to the native ledger: a clear, then this many records. */
    private class Owed(var clear: Boolean = false, var records: Int = 0)
    private val owed = ConcurrentHashMap<String, Owed>()

    /** The native generation that lacked the ABI, so it is not asked again. */
    @Volatile private var unsupportedGeneration: Any? = null

    fun count(key: String): Int = observed[key] ?: 0

    /** Record one refused attempt of [key] and return its count (1 for the first refusal). */
    fun recordFailure(key: String, reason: String): Int {
        if (!nativeCallAllowed()) {
            if (nativeLedger() != NativeLedger.Unsupported) {
                owed.compute(key) { _, current -> (current ?: Owed()).also { it.records++ } }
            }
            return observed.merge(key, 1, Int::plus) ?: 1
        }
        val lazilyCount =
            withNative { ledger ->
                settleOwed(ledger, key)
                ledger.record(scope, key, reason)
            }?.takeIf { it > 0 }
        if (lazilyCount != null) {
            observed[key] = lazilyCount
            return lazilyCount
        }
        // Compatibility path: the previous plugin-side count.
        return observed.merge(key, 1, Int::plus) ?: 1
    }

    /** Receipt [key]'s retained failure: the effect converged or was given up on. */
    fun clear(key: String) {
        val retained = observed.remove(key) != null
        if (!nativeCallAllowed()) {
            if (retained || owed.containsKey(key)) owed[key] = Owed(clear = true)
            return
        }
        if (!retained && !owed.containsKey(key)) return
        owed.remove(key)
        withNative { ledger -> ledger.clear(scope, key) }
    }

    /** Drop every retained failure (the owner is disposed or reset). */
    fun clearAll() {
        val retained = observed.isNotEmpty() || owed.isNotEmpty()
        observed.clear()
        owed.clear()
        // A disposed owner's scope is never read again, so an EDT dispose may leave it to the
        // native ledger rather than block on a native call.
        if (retained && nativeCallAllowed()) withNative { ledger -> ledger.clearScope(scope) }
    }

    private fun settleOwed(ledger: EffectFailureLedgerNative, key: String) {
        val work = owed.remove(key) ?: return
        if (work.clear) ledger.clear(scope, key)
        repeat(work.records) { ledger.record(scope, key, "recorded-on-edt") }
    }

    private enum class NativeLedger { Unknown, Unsupported }

    /** Whether the native ledger is known to be missing (EDT-safe: no native call). */
    private fun nativeLedger(): NativeLedger =
        if (unsupportedGeneration != null) NativeLedger.Unsupported else NativeLedger.Unknown

    private fun <T> withNative(call: (EffectFailureLedgerNative) -> T): T? {
        val ledger =
            try {
                native()
            } catch (error: Throwable) {
                LOG.debug("[effect-failure-ledger] native library unavailable for $scope: ${error.message}")
                null
            } ?: return null
        if (unsupportedGeneration === ledger.generation) return null
        return try {
            call(ledger).also { unsupportedGeneration = null }
        } catch (error: UnsatisfiedLinkError) {
            unsupportedGeneration = ledger.generation
            owed.clear()
            LOG.info(
                "[effect-failure-ledger] loaded native library predates agent_doc_effect_failure_*; " +
                    "$scope keeps its plugin-side retry count",
            )
            null
        } catch (error: Throwable) {
            LOG.debug("[effect-failure-ledger] native ledger call failed for $scope: ${error.message}")
            null
        }
    }

    private companion object {
        val SCOPE_SEQUENCE = AtomicLong(0)
        val LOG = com.intellij.openapi.diagnostic.Logger.getInstance(EffectFailureLedger::class.java)
    }
}
