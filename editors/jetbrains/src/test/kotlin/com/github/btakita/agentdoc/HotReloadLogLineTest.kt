package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Test

/**
 * `#hotreloadversion` (btakita/agent-doc#59): an install replaced the library under the IDE and
 * the log read `hot-reloaded libagent_doc vmtime`, because the trigger rode in the version
 * parameter. The rendered line is asserted per trigger so the two cannot swap places again.
 */
class HotReloadLogLineTest {
    private val path = "/home/u/.cargo/bin/libagent_doc.so"

    @Test
    fun `an mtime reload names the version that loaded and the trigger`() {
        val line = hotReloadLogLineUtil("0.35.418", NATIVE_RELOAD_TRIGGER_MTIME, null, path)
        assertEquals(
            "[native] hot-reloaded libagent_doc v0.35.418 (trigger=mtime) from $path after quiesce/close handoff",
            line,
        )
        assertFalse(line.contains("vmtime"))
    }

    @Test
    fun `an ipc reload whose announced version loaded does not repeat it`() {
        assertEquals(
            "[native] hot-reloaded libagent_doc v0.35.418 (trigger=ipc) from $path after quiesce/close handoff",
            hotReloadLogLineUtil("0.35.418", NATIVE_RELOAD_TRIGGER_IPC, "0.35.418", path),
        )
    }

    @Test
    fun `an ipc reload that loaded a different version than announced says so`() {
        assertEquals(
            "[native] hot-reloaded libagent_doc v0.35.418 (trigger=ipc announced=v0.35.419) from $path " +
                "after quiesce/close handoff",
            hotReloadLogLineUtil("0.35.418", NATIVE_RELOAD_TRIGGER_IPC, "0.35.419", path),
        )
    }

    @Test
    fun `a missing version is reported as unknown, never as a trigger`() {
        assertEquals(
            "[native] hot-reloaded libagent_doc version=unknown (trigger=mtime) from $path " +
                "after quiesce/close handoff",
            hotReloadLogLineUtil(null, NATIVE_RELOAD_TRIGGER_MTIME, null, path),
        )
    }
}
