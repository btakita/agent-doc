package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * `#implementlazilyowned` (plan step 7): [EffectFailureLedger] reads its counts from the native
 * Lazily failure ledger, owes EDT work to it in order, and falls back to the plugin-side count
 * when the loaded library predates the ABI.
 *
 * [LazilyLedgerModel] is the contract `agent_doc_effect_cas::failure_ledger` implements and
 * `ffi::tests::effect_failure_ledger_abi_counts_lazily_attempts_per_scope_and_key` pins on the
 * native side: a count per `(scope, key)` that a clear resets and a scope clear drops.
 */
class EffectFailureLedgerTest {
    private class LazilyLedgerModel(override val generation: Any = Any()) : EffectFailureLedgerNative {
        val counts = mutableMapOf<Pair<String, String>, Int>()
        val calls = mutableListOf<String>()

        override fun record(scope: String, key: String, reason: String): Int {
            calls += "record:$key"
            return counts.merge(scope to key, 1, Int::plus)!!
        }

        override fun clear(scope: String, key: String): Int {
            calls += "clear:$key"
            return if (counts.remove(scope to key) != null) 1 else 0
        }

        override fun clearScope(scope: String): Int {
            calls += "clear-scope"
            val dropped = counts.keys.filter { it.first == scope }
            dropped.forEach(counts::remove)
            return dropped.size
        }
    }

    /** A native library generation built before `agent_doc_effect_failure_*` existed. */
    private class PreAbiLibrary(override val generation: Any = Any()) : EffectFailureLedgerNative {
        var calls = 0

        override fun record(scope: String, key: String, reason: String): Int {
            calls++
            throw UnsatisfiedLinkError("Error looking up function 'agent_doc_effect_failure_record'")
        }

        override fun clear(scope: String, key: String): Int {
            calls++
            throw UnsatisfiedLinkError("Error looking up function 'agent_doc_effect_failure_clear'")
        }

        override fun clearScope(scope: String): Int {
            calls++
            throw UnsatisfiedLinkError("Error looking up function 'agent_doc_effect_failure_clear_scope'")
        }
    }

    @Test
    fun `counts are the native Lazily attempt numbers and a clear restarts them`() {
        val native = LazilyLedgerModel()
        val ledger = EffectFailureLedger("test.native", native = { native }, nativeCallAllowed = { true })
        assertEquals(0, ledger.count("/a.md"))
        assertEquals(1, ledger.recordFailure("/a.md", "refused"))
        assertEquals(2, ledger.recordFailure("/a.md", "refused"))
        assertEquals(1, ledger.recordFailure("/b.md", "refused"))
        assertEquals(2, ledger.count("/a.md"))
        assertEquals(2, native.counts[ledger.scope to "/a.md"])
        ledger.clear("/a.md")
        assertEquals(0, ledger.count("/a.md"))
        assertEquals(null, native.counts[ledger.scope to "/a.md"])
        assertEquals(1, ledger.recordFailure("/a.md", "refused"))
        ledger.clearAll()
        assertTrue(native.counts.isEmpty())
    }

    @Test
    fun `an older native library without the ABI falls back to the plugin-side count`() {
        val old = PreAbiLibrary()
        val ledger = EffectFailureLedger("test.compat", native = { old }, nativeCallAllowed = { true })
        assertEquals(1, ledger.recordFailure("/a.md", "refused"))
        assertEquals(2, ledger.recordFailure("/a.md", "refused"))
        assertEquals(3, ledger.recordFailure("/a.md", "refused"))
        assertEquals(3, ledger.count("/a.md"))
        ledger.clear("/a.md")
        assertEquals(0, ledger.count("/a.md"))
        assertEquals(1, ledger.recordFailure("/a.md", "refused"))
        ledger.clearAll()
        // The missing symbol is learned once per native generation, never re-probed per call.
        assertEquals(1, old.calls)
    }

    @Test
    fun `a hot-reloaded library with the ABI is asked again after an older one`() {
        var loaded: EffectFailureLedgerNative = PreAbiLibrary()
        val ledger = EffectFailureLedger("test.reload", native = { loaded }, nativeCallAllowed = { true })
        assertEquals(1, ledger.recordFailure("/a.md", "refused"))
        val current = LazilyLedgerModel()
        loaded = current
        ledger.clear("/a.md")
        assertEquals(1, ledger.recordFailure("/a.md", "refused"))
        assertEquals(1, current.counts[ledger.scope to "/a.md"])
    }

    @Test
    fun `no loaded library keeps the plugin-side count`() {
        val ledger = EffectFailureLedger("test.unloaded", native = { null }, nativeCallAllowed = { true })
        assertEquals(1, ledger.recordFailure("/a.md", "refused"))
        assertEquals(2, ledger.recordFailure("/a.md", "refused"))
        ledger.clear("/a.md")
        assertEquals(1, ledger.recordFailure("/a.md", "refused"))
    }

    @Test
    fun `EDT records and clears are owed to the native ledger in order`() {
        val native = LazilyLedgerModel()
        var onEdt = false
        val ledger = EffectFailureLedger("test.edt", native = { native }, nativeCallAllowed = { !onEdt })
        assertEquals(1, ledger.recordFailure("/a.md", "refused"))
        onEdt = true
        assertEquals(2, ledger.recordFailure("/a.md", "refused"))
        assertEquals(2, ledger.count("/a.md"))
        onEdt = false
        // The owed EDT record lands before this one, so the Lazily count stays exact.
        assertEquals(3, ledger.recordFailure("/a.md", "refused"))
        assertEquals(3, native.counts[ledger.scope to "/a.md"])

        onEdt = true
        ledger.clear("/a.md")
        assertEquals(1, ledger.recordFailure("/a.md", "refused"))
        assertEquals(3, native.counts[ledger.scope to "/a.md"])
        onEdt = false
        assertEquals(2, ledger.recordFailure("/a.md", "refused"))
        assertEquals(
            listOf(
                "record:/a.md",
                "record:/a.md",
                "record:/a.md",
                "clear:/a.md",
                "record:/a.md",
                "record:/a.md",
            ),
            native.calls,
        )
    }

    @Test
    fun `two ledgers of one family never share native counts`() {
        val native = LazilyLedgerModel()
        val first = EffectFailureLedger("test.scope", native = { native }, nativeCallAllowed = { true })
        val second = EffectFailureLedger("test.scope", native = { native }, nativeCallAllowed = { true })
        assertTrue(first.scope != second.scope)
        assertEquals(1, first.recordFailure("/a.md", "refused"))
        assertEquals(2, first.recordFailure("/a.md", "refused"))
        assertEquals(1, second.recordFailure("/a.md", "refused"))
    }

    @Test
    fun `the shared doubling pace matches the previous inline curves`() {
        assertEquals(100L, crdtDoublingBackoffMsUtil(1))
        assertEquals(200L, crdtDoublingBackoffMsUtil(2))
        assertEquals(25_600L, crdtDoublingBackoffMsUtil(9))
        assertEquals(30_000L, crdtDoublingBackoffMsUtil(10))
        assertEquals(30_000L, crdtDoublingBackoffMsUtil(400))
    }
}
