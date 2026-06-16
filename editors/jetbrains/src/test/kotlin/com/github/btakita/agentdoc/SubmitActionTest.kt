package com.github.btakita.agentdoc

import java.nio.file.Paths
import org.junit.Assert.assertTrue
import org.junit.Test

class SubmitActionTest {
    @Test
    fun `run agent doc waits for typing idle before saving and routing`() {
        val source = Paths.get(
            "src/main/kotlin/com/github/btakita/agentdoc/SubmitAction.kt"
        ).toFile().readText()

        val awaitIdx = source.indexOf("TypingTracker.awaitIdle(file.path)")
        val deferIdx = source.indexOf("return@Thread")
        val saveIdx = source.indexOf("FileDocumentManager.getInstance().saveAllDocuments()")
        val ledgerIdx = source.indexOf("RunAgentDocAttemptLedger.begin(")
        val awaitRecordIdx = source.indexOf("attempt.recordIfCurrent(\"await_typing_idle\")")
        val saveRecordIdx = source.indexOf("attempt.recordIfCurrent(\"documents_saved\")")
        val routeIdx = source.indexOf("TerminalUtil.sendToTerminal(project, file, attempt = attempt)")

        assertTrue("SubmitAction should begin a durable Run Agent Doc attempt", ledgerIdx >= 0)
        assertTrue("SubmitAction should wait for typing idle", awaitIdx >= 0)
        assertTrue("SubmitAction should record the typing wait stage", awaitRecordIdx in ledgerIdx..awaitIdx)
        assertTrue("SubmitAction should defer routing when typing never settles", deferIdx > awaitIdx)
        assertTrue("SubmitAction should defer before saving", saveIdx > deferIdx)
        assertTrue("SubmitAction should save after waiting for idle", saveIdx > awaitIdx)
        assertTrue("SubmitAction should record document save before routing", saveRecordIdx > saveIdx)
        assertTrue("SubmitAction should route after saving", routeIdx > saveIdx)
    }
}
