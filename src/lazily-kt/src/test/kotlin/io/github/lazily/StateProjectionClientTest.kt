package io.github.lazily

import com.sun.jna.Pointer
import kotlinx.coroutines.test.runTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

/**
 * Tests for [StateProjectionClient] using a mock FFI.
 *
 * The real FFI requires the `agent_doc` native library to be loaded;
 * these tests verify the client logic (StateFlow updates, null handling,
 * free lifecycle) without a native dependency.
 */
class StateProjectionClientTest {

    private class MockFFI : LazilyFFI {
        var projectionJson = "null"
        var recordedEvent: String? = null
        var recordResult = 1
        var freeCallCount = 0

        override fun agent_doc_state_projection(documentHash: String): Pointer {
            val bytes = projectionJson.toByteArray(Charsets.UTF_8)
            val mem = com.sun.jna.Memory(bytes.size + 1L)
            mem.write(0, bytes, 0, bytes.size)
            mem.setByte(bytes.size.toLong(), 0)
            return mem
        }

        override fun agent_doc_record_state_event(documentHash: String, factJson: String): Int {
            recordedEvent = factJson
            return recordResult
        }

        override fun agent_doc_free_string(ptr: Pointer) {
            freeCallCount++
        }
    }

    @Test
    fun `refresh updates StateFlow with projection JSON`() = runTest {
        val mock = MockFFI()
        mock.projectionJson = """{"document":{"hash":"abc123"}}"""
        val client = StateProjectionClient("abc123", mock)

        assertFalse(client.projection.value.isAvailable)

        client.refresh()

        assertTrue(client.projection.value.isAvailable)
        assertEquals("""{"document":{"hash":"abc123"}}""", client.projection.value.json)
        assertEquals(1, mock.freeCallCount)
    }

    @Test
    fun `refresh with null projection keeps isAvailable false`() = runTest {
        val mock = MockFFI()
        mock.projectionJson = "null"
        val client = StateProjectionClient("noevents", mock)

        client.refresh()

        assertFalse(client.projection.value.isAvailable)
        assertEquals(null, client.projection.value.json)
        assertEquals(1, mock.freeCallCount)
    }

    @Test
    fun `recordStateEvent passes fact JSON and returns true on success`() = runTest {
        val mock = MockFFI()
        mock.recordResult = 1
        val client = StateProjectionClient("doc1", mock)

        val accepted = client.recordStateEvent("""{"type":"BaselineSaved"}""")

        assertTrue(accepted)
        assertEquals("""{"type":"BaselineSaved"}""", mock.recordedEvent)
    }

    @Test
    fun `recordStateEvent returns false on failure`() = runTest {
        val mock = MockFFI()
        mock.recordResult = 0
        val client = StateProjectionClient("doc1", mock)

        val accepted = client.recordStateEvent("""{"type":"Invalid"}""")

        assertFalse(accepted)
    }
}
