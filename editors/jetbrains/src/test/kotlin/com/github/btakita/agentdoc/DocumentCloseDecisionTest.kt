package com.github.btakita.agentdoc

import java.nio.file.Files
import java.nio.file.Paths
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * GH #241: a `fileClosed` for one removed editor of a still-open document must not
 * deregister the document's only CRDT replica.
 */
class DocumentCloseDecisionTest {
    private val doc = "/repo/tasks/doc-a.md"

    private fun observe(
        backendOpen: Boolean = false,
        sourceOpen: Boolean = false,
        clients: List<Collection<String>> = emptyList(),
    ) = DocumentCloseObservation(backendOpen, sourceOpen, clients)

    @Test
    fun `duplicate editor removed while the file stays open locally keeps the replica`() {
        assertEquals(
            DocumentCloseVerdict.KEEP_STILL_OPEN,
            DocumentCloseDecision.verdict(doc, observe(backendOpen = true, sourceOpen = true)),
        )
        assertEquals(
            "the manager that fired fileClosed still has another editor for the file",
            DocumentCloseVerdict.KEEP_STILL_OPEN,
            DocumentCloseDecision.verdict(doc, observe(sourceOpen = true)),
        )
    }

    @Test
    fun `remote dev guest still listing the file in getAllFiles keeps the replica`() {
        // The GH #241 shape: the backend-local manager reports nothing open, the
        // remote client removed one of two editors, and its getAllFiles still lists
        // the document.
        assertEquals(
            DocumentCloseVerdict.KEEP_STILL_OPEN,
            DocumentCloseDecision.verdict(
                doc,
                observe(clients = listOf(listOf("/repo/tasks/doc-b.md", doc, "/repo/tasks/doc-d.md"))),
            ),
        )
        assertEquals(
            "any one client still holding the file keeps it open",
            DocumentCloseVerdict.KEEP_STILL_OPEN,
            DocumentCloseDecision.verdict(
                doc,
                observe(clients = listOf(listOf("/repo/tasks/doc-b.md"), listOf(doc))),
            ),
        )
    }

    @Test
    fun `last editor closed in every client releases the replica`() {
        assertEquals(DocumentCloseVerdict.RELEASE, DocumentCloseDecision.verdict(doc, observe()))
        assertEquals(
            DocumentCloseVerdict.RELEASE,
            DocumentCloseDecision.verdict(
                doc,
                observe(clients = listOf(listOf("/repo/tasks/doc-b.md"), emptyList())),
            ),
        )
    }

    @Test
    fun `a sibling path sharing a prefix does not count as open`() {
        assertEquals(
            DocumentCloseVerdict.RELEASE,
            DocumentCloseDecision.verdict(doc, observe(clients = listOf(listOf("$doc.bak", "/repo/tasks/doc-a.mdx")))),
        )
    }

    private fun source(name: String): String {
        val path = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/$name"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/$name"),
        ).first { Files.exists(it) }
        return Files.readString(path)
    }

    @Test
    fun `close decision ignores the EditorTracker active-editor set`() {
        // GH #175: EditorTracker retains hidden Editor/Preview tabs and can still hold
        // the editor being removed at fileClosed time; trusting it would keep a
        // genuinely closed document's replica forever.
        val decision = source("DocumentCloseDecision.kt")
        assertFalse(decision.contains("EditorTracker.getInstance"))
        assertFalse(decision.contains("EditorOpenFileSurface."))
        assertTrue(decision.contains("MarketplacePlatformBridge.remoteFileEditorManagers(project)"))
        assertTrue(decision.contains("MarketplacePlatformBridge.allFiles(manager)"))
        assertTrue(decision.contains("FileEditorManager.getInstance(project).isFileOpen(file)"))
    }

    @Test
    fun `fileClosed routes the replica release and liveness close through the decision`() {
        val lifecycle = source("AgentDocProjectLifecycle.kt")
        val closed = lifecycle.substringAfter("override fun fileClosed(source: FileEditorManager, file: VirtualFile)")
            .substringBefore("\n                    }")
        assertTrue(closed.contains("TypingTracker.onEditorClosed(project, source, file)"))
        assertFalse(
            "fileClosed must not release the replica unconditionally",
            closed.contains("clearOpenDocumentReport"),
        )

        val tracker = source("TypingTracker.kt")
        val onClosed = tracker.substringAfter("fun onEditorClosed(").substringBefore("fun clearOpenDocumentReport")
        assertTrue(onClosed.contains("DocumentCloseDecision.route("))
        assertTrue(onClosed.contains("onRelease = { clearOpenDocumentReport(file) }"))
        assertTrue(
            "a kept close re-observes the replica so a lost registration self-heals",
            onClosed.contains("CrdtReplicaManager.ensureOpenDocumentReplica("),
        )

        val liveness = source("ReliableSyncLivenessListener.kt")
        val livenessClosed = liveness.substringAfter("override fun fileClosed(").substringBefore("private fun publishClose")
        assertTrue(livenessClosed.contains("DocumentCloseDecision.route("))
        assertTrue(livenessClosed.contains("onRelease = { publishClose(file) }"))
        assertFalse(livenessClosed.contains("graph.close("))
    }

    @Test
    fun `a still-open verdict re-observes once after a bound and then releases`() {
        val decision = source("DocumentCloseDecision.kt")
        val route = decision.substringAfter("fun route(")
        val keep = route.substringAfter("DocumentCloseVerdict.KEEP_STILL_OPEN ->")
        assertTrue(keep.contains("STILL_OPEN_RECHECK_MS"))
        assertTrue(keep.contains("observe(project, null, file)"))
        assertTrue(keep.contains("onRelease()"))
        assertTrue(
            "a newer close for the same consumer and file supersedes a pending re-observation",
            keep.contains("pendingRechecks.put(key, future)?.cancel(false)"),
        )
        assertTrue(DocumentCloseDecision.STILL_OPEN_RECHECK_MS in 100L..10_000L)
    }

    @Test
    fun `selecting an open document re-attaches a missing replica`() {
        val lifecycle = source("AgentDocProjectLifecycle.kt")
        val selection = lifecycle.substringAfter("override fun selectionChanged(event: FileEditorManagerEvent)")
            .substringBefore("override fun fileOpened")
        assertTrue(selection.contains("CrdtReplicaManager.ensureOpenDocumentReplica(project, file.path, \"selection\")"))
    }
}
