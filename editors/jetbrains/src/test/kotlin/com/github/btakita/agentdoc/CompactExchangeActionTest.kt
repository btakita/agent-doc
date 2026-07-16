package com.github.btakita.agentdoc

import java.nio.file.Paths
import org.junit.Assert.assertTrue
import org.junit.Test

class CompactExchangeActionTest {
    @Test
    fun `compact exchange saves only its target document before routing`() {
        val source = Paths.get(
            "src/main/kotlin/com/github/btakita/agentdoc/CompactExchangeAction.kt",
        ).toFile().readText()

        val lookupIdx = source.indexOf("fdm.getDocument(file)")
        val saveIdx = source.indexOf("fdm.saveDocument(document)")
        val compactIdx = source.indexOf("TerminalUtil.compactExchange(project, file)")

        assertTrue("Compact Exchange should resolve its target document", lookupIdx >= 0)
        assertTrue("Compact Exchange should save only that document", saveIdx > lookupIdx)
        assertTrue("Compact Exchange should route after its target save", compactIdx > saveIdx)
        assertTrue(
            "Compact Exchange must not wake ACK recovery for unrelated open documents",
            !source.contains("saveAllDocuments()"),
        )
    }
}
