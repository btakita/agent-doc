package com.github.btakita.agentdoc

import java.nio.file.Files
import java.nio.file.Paths
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * `#ctrlkillreregister` Tier 3 — the editor-side pull decision.
 *
 * These assert the two distinctions the caller's correctness hangs on: "could not
 * ask" is not "nothing to do", and another peer's stranded registration is not this
 * editor's to rebuild.
 */
class PeerReplicaPullTest {
    private fun registration(pid: Long, path: String): String =
        """{"document_hash":"h-$pid","pid":$pid,"path":"$path","editor_id":"jetbrains-$pid",""" +
            """"editor_kind":"jetbrains","editor_version":"0.2.0","capabilities":[],"timestamp_ms":1}"""

    @Test
    fun `an up-to-date editor is told to rebuild nothing`() {
        assertEquals(emptyList<String>(), PeerReplicaPull.rebuildPaths("[]", 42L))
    }

    @Test
    fun `an unanswerable pull is distinct from an empty answer`() {
        // Null must NOT collapse into "nothing to do": the caller falls back to the
        // controller's compatibility fan-out on null, and doing nothing instead would
        // leave the editor stranded exactly when the pull is unavailable.
        assertNull("a missing ABI/controller cannot mean up to date", PeerReplicaPull.rebuildPaths(null, 42L))
        assertNull(PeerReplicaPull.rebuildPaths("", 42L))
        assertNull(PeerReplicaPull.rebuildPaths("   ", 42L))
        assertNull("malformed JSON is not an empty answer", PeerReplicaPull.rebuildPaths("{not json", 42L))
        assertNull("a non-array answer is not an empty answer", PeerReplicaPull.rebuildPaths("""{"pid":42}""", 42L))

        assertNotNull("a real empty answer stays empty", PeerReplicaPull.rebuildPaths("[]", 42L))
    }

    @Test
    fun `only this editor's own stranded registrations are rebuilt`() {
        val json = "[${registration(42L, "/proj/mine.md")},${registration(7L, "/proj/theirs.md")}]"

        assertEquals(
            "rebuilding another editor's document would publish this buffer's text over theirs",
            listOf("/proj/mine.md"),
            PeerReplicaPull.rebuildPaths(json, 42L),
        )
        assertEquals(listOf("/proj/theirs.md"), PeerReplicaPull.rebuildPaths(json, 7L))
        assertEquals(emptyList<String>(), PeerReplicaPull.rebuildPaths(json, 9999L))
    }

    @Test
    fun `duplicate and malformed entries do not produce duplicate or empty rebuild targets`() {
        val json = "[" +
            registration(42L, "/proj/a.md") + "," +
            registration(42L, "/proj/a.md") + "," +
            """{"pid":42}""" + "," +
            """{"path":"/proj/b.md"}""" + "," +
            """{"pid":42,"path":""}""" + "," +
            "\"not-an-object\"" +
            "]"

        assertEquals(
            "one re-register per stranded path; entries without a pid+path are skipped",
            listOf("/proj/a.md"),
            PeerReplicaPull.rebuildPaths(json, 42L),
        )
    }

    /**
     * The pull must be what runs at startup. A blind refresh of every open document
     * re-registers healthy replicas — dropping and rebuilding a live CRDT baseline —
     * and still misses a stranded registration whose document is not open in a tab.
     */
    @Test
    fun `startup pulls missing replicas instead of blindly refreshing every open document`() {
        val source = Files.readString(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/PluginLifecycleListener.kt")
                .takeIf { Files.exists(it) }
                ?: Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/PluginLifecycleListener.kt"),
        )

        assertTrue(
            source.contains("CrdtReplicaManager.pullMissingReplicas(project, \"plugin-startup\")"),
        )
        assertTrue(
            "the blind sweep must not remain a startup call site",
            !source.contains("forceRefreshOpenDocumentReplicas(project, \"plugin-startup\")"),
        )
    }

    /**
     * The pull is also the reconnect path: one document noticing controller transport
     * loss means every registration this editor holds is stranded, including ones
     * nothing is currently draining.
     */
    @Test
    fun `controller transport recovery pulls for the whole editor, coalesced`() {
        val source = Files.readString(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt")
                .takeIf { Files.exists(it) }
                ?: Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt"),
        )

        val recovery = source.substringAfter("private fun refreshReplicaAfterTransportLoss")
            .substringBefore("private fun shouldAttemptRegister")
        assertTrue(
            "transport recovery must ask about the rest of this editor's registrations",
            recovery.contains("pullMissingReplicas(project, \"controller-transport-recovered\")"),
        )

        val pull = source.substringAfter("fun pullMissingReplicas(")
            .substringBefore("fun forceRefreshOpenDocumentReplicas")
        assertTrue(
            "a dead controller strands every document at once; one pull answers for all",
            pull.contains("beginPeerReplicaPull()"),
        )
        assertTrue(
            "held must stay empty: stale local forwarders would suppress the repair",
            pull.contains("emptyList()"),
        )
        assertTrue(
            "an unaskable pull must fall back, not silently do nothing",
            pull.contains("forceRefreshOpenDocumentReplicas(project, \"\$reason-pull-unavailable\")"),
        )
    }

    /**
     * The capability token is the controller's retirement condition for the Tier 1
     * fan-out. Advertising it without calling the pull would silence the push while
     * nothing repaired — strictly worse than either tier alone.
     */
    @Test
    fun `the advertised pull capability matches the code that actually pulls`() {
        val capabilities = Files.readString(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/TypingTracker.kt")
                .takeIf { Files.exists(it) }
                ?: Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/TypingTracker.kt"),
        )
        assertTrue(capabilities.contains("\"peer_replica_pull_v1\""))
        assertTrue(capabilities.contains("add(PEER_REPLICA_PULL_CAPABILITY)"))
        assertTrue(
            "the token must be advertised in the registration payload",
            EDITOR_CAPABILITIES.split(",").contains("peer_replica_pull_v1"),
        )

        val pullSource = Files.readString(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/PeerReplicaPull.kt")
                .takeIf { Files.exists(it) }
                ?: Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/PeerReplicaPull.kt"),
        )
        assertTrue(
            "the capability claims this exact FFI call",
            pullSource.contains("agent_doc_peer_replicas_missing("),
        )
    }
}
