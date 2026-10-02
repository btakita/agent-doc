package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Test

/**
 * GH #94: a project initialized by one plugin generation must be re-initializable after a
 * dynamic-unload release. The old one-shot `initialized` flag made the still-live generation's
 * restore a no-op after an aborted upgrade, leaving its replica transport deregistered.
 */
class InitializationScopeTest {
    private class Scope(val id: Int) {
        var closed = false
    }

    @Test
    fun `a second begin while initialized is refused`() {
        var opened = 0
        val scope = InitializationScope(open = { Scope(++opened) }, close = { it.closed = true })

        assertNotNull(scope.begin())
        assertNull(scope.begin())
        assertEquals(1, opened)
    }

    @Test
    fun `ending the scope closes its listeners and allows a rebuild`() {
        var opened = 0
        val scope = InitializationScope(open = { Scope(++opened) }, close = { it.closed = true })
        val first = scope.begin()!!

        scope.end()
        val second = scope.begin()

        assertEquals(true, first.closed)
        assertNotNull("the restore after an aborted upgrade must re-initialize", second)
        assertEquals(2, second!!.id)
        assertEquals(false, second.closed)
    }

    @Test
    fun `ending twice closes once and forget leaves closing to the owner`() {
        val closed = mutableListOf<Scope>()
        val scope = InitializationScope(open = { Scope(0) }, close = { closed += it })
        val first = scope.begin()!!

        scope.end()
        scope.end()
        assertEquals(listOf(first), closed)

        scope.begin()
        scope.forget()
        assertEquals(1, closed.size)
        assertSame(first, closed.single())
    }
}
