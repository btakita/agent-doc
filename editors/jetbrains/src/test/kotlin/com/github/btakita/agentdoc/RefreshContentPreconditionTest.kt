package com.github.btakita.agentdoc

import org.junit.Assert.*
import org.junit.Test

class RefreshContentPreconditionTest {

    @Test
    fun `accepts exact stale buffer proof`() {
        val stale = "stale editor buffer"
        val target = "committed HEAD buffer"

        assertTrue(
            refreshContentPreconditionUtil(
                stale,
                target,
                PatchWatcher.contentHash(stale),
                stale.length,
            ),
        )
    }

    @Test
    fun `accepts no-op when editor already matches committed content`() {
        val target = "committed HEAD buffer"

        assertTrue(
            refreshContentPreconditionUtil(
                target,
                target,
                PatchWatcher.contentHash("old stale buffer"),
                16,
            ),
        )
    }

    @Test
    fun `rejects editor content changed after stale proof`() {
        val stale = "stale editor buffer"
        val changed = "$stale plus user edit"
        val target = "committed HEAD buffer"

        assertFalse(
            refreshContentPreconditionUtil(
                changed,
                target,
                PatchWatcher.contentHash(stale),
                stale.length,
            ),
        )
    }
}
