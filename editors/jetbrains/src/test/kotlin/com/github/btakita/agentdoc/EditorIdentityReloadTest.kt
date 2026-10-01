package com.github.btakita.agentdoc

import java.util.Properties
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #jbrejectlog: a dynamic plugin reload re-initializes Kotlin objects, so the
 * editor id and replica epoch must come from process-wide state that outlives
 * the plugin classloader.
 */
class EditorIdentityReloadTest {
    @Test
    fun `a reloaded plugin adopts the editor id its process already minted`() {
        val process = Properties()
        var minted = 0
        val first = processStableEditorIdUtil(process) { minted += 1; "jetbrains-1-aaa" }
        // A second classloader initializing EditorIdentity in the same JVM.
        val reloaded = processStableEditorIdUtil(process) { minted += 1; "jetbrains-1-bbb" }
        assertEquals("jetbrains-1-aaa", first)
        assertEquals(first, reloaded)
        assertEquals(1, minted)
    }

    @Test
    fun `a fresh process mints its own id`() {
        assertEquals("jetbrains-2-ccc", processStableEditorIdUtil(Properties()) { "jetbrains-2-ccc" })
    }

    @Test
    fun `replica epochs keep increasing across a reload`() {
        val process = Properties()
        val before = (1..3).map { nextReplicaConnectionEpochUtil(process) }
        // The reloaded plugin continues the same process-wide sequence, so it can
        // never reissue a `:refresh-N` identity its predecessor already used.
        val after = (1..2).map { nextReplicaConnectionEpochUtil(process) }
        assertEquals(listOf(1L, 2L, 3L), before)
        assertEquals(listOf(4L, 5L), after)
        assertTrue((before + after).toSet().size == 5)
    }
}
