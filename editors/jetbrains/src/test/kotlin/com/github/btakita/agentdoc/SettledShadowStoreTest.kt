package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import java.nio.file.Files
import java.nio.file.Path
import java.nio.file.attribute.FileTime
import java.util.concurrent.TimeUnit

/** `#restartadoptscanonical`: the durable settled shadow an IDE restart reads back. */
class SettledShadowStoreTest {
    @get:Rule val temp = TemporaryFolder()

    private val doc = "/work/project/session.md"
    private val text = "# Session\n\n<!-- agent:queue -->\n- operator row\n<!-- /agent:queue -->\n"

    /** A scheduler that only queues: the test decides when (and in which order) steps run. */
    private class ManualScheduler {
        val tasks = mutableListOf<Pair<Long, Runnable>>()

        fun schedule(delayMs: Long, task: Runnable) {
            tasks += delayMs to task
        }

        fun runAll() {
            while (tasks.isNotEmpty()) tasks.removeAt(0).second.run()
        }
    }

    private var now = 1_000_000_000_000L

    private fun store(
        dir: Path?,
        scheduler: ManualScheduler = ManualScheduler(),
        exists: (String) -> Boolean = { true },
    ) = SettledShadowStore(
        directory = { dir },
        scheduler = scheduler::schedule,
        clock = { now },
        documentExists = exists,
    )

    /** Save through [store] and flush, as a settle followed by a project close. */
    private fun saveFlushed(dir: Path?, path: String, value: String): Boolean =
        store(dir).run {
            save(path, value)
            flush()
        }

    private fun records(dir: Path): List<String> =
        Files.list(dir).use { listing -> listing.map { it.fileName.toString() }.toList() }.sorted()

    @Test
    fun `a saved shadow is read back by a new store, as after an IDE restart`() {
        val dir = temp.newFolder("shadows").toPath()
        assertTrue(saveFlushed(dir, doc, text))
        assertEquals(text, store(dir).load(doc))
        // The latest settle wins.
        val advanced = text + "agent response\n"
        assertTrue(saveFlushed(dir, doc, advanced))
        assertEquals(advanced, store(dir).load(doc))
        // Another document has its own record.
        assertNull(store(dir).load("/work/project/other.md"))
        // No temp files are left behind.
        val files = records(dir)
        assertEquals(1, files.size)
        assertTrue(files.single().endsWith(".settled-shadow"))
        // One JSON header line, then the exact text as raw bytes (never escaped into JSON).
        val record = Files.readString(dir.resolve(files.single()))
        assertTrue(record.substringBefore('\n').startsWith("{\"version\":1,"))
        assertEquals(advanced, record.substringAfter('\n'))
    }

    @Test
    fun `exact text survives, including CRLF, astral, and empty documents`() {
        val dir = temp.newFolder("exact").toPath()
        for (sample in listOf("a\r\nb\r\n", "é 😀\n", "")) {
            assertTrue(saveFlushed(dir, doc, sample))
            assertEquals(sample, store(dir).load(doc))
        }
    }

    @Test
    fun `a record that does not prove itself is no record`() {
        val dir = temp.newFolder("invalid").toPath()
        val writer = store(dir)
        writer.save(doc, text)
        assertTrue(writer.flush())
        val record = writer.recordPath(doc)!!
        val valid = Files.readString(record)

        // Text edited behind the store's back: its content hash no longer matches.
        Files.writeString(record, valid.replace("operator row", "tampered row"))
        assertNull(store(dir).load(doc))
        // A record that names a different document.
        Files.writeString(record, valid.replace(doc, "/work/project/elsewhere.md"))
        assertNull(store(dir).load(doc))
        // Another record version.
        Files.writeString(record, valid.replace("\"version\":1", "\"version\":2"))
        assertNull(store(dir).load(doc))
        // A torn or unreadable file.
        Files.writeString(record, valid.substring(0, valid.length / 2))
        assertNull(store(dir).load(doc))
        Files.writeString(record, "")
        assertNull(store(dir).load(doc))
    }

    @Test
    fun `no directory or an unwritable one never throws and persists nothing`() {
        assertFalse(saveFlushed(null, doc, text))
        assertNull(store(null).load(doc))
        val blocker = temp.newFile("not-a-directory").toPath()
        val blocked = blocker.resolve("shadows")
        assertFalse(saveFlushed(blocked, doc, text))
        assertNull(store(blocked).load(doc))
    }

    @Test
    fun `a settle does no disk work on the calling thread`() {
        // #shadowstorecost: the replica worker only records the text and schedules.
        val dir = temp.newFolder("hot").toPath()
        val scheduler = ManualScheduler()
        val shadows = store(dir, scheduler)
        shadows.save(doc, text)
        assertTrue("nothing written before a scheduled step runs", records(dir).isEmpty())
        // Pending is still the answer a registration reads.
        assertEquals(text, shadows.load(doc))
        scheduler.runAll()
        assertEquals(text, store(dir).load(doc))
    }

    @Test
    fun `a burst of settles is one write of the newest, never an older one over a newer`() {
        val dir = temp.newFolder("burst").toPath()
        val scheduler = ManualScheduler()
        val shadows = store(dir, scheduler)
        val versions = (1..50).map { "$text revision $it\n" }
        versions.forEach { shadows.save(doc, it) }
        // One invalidation and one write were queued for the whole burst, plus one collection.
        val writeDelay = SettledShadowStore.writeDelayMs(versions.first().length)
        assertEquals(1, scheduler.tasks.count { it.first == 0L })
        assertEquals(1, scheduler.tasks.count { it.first == writeDelay })
        scheduler.runAll()
        assertEquals(versions.last(), store(dir).load(doc))

        // The steps an older settle scheduled carry the newer settle that coalesced into them,
        // in whichever order they run: each reads the pending shadow when it writes.
        val older = "$text older\n"
        val newer = "$text newer\n"
        shadows.save(doc, older)
        val steps = scheduler.tasks.toList()
        scheduler.tasks.clear()
        shadows.save(doc, newer)
        assertTrue("the newer settle joins the queued steps", scheduler.tasks.isEmpty())
        steps.reversed().forEach { it.second.run() }
        assertEquals(newer, store(dir).load(doc))
        // A late step after the write changes nothing.
        steps.forEach { it.second.run() }
        assertEquals(newer, store(dir).load(doc))
    }

    @Test
    fun `a newer settle removes the older record before its debounced write`() {
        // A crash between a settle and its write leaves no record (the hold), never the older
        // shadow (which a controller that reverted to it would publish over).
        val dir = temp.newFolder("invalidate").toPath()
        val scheduler = ManualScheduler()
        val shadows = store(dir, scheduler)
        shadows.save(doc, text)
        assertTrue(shadows.flush())
        // The steps queued before the flush find nothing pending.
        scheduler.runAll()
        assertEquals(text, store(dir).load(doc))

        val advanced = "$text advanced\n"
        shadows.save(doc, advanced)
        val writeDelay = SettledShadowStore.writeDelayMs(advanced.length)
        scheduler.tasks.filter { it.first == 0L }.forEach { it.second.run() }
        assertNull("the stale record is gone before the write", store(dir).load(doc))
        scheduler.tasks.filter { it.first == writeDelay }.forEach { it.second.run() }
        assertEquals(advanced, store(dir).load(doc))

        // Re-settling the text already on disk (an equal, distinct string) deletes nothing.
        scheduler.tasks.clear()
        shadows.save(doc, String(advanced.toCharArray()))
        scheduler.tasks.filter { it.first == 0L }.forEach { it.second.run() }
        assertEquals(advanced, store(dir).load(doc))
    }

    @Test
    fun `close flushes what is pending and later settles write synchronously`() {
        val dir = temp.newFolder("close").toPath()
        val scheduler = ManualScheduler()
        val shadows = store(dir, scheduler)
        shadows.save(doc, text)
        val queued = scheduler.tasks.toList()
        scheduler.tasks.clear()
        shadows.close()
        assertEquals(text, store(dir).load(doc))
        shadows.save(doc, "$text late\n")
        assertTrue("nothing is scheduled after close", scheduler.tasks.isEmpty())
        assertEquals("$text late\n", store(dir).load(doc))
        // A step scheduled before close is inert afterwards (the directory may be the next
        // plugin generation's by then).
        Files.delete(shadows.recordPath(doc)!!)
        queued.forEach { it.second.run() }
        assertTrue(records(dir).isEmpty())
    }

    @Test
    fun `the write debounce grows with document size and is bounded`() {
        assertEquals(SettledShadowStore.MIN_WRITE_DELAY_MS, SettledShadowStore.writeDelayMs(0))
        assertEquals(SettledShadowStore.MIN_WRITE_DELAY_MS, SettledShadowStore.writeDelayMs(10_000))
        assertEquals(2_000L, SettledShadowStore.writeDelayMs(2_000_000))
        assertEquals(SettledShadowStore.MAX_WRITE_DELAY_MS, SettledShadowStore.writeDelayMs(Int.MAX_VALUE))
    }

    @Test
    fun `collection removes deleted, unopened, corrupt, and leftover records only`() {
        val dir = temp.newFolder("gc").toPath()
        val deleted = "/work/project/deleted.md"
        val stale = "/work/project/stale.md"
        val fresh = "/work/project/fresh.md"
        val live = "/work/project/live.md"
        listOf(deleted, stale, fresh, live).forEach { assertTrue(saveFlushed(dir, it, "$it\n")) }
        val writer = store(dir)
        val day = TimeUnit.DAYS.toMillis(1)
        // stale: last written or read 31 days ago; everything else today.
        Files.setLastModifiedTime(writer.recordPath(stale)!!, FileTime.fromMillis(now - 31 * day))
        // live: also old, but this session already read it (opened), which touches it.
        Files.setLastModifiedTime(writer.recordPath(live)!!, FileTime.fromMillis(now - 31 * day))
        val corrupt = dir.resolve("0".repeat(64) + ".settled-shadow")
        Files.writeString(corrupt, "not a record")
        val leftover = Files.createTempFile(dir, "x.settled-shadow", ".tmp")
        Files.setLastModifiedTime(leftover, FileTime.fromMillis(now - 2 * TimeUnit.HOURS.toMillis(1)))
        val inFlight = Files.createTempFile(dir, "y.settled-shadow", ".tmp")
        Files.setLastModifiedTime(inFlight, FileTime.fromMillis(now))

        val collector = store(dir, exists = { it != deleted })
        assertEquals("$live\n", collector.load(live))
        assertEquals(4, collector.collectGarbage())
        val left = records(dir)
        assertEquals(
            listOf(fresh, live).map { writer.recordPath(it)!!.fileName.toString() }.plus(inFlight.fileName.toString()).sorted(),
            left,
        )
        // A document saved this session survives even when its file is momentarily missing; one
        // nothing touched this session does not.
        val saving = store(dir, exists = { false })
        saving.save(fresh, "$fresh 2\n")
        assertEquals(1, saving.collectGarbage())
        assertTrue(Files.exists(writer.recordPath(fresh)!!))
        assertFalse(Files.exists(writer.recordPath(live)!!))
    }

    @Test
    fun `collection is scheduled once per interval after first use`() {
        val dir = temp.newFolder("gc-schedule").toPath()
        val scheduler = ManualScheduler()
        val shadows = store(dir, scheduler)
        shadows.load(doc)
        shadows.save(doc, text)
        val gcDelay = SettledShadowStore.GC_INITIAL_DELAY_MS
        assertEquals(1, scheduler.tasks.count { it.first == gcDelay })
        scheduler.runAll()
        shadows.save(doc, "$text 2\n")
        assertEquals("no second collection inside the interval", 0, scheduler.tasks.count { it.first == gcDelay })
        now += SettledShadowStore.GC_INTERVAL_MS + 1
        shadows.save(doc, "$text 3\n")
        assertEquals(1, scheduler.tasks.count { it.first == gcDelay })
    }
}
