package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * `#jbinputrequiredstale`: regression coverage for the input-required balloon lifecycle.
 *
 * Before the fix the notification was fire-and-forget, so it outlived the `WaitingInput` state that
 * justified it and every later edge stacked another balloon beside the stale one.
 */
class InputRequiredNotificationsTest {
    private class Handle(val id: Int) {
        var expired = false
    }

    private fun notifications(): Pair<InputRequiredNotifications<Handle>, MutableList<Handle>> {
        val expired = mutableListOf<Handle>()
        val subject =
            InputRequiredNotifications<Handle> {
                it.expired = true
                expired.add(it)
            }
        return subject to expired
    }

    @Test
    fun `raises one notification per document`() {
        val (subject, _) = notifications()
        var created = 0

        assertTrue(subject.raise("/docs/contracts.md") { Handle(created++) })
        assertTrue(subject.isLive("/docs/contracts.md"))
        assertEquals(1, created)
    }

    @Test
    fun `does not stack a second balloon while one is still live`() {
        val (subject, expired) = notifications()
        var created = 0

        subject.raise("/docs/contracts.md") { Handle(created++) }
        val second = subject.raise("/docs/contracts.md") { Handle(created++) }

        assertFalse("a second balloon must not be created", second)
        assertEquals("create must not be invoked again", 1, created)
        assertTrue("nothing should have been expired", expired.isEmpty())
    }

    @Test
    fun `clear expires the live notification so it stops claiming input is required`() {
        val (subject, expired) = notifications()
        val handle = Handle(0)

        subject.raise("/docs/contracts.md") { handle }
        subject.clear("/docs/contracts.md")

        assertTrue("the balloon must be retracted", handle.expired)
        assertEquals(listOf(handle), expired)
        assertFalse(subject.isLive("/docs/contracts.md"))
    }

    @Test
    fun `clear is a no-op when nothing is outstanding`() {
        val (subject, expired) = notifications()

        subject.clear("/docs/contracts.md")

        assertTrue(expired.isEmpty())
    }

    @Test
    fun `a document can raise again after its previous notification was retracted`() {
        val (subject, expired) = notifications()
        val first = Handle(0)
        val second = Handle(1)

        subject.raise("/docs/contracts.md") { first }
        subject.clear("/docs/contracts.md")
        assertTrue(subject.raise("/docs/contracts.md") { second })

        assertEquals(listOf(first), expired)
        assertFalse(second.expired)
    }

    @Test
    fun `documents are tracked independently`() {
        val (subject, _) = notifications()
        val contracts = Handle(0)
        val backend = Handle(1)

        subject.raise("/docs/contracts.md") { contracts }
        subject.raise("/docs/backend.md") { backend }
        subject.clear("/docs/contracts.md")

        assertTrue(contracts.expired)
        assertFalse("an unrelated document must keep its balloon", backend.expired)
        assertTrue(subject.isLive("/docs/backend.md"))
    }

    @Test
    fun `clearAll retracts every outstanding notification on dispose`() {
        val (subject, expired) = notifications()
        val contracts = Handle(0)
        val backend = Handle(1)

        subject.raise("/docs/contracts.md") { contracts }
        subject.raise("/docs/backend.md") { backend }
        subject.clearAll()

        assertEquals(2, expired.size)
        assertTrue(contracts.expired)
        assertTrue(backend.expired)
        assertFalse(subject.isLive("/docs/contracts.md"))
        assertFalse(subject.isLive("/docs/backend.md"))
    }

    @Test
    fun `project bus keeps only the newest equivalent notification across plugin generations`() {
        val expired = mutableListOf<Handle>()
        val subject =
            NotificationBusDeduplicator<Handle, String>(
                keyOf = { "Agent input required for session.md" },
                expire = {
                    it.expired = true
                    expired.add(it)
                },
            )
        val firstGeneration = Handle(1)
        val secondGeneration = Handle(2)
        val thirdGeneration = Handle(3)

        assertFalse(subject.observe(firstGeneration))
        assertTrue(subject.observe(secondGeneration))
        assertTrue(subject.observe(thirdGeneration))

        assertEquals(listOf(firstGeneration, secondGeneration), expired)
        assertTrue(firstGeneration.expired)
        assertTrue(secondGeneration.expired)
        assertFalse("the newest notification must remain visible", thirdGeneration.expired)
    }

    @Test
    fun `project bus ignores unrelated notices and keeps documents independent`() {
        val expired = mutableListOf<Handle>()
        val keys = mutableMapOf<Int, String?>()
        val subject =
            NotificationBusDeduplicator<Handle, String>(
                keyOf = { keys[it.id] },
                expire = {
                    it.expired = true
                    expired.add(it)
                },
            )
        val contracts = Handle(1).also { keys[it.id] = "contracts.md" }
        val backend = Handle(2).also { keys[it.id] = "backend.md" }
        val unrelated = Handle(3).also { keys[it.id] = null }

        assertFalse(subject.observe(contracts))
        assertFalse(subject.observe(backend))
        assertFalse(subject.observe(unrelated))

        assertTrue(expired.isEmpty())
    }

    @Test
    fun `startup inventory can reobserve the bus winner without expiring it`() {
        val expired = mutableListOf<Handle>()
        val subject =
            NotificationBusDeduplicator<Handle, String>(
                keyOf = { "session.md" },
                expire = expired::add,
            )
        val notification = Handle(1)

        assertFalse(subject.observe(notification))
        assertFalse(subject.observe(notification))

        assertTrue(expired.isEmpty())
    }

    @Test
    fun `disposing a bus observer forgets foreign notices without retracting them`() {
        val expired = mutableListOf<Handle>()
        val subject =
            NotificationBusDeduplicator<Handle, String>(
                keyOf = { "session.md" },
                expire = expired::add,
            )
        val notification = Handle(1)
        val laterNotification = Handle(2)

        subject.observe(notification)
        subject.forgetAll()
        assertFalse(subject.observe(laterNotification))

        assertTrue(expired.isEmpty())
        assertFalse(notification.expired)
        assertFalse(laterNotification.expired)
    }
}
