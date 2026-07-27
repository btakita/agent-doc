package com.github.btakita.agentdoc

import java.nio.file.Paths
import org.junit.Assert.assertTrue
import org.junit.Test

class SubmitActionTest {
    @Test
    fun `run agent doc saves active document before routing without blocking debounce`() {
        val source = Paths.get(
            "src/main/kotlin/com/github/btakita/agentdoc/SubmitAction.kt"
        ).toFile().readText()

        val ledgerIdx = source.indexOf("RunAgentDocAttemptLedger.begin(")
        val currentCheckIdx = source.indexOf("if (!attempt.isCurrent())")
        val saveIdx = source.indexOf("fdm.saveDocument(document)")
        val saveRecordIdx = source.indexOf("attempt.recordIfCurrent(\"active_document_saved\")")
        // Match the call, not its exact argument list — pinning the full string
        // breaks whenever a parameter is added (it did, for `resolved =`), which
        // tests ordering by accident rather than on purpose.
        val routeIdx = source.indexOf("TerminalUtil.sendToTerminal(")

        assertTrue("SubmitAction should begin a durable Run Agent Doc attempt", ledgerIdx >= 0)
        assertTrue("SubmitAction should drop stale queued callbacks before saving", currentCheckIdx > ledgerIdx)
        assertTrue("SubmitAction should check current attempt before saving", saveIdx > currentCheckIdx)
        assertTrue("SubmitAction should save the active document", saveIdx > ledgerIdx)
        assertTrue("SubmitAction should record document save before routing", saveRecordIdx > saveIdx)
        assertTrue("SubmitAction should route after saving", routeIdx > saveIdx)
        assertTrue(
            "text selection must not fork Run Agent Doc onto a distinct steering path",
            !source.contains("selectionModel") && !source.contains("selectedText"),
        )
        assertTrue("SubmitAction should not block on the typing debounce", !source.contains("TypingTracker.awaitIdle(file.path)"))
        assertTrue("SubmitAction should not save unrelated open documents", !source.contains("saveAllDocuments()"))
        assertTrue("SubmitAction should let repeated Run clicks reach the route supersede path", !source.contains("InvocationCoalescer.key(\"run\""))
    }
}
