package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import java.nio.file.Files
import java.nio.file.Path

/**
 * `#netadv5` R2: a slow replica attach (750ms) or a slow document-lane save (5s)
 * must answer `deferred`, never `rejected`. The controller drops a replica from
 * the delivery cut only on a definitive refusal, so a timeout that answered
 * `rejected` manufactured that proof.
 */
class PatchWatcherDeferredReceiptTest {
    @Test
    fun `a bounded wait that elapsed answers deferred, a real refusal answers failed`() {
        assertEquals(PatchWatcher.APPLY_DEFERRED, PatchWatcher.timeoutAwareReceiptUtil(pending = true))
        assertEquals(PatchWatcher.APPLY_FAILED, PatchWatcher.timeoutAwareReceiptUtil(pending = false))
        assertEquals(3, PatchWatcher.APPLY_DEFERRED)
    }

    @Test
    fun `deliver and persist lanes route their timeout to the deferred receipt`() {
        val watcher = Files.readString(Path.of("src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt"))
        val delivery = watcher.substringAfter("EditorIntent.DeliverCrdtRemote.token ->")
            .substringBefore("EditorIntent.RefreshVcs.token ->")
        assertTrue(delivery.contains("reregisterPending.get()") && delivery.contains("APPLY_DEFERRED"))
        val persist = watcher.substringAfter("EditorIntent.PersistCurrent.token ->")
            .substringBefore("EditorIntent.DeliverCrdtRemote.token ->")
        assertTrue(persist.contains("timeoutAwareReceiptUtil(pending = persistPending.get())"))

        val manager = Files.readString(Path.of("src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt"))
        val recovery = manager.substringAfter("fun refreshOpenDocumentReplicaForRecoveryAndWait(")
            .substringBefore("private fun runOnEdtNonBlocking(")
        assertTrue("attach timeout marks pending", recovery.contains("onAwaitTimeout = { pendingOut?.set(true) }"))
        assertTrue("coalesced re-register marks pending", recovery.contains("receipt=deferred"))
        val persistLane = manager.substringAfter("fun persistCurrentVisibleRevision(\n            project: Project,")
            .substringBefore("fun ")
        assertTrue("document-lane timeout marks pending", persistLane.contains("pendingOut?.set(true)"))
    }
}
