package com.github.btakita.agentdoc

import java.nio.file.Files
import java.nio.file.Paths
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * `#jetbrainsreplicaloops` (sdk.md, 2026-10-16T05:57Z-06:05Z). The replica looped
 * every 30 s on a stale-baseline local delta
 * (`recovery=canonical-rebootstrap-captured-local-delta`), every native save was
 * refused (`visible_length_mismatch`), and the controller's rebuilds were
 * exhausted (`state=editor_holds_unmerged_visible_text`). Only a controller
 * recycle ended it, by reseeding from this editor's buffer. The controller now
 * asks for that roll-forward in place; these tests pin when the editor may do it.
 */
class VisibleRollForwardTest {
    private val expected = "72a301c0"

    private fun decide(
        canonical: String? = expected,
        replica: String? = expected,
        visible: String? = "d5e3a7b1",
        structureExact: Boolean = true,
        remoteApplyInFlight: Boolean = false,
    ) = visibleRollForwardDecisionUtil(
        expectedCanonicalHash = expected,
        canonicalHash = canonical,
        replicaHash = replica,
        visibleHash = visible,
        visibleStructureExact = structureExact,
        remoteApplyInFlight = remoteApplyInFlight,
    )

    @Test
    fun `the measured stuck state rolls the visible buffer forward`() {
        // Replica rebuilt onto canonical 72a301c0, buffer holds operator text.
        assertEquals(VisibleRollForwardDecision.RollForward, decide())
    }

    @Test
    fun `a request for a canonical that has moved is declined`() {
        assertEquals(VisibleRollForwardDecision.CanonicalMoved, decide(canonical = "609d1d50", replica = "609d1d50"))
        assertEquals(VisibleRollForwardDecision.CanonicalMoved, decide(canonical = null))
    }

    @Test
    fun `a converged buffer needs nothing`() {
        assertEquals(
            VisibleRollForwardDecision.AlreadyConverged,
            decide(canonical = "d5e3a7b1", replica = "d5e3a7b1", visible = "d5e3a7b1"),
        )
    }

    @Test
    fun `a replica behind the fenced canonical drains before any splice is computed`() {
        assertEquals(VisibleRollForwardDecision.ReplicaBehind, decide(replica = "21456910"))
        assertEquals(VisibleRollForwardDecision.ReplicaBehind, decide(replica = null))
    }

    @Test
    fun `a replica that already holds the visible cut only republishes`() {
        assertEquals(VisibleRollForwardDecision.RepublishPending, decide(replica = "d5e3a7b1"))
    }

    @Test
    fun `no cut, a remote apply in flight, or a malformed buffer leaves the buffer held`() {
        assertEquals(VisibleRollForwardDecision.VisibleUnavailable, decide(visible = null))
        assertEquals(VisibleRollForwardDecision.RemoteApplyInFlight, decide(remoteApplyInFlight = true))
        assertEquals(VisibleRollForwardDecision.VisibleStructureInvalid, decide(structureExact = false))
    }

    @Test
    fun `roll forward happens only under every safety fact, across all combinations`() {
        val hashes = listOf(null, expected, "d5e3a7b1", "21456910")
        for (canonical in hashes) for (replica in hashes) for (visible in hashes) {
            for (exact in listOf(true, false)) for (inFlight in listOf(true, false)) {
                val decision = decide(canonical, replica, visible, exact, inFlight)
                val safe =
                    canonical == expected && replica == canonical && visible != null &&
                        visible != canonical && exact && !inFlight
                assertEquals(
                    "canonical=$canonical replica=$replica visible=$visible exact=$exact inFlight=$inFlight",
                    safe,
                    decision == VisibleRollForwardDecision.RollForward,
                )
                if (canonical != expected && canonical != visible) {
                    assertTrue(
                        "a moved canonical never mutates anything: $decision",
                        decision == VisibleRollForwardDecision.CanonicalMoved ||
                            decision == VisibleRollForwardDecision.VisibleUnavailable,
                    )
                }
            }
        }
    }

    @Test
    fun `only the typed roll-forward event licenses it, and it never re-registers`() {
        assertTrue(shouldRollVisibleForwardForRemoteEventUtil("editor_visible_roll_forward"))
        assertFalse(shouldRollVisibleForwardForRemoteEventUtil("editor_replica_reregister"))
        assertFalse(shouldRollVisibleForwardForRemoteEventUtil("canonical_projection"))
        assertFalse(shouldRollVisibleForwardForRemoteEventUtil(null))
        assertFalse(shouldReregisterForRemoteEventUtil("editor_visible_roll_forward"))
    }

    private fun source(name: String): String {
        val path = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/$name"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/$name"),
        ).first { Files.exists(it) }
        return Files.readString(path)
    }

    @Test
    fun `the socket routes the fenced request to the replica manager before any re-register`() {
        val branch = source("PatchWatcher.kt")
            .substringAfter("EditorIntent.DeliverCrdtRemote.token ->")
            .substringBefore("EditorIntent.RefreshVcs.token ->")
        val roll = branch.indexOf("shouldRollVisibleForwardForRemoteEventUtil(reasonToken)")
        assertTrue(roll >= 0)
        assertTrue(
            "the roll-forward branch must be decided before the re-register branch",
            roll < branch.indexOf("shouldReregisterForRemoteEventUtil(reasonToken)"),
        )
        assertTrue(
            "the request is fenced by the controller's canonical hash",
            branch.contains("extractStringField(json, \"content_hash\")") &&
                branch.contains("cause=missing_content_hash") &&
                branch.contains("CrdtReplicaManager.rollVisibleBufferForward("),
        )
        assertTrue(
            "only a completed editor decision may spend the controller attempt",
            branch.contains("VisibleRollForwardResult.Applied -> APPLY_APPLIED") &&
                branch.contains("VisibleRollForwardResult.Deferred -> APPLY_PROJECTION_CHANGED") &&
                branch.contains("VisibleRollForwardResult.Rejected -> APPLY_FAILED"),
        )
    }

    @Test
    fun `the roll-forward publishes the buffer and never writes controller text into it`() {
        val body = source("CrdtReplicaManager.kt")
            .substringAfter("private fun rollVisibleBufferForward(")
            .substringBefore("private fun scheduleUnforwardedOperatorTextRecovery(")
        assertTrue(
            "the visible cut is read under one read action and published as the replica text",
            body.contains("withEditorCaptureCut(filePath) { it }") &&
                body.contains("forwarder.ensureEditorText(visibleText)") &&
                body.contains("forwarder.replicaText() != visibleText"),
        )
        assertTrue(
            "the stale-baseline batch is subsumed; later keystrokes are kept",
            body.contains("shadows[filePath] = visibleText") &&
                body.contains("fenceCapturedEditsSubsumedByPublishedBuffer(filePath, visibleText, \"visible-roll-forward\")"),
        )
        assertTrue(
            "a canonical that kept controller text is drained in, not projected over",
            body.contains("canonicalAfter == visibleText && projectSettledVisibleState(") &&
                body.contains("requestUrgentRemoteDrain(filePath, \"visible-roll-forward-drain\")"),
        )
        assertFalse(
            "the roll-forward must never mutate the editor buffer",
            body.contains("runUndoableRemoteUpdateCommand") ||
                body.contains("setText(") ||
                body.contains("replaceString(") ||
                body.contains("prepareNonOperatorEditorMutationOnWorker"),
        )
    }

    @Test
    fun `the socket waits for the serialized decision instead of acknowledging scheduling`() {
        val body = source("CrdtReplicaManager.kt")
            .substringAfter("fun rollVisibleBufferForward(\n            project: Project,")
            .substringBefore("/**\n         * #jbmanagerbyfile")
        assertTrue(body.contains("submit<Boolean>"))
        assertTrue(body.contains(".get(CRDT_AWAIT_PERSIST_CURRENT_TIMEOUT_MS, TimeUnit.MILLISECONDS)"))
        assertTrue(body.contains("if (applied) VisibleRollForwardResult.Applied else VisibleRollForwardResult.Deferred"))
        assertTrue(body.contains("TimeoutException"))
        assertTrue(body.contains("VisibleRollForwardResult.Deferred"))
        assertFalse(body.contains("scheduleVisibleRollForward"))
    }
}
