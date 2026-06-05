package com.github.btakita.agentdoc

import java.nio.file.Files
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class RunAgentDocAttemptLedgerTest {
    @Test
    fun `attempt ledger writes latest stage and append-only history`() {
        val cwd = Files.createTempDirectory("agent-doc-jb-route-attempt").toFile()
        val attempt = RunAgentDocAttemptLedger.begin(
            cwd = cwd.path,
            relativePath = "tasks/root.md",
            filePath = "${cwd.path}/tasks/root.md",
            focusedFile = "${cwd.path}/tasks/root.md",
        )

        attempt.recordIfCurrent("await_typing_idle")
        attempt.finishIfCurrent("typing_idle_timeout", error = "mtime did not settle")

        val diagnostics = RunAgentDocAttemptLedger.attemptDiagnosticsFile(cwd.path, "tasks/root.md")
        val text = diagnostics.readText()
        assertTrue(text.contains("attempt_id=${attempt.id}"))
        assertTrue(text.contains("relative_path=tasks/root.md"))
        assertTrue(text.contains("stage=typing_idle_timeout"))
        assertTrue(text.contains("error=mtime did not settle"))
        assertTrue(text.contains("stage=clicked"))
        assertTrue(text.contains("stage=await_typing_idle"))
    }

    @Test
    fun `superseded attempts cannot overwrite the newer route key snapshot`() {
        val cwd = Files.createTempDirectory("agent-doc-jb-route-supersede").toFile()
        val first = RunAgentDocAttemptLedger.begin(
            cwd = cwd.path,
            relativePath = "tasks/root.md",
            filePath = "${cwd.path}/tasks/root.md",
            focusedFile = "${cwd.path}/tasks/root.md",
        )
        val second = RunAgentDocAttemptLedger.begin(
            cwd = cwd.path,
            relativePath = "tasks/root.md",
            filePath = "${cwd.path}/tasks/root.md",
            focusedFile = "${cwd.path}/tasks/root.md",
        )

        first.finishIfCurrent("route_failed", error = "stale failure")

        val diagnostics = RunAgentDocAttemptLedger.attemptDiagnosticsFile(cwd.path, "tasks/root.md")
        val text = diagnostics.readText()
        assertTrue(text.contains("attempt_id=${second.id}"))
        assertTrue(text.contains("previous_attempt_id=${first.id}"))
        assertTrue(text.contains("stage=clicked"))
        assertTrue(text.contains("stage=superseded"))
        assertFalse(text.contains("stale failure"))
    }
}
