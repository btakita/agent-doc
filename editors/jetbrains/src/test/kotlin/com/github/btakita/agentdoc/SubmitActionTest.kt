package com.github.btakita.agentdoc

import java.nio.file.Paths
import org.junit.Assert.assertTrue
import org.junit.Test

class SubmitActionTest {
    @Test
    fun `run agent doc saves active document before routing without blocking debounce`() {
        val source =
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/SubmitAction.kt")
                .toFile()
                .readText()

        val saveIdx = source.indexOf("fileDocumentManager.saveDocument(document)")
        val pooledIdx = source.indexOf("executeOnPooledThread")
        val resolveIdx = source.indexOf("TerminalUtil.resolveProject(project, file)")
        val ledgerIdx = source.indexOf("RunAgentDocAttemptLedger.begin(")
        val saveRecordIdx = source.indexOf("attempt.recordIfCurrent(saveStage)")
        val currentCheckIdx = source.indexOf("if (!attempt.isCurrent())")
        // Match the call, not its exact argument list — pinning the full string
        // breaks whenever a parameter is added (it did, for `resolved =`), which
        // tests ordering by accident rather than on purpose.
        val routeIdx = source.indexOf("TerminalUtil.sendToTerminal(")

        assertTrue("SubmitAction should save the active document", saveIdx >= 0)
        assertTrue("SubmitAction should save before leaving the EDT", pooledIdx > saveIdx)
        assertTrue("project resolution must run on the pooled thread", resolveIdx > pooledIdx)
        assertTrue("SubmitAction should begin a durable Run Agent Doc attempt", ledgerIdx >= 0)
        assertTrue("attempt creation must follow project resolution", ledgerIdx > resolveIdx)
        assertTrue(
            "SubmitAction should record document save before routing",
            saveRecordIdx > saveIdx,
        )
        assertTrue(
            "SubmitAction should drop stale queued callbacks before routing",
            currentCheckIdx > ledgerIdx,
        )
        assertTrue("SubmitAction should route after saving", routeIdx > saveIdx)
        assertTrue(
            "text selection must not fork Run Agent Doc onto a distinct steering path",
            !source.contains("selectionModel") && !source.contains("selectedText"),
        )
        assertTrue(
            "SubmitAction should not block on the typing debounce",
            !source.contains("TypingTracker.awaitIdle(file.path)"),
        )
        assertTrue(
            "SubmitAction should not save unrelated open documents",
            !source.contains("saveAllDocuments()"),
        )
        assertTrue(
            "SubmitAction should let repeated Run clicks reach the route supersede path",
            !source.contains("InvocationCoalescer.key(\"run\""),
        )
    }
}
