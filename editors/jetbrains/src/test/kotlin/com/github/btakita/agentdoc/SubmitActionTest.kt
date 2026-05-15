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
        val saveIdx = source.indexOf("FileDocumentManager.getInstance().saveAllDocuments()")
        val routeIdx = source.indexOf("TerminalUtil.sendToTerminal(project, file)")

        assertTrue("SubmitAction should wait for typing idle", awaitIdx >= 0)
        assertTrue("SubmitAction should save after waiting for idle", saveIdx > awaitIdx)
        assertTrue("SubmitAction should route after saving", routeIdx > saveIdx)
    }
}
