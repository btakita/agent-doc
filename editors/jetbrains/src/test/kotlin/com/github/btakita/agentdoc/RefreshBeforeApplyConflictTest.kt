package com.github.btakita.agentdoc

import java.nio.file.Files
import java.nio.file.Paths
import org.junit.Assert.*
import org.junit.Test

/**
 * `#p2j4` / `#jbcfdiag` — pin the invariant that a content-bearing
 * `VirtualFile.refresh` never runs against an UNSAVED document on an agent
 * write/apply path. A refresh against an unsaved buffer whose disk bytes diverged
 * is what arms IntelliJ's memory↔disk "File Cache Conflict" dialog behind a live
 * editor (the remaining trigger after IPC-first writes removed the Rust-side
 * behind-editor disk writes).
 */
class RefreshBeforeApplyConflictTest {

    @Test
    fun `refresh decision skips unsaved buffers and allows clean buffers`() {
        // Clean (saved) buffer: refreshing from disk is safe — no memory↔disk
        // divergence the platform can turn into a dialog.
        assertTrue(shouldRefreshVfsBeforeApplyUtil(false))
        // Unsaved buffer: skip the refresh — the apply path reconciles via the
        // Document API; a bare refresh here would arm the File Cache Conflict dialog.
        assertFalse(shouldRefreshVfsBeforeApplyUtil(true))
    }

    @Test
    fun `apply-time refresh sites are gated on document save state`() {
        val patchWatcherPath = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt"),
        ).first { Files.exists(it) }
        val patchWatcher = Files.readString(patchWatcherPath)

        // Every content-bearing apply-time refresh must be wrapped by the
        // shouldRefreshVfsBeforeApplyUtil gate. Count bare refresh calls that are
        // NOT immediately preceded (within the same guard line above) by the gate.
        val refreshToken = "targetFile.refresh(false, false)"
        val gateToken = "shouldRefreshVfsBeforeApplyUtil"

        // There must be at least as many gate uses as apply-time refresh sites.
        val refreshCount = patchWatcher.split(refreshToken).size - 1
        val gateCount = patchWatcher.split(gateToken).size - 1
        assertTrue(
            "expected at least 3 apply-time refresh sites, found $refreshCount",
            refreshCount >= 3,
        )
        // 3 call-sites gated + 1 function definition reference => >= 4 occurrences.
        assertTrue(
            "expected the refresh gate to wrap apply-time refresh sites (found $gateCount uses)",
            gateCount >= refreshCount + 1,
        )

        // Each apply-time refresh must be immediately preceded by the gate in an
        // `if (...)` guard, never a bare unconditional call before an apply.
        assertEveryRefreshGated(patchWatcher, refreshToken, gateToken)
    }

    /**
     * `#p2j4` / `#jbcfdiag` — PromptPoller's auto-save / refresh loop runs on the
     * 1.5s poll timer regardless of agent activity. Its content-bearing
     * `VirtualFile.refresh(false, false)` calls were the dominant behind-editor File
     * Cache Conflict trigger (fired every cycle, not just during an apply). Pin that
     * both poll-timer refresh sites are gated by shouldRefreshVfsBeforeApplyUtil so a
     * refresh never runs against an unsaved buffer.
     */
    @Test
    fun `prompt-poller timer refresh sites are gated on document save state`() {
        val pollerPath = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/PromptPoller.kt"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/PromptPoller.kt"),
        ).first { Files.exists(it) }
        val poller = Files.readString(pollerPath)

        val gateToken = "shouldRefreshVfsBeforeApplyUtil"
        // PromptPoller refreshes `tracked.file` / `file`, not `targetFile`.
        val refreshTokens = listOf(
            "tracked.file.refresh(false, false)",
            "file.refresh(false, false)",
        )
        val refreshCount = refreshTokens.sumOf { token -> poller.split(token).size - 1 }
        assertTrue(
            "expected at least 2 poll-timer refresh sites, found $refreshCount",
            refreshCount >= 2,
        )
        for (token in refreshTokens) {
            assertEveryRefreshGated(poller, token, gateToken)
        }
    }

    private fun assertEveryRefreshGated(source: String, refreshToken: String, gateToken: String) {
        var idx = source.indexOf(refreshToken)
        while (idx >= 0) {
            // Look back a small window for the gate `if`.
            val windowStart = (idx - 200).coerceAtLeast(0)
            val window = source.substring(windowStart, idx)
            assertTrue(
                "content-bearing refresh at offset $idx is not gated by shouldRefreshVfsBeforeApplyUtil:\n$window",
                window.contains("if (") && window.contains(gateToken),
            )
            idx = source.indexOf(refreshToken, idx + refreshToken.length)
        }
    }
}
