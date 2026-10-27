package com.github.btakita.agentdoc.shell

import com.intellij.openapi.Disposable
import com.intellij.openapi.util.Disposer
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotSame
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test

/**
 * GH #339 R1 Tier A: the platform holds one shell forwarder per key, and that forwarder delivers
 * to every staged handler until the handler's own parent is disposed.
 */
class ShellListenerHubTest {
    interface Probe {
        fun event(value: String)

        /** A default method a later platform could add: the forwarder must still deliver it. */
        fun added(value: String) {
            event("default:$value")
        }
    }

    private val owner = Disposer.newDisposable("shell-hub-test-owner")
    private val hub = ShellListenerHub(owner, "test") { error("this test registers no message bus topic") }
    private val registered = mutableListOf<Probe>()

    @After
    fun tearDown() {
        Disposer.dispose(owner)
    }

    private fun stage(handler: Probe, parent: Disposable) {
        hub.addListener("probe", Probe::class.java, handler, parent) { forwarder, forwarderOwner ->
            assertSame(owner, forwarderOwner)
            registered += forwarder
        }
    }

    @Test
    fun `one platform forwarder serves every handler in staging order`() {
        val seen = mutableListOf<String>()
        val first = Disposer.newDisposable()
        val second = Disposer.newDisposable()
        Disposer.register(owner, first)
        Disposer.register(owner, second)
        stage(object : Probe { override fun event(value: String) { seen += "a:$value" } }, first)
        stage(object : Probe { override fun event(value: String) { seen += "b:$value" } }, second)

        assertEquals("the platform sees one forwarder for the key", 1, registered.size)
        assertEquals(1, hub.forwarderCount())
        registered.single().event("x")
        registered.single().added("y")
        assertEquals(listOf("a:x", "b:x", "a:default:y", "b:default:y"), seen)
    }

    @Test
    fun `disposing a handler's parent removes only that handler`() {
        val seen = mutableListOf<String>()
        val first = Disposer.newDisposable()
        val second = Disposer.newDisposable()
        Disposer.register(owner, first)
        Disposer.register(owner, second)
        stage(object : Probe { override fun event(value: String) { seen += "a:$value" } }, first)
        stage(object : Probe { override fun event(value: String) { seen += "b:$value" } }, second)

        Disposer.dispose(first)
        assertEquals(1, hub.handlerCount("probe"))
        registered.single().event("x")
        assertEquals(listOf("b:x"), seen)
    }

    @Test
    fun `a disposed parent refuses a new handler like a direct registration did`() {
        val parent = Disposer.newDisposable()
        Disposer.dispose(parent)
        try {
            stage(object : Probe { override fun event(value: String) = Unit }, parent)
            fail("a disposed parent must refuse the handler")
        } catch (expected: RuntimeException) {
            assertEquals(0, hub.handlerCount("probe"))
        }
    }

    @Test
    fun `every handler sees the event when one throws and the first failure is rethrown`() {
        val seen = mutableListOf<String>()
        val parent = Disposer.newDisposable()
        Disposer.register(owner, parent)
        stage(object : Probe { override fun event(value: String) = throw IllegalStateException("first") }, parent)
        stage(object : Probe { override fun event(value: String) { seen += value } }, parent)
        stage(object : Probe { override fun event(value: String) = throw IllegalArgumentException("second") }, parent)

        try {
            registered.single().event("x")
            fail("the first handler failure must reach the platform")
        } catch (failure: IllegalStateException) {
            assertEquals("first", failure.message)
            assertTrue(failure.suppressed.single() is IllegalArgumentException)
        }
        assertEquals(listOf("x"), seen)
    }

    @Test
    fun `the forwarder is a shell object with identity semantics`() {
        val parent = Disposer.newDisposable()
        Disposer.register(owner, parent)
        stage(object : Probe { override fun event(value: String) = Unit }, parent)
        val forwarder = registered.single()
        assertSame(ShellListenerHub::class.java.classLoader, forwarder.javaClass.classLoader)
        assertTrue(forwarder == forwarder)
        assertNotSame(forwarder, Any())
        assertTrue(forwarder.toString().contains("agent-doc shell forwarder"))
    }
}
