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
        val saveIdx = source.indexOf("fdm.saveDocument(it)")
        val attachIdx = source.indexOf("CrdtReplicaManager.ensureReplicaForOpenDocument(")
        val compactIdx = source.indexOf("TerminalUtil.compactExchange(project, file)", attachIdx)

        assertTrue("Compact Exchange should resolve its target document", lookupIdx >= 0)
        assertTrue("Compact Exchange should save only that document", saveIdx > lookupIdx)
        assertTrue("Compact Exchange should prove editor attachment after saving", attachIdx > saveIdx)
        assertTrue("Compact Exchange should route only after editor attachment", compactIdx > attachIdx)
        assertTrue(
            "the attachment proof must wait off the EDT before the command can infer disk authority",
            source.contains("await = true"),
        )
        assertTrue(
            "a missing editor replica must fail before launching Compact Exchange",
            source.contains("if (!attached)"),
        )
        assertTrue(
            "Compact Exchange must not wake ACK recovery for unrelated open documents",
            !source.contains("saveAllDocuments()"),
        )
    }
}
