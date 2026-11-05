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

/** `#restartadoptscanonical`: the durable settled shadow an IDE restart reads back. */
class SettledShadowStoreTest {
    @get:Rule val temp = TemporaryFolder()

    private val doc = "/work/project/session.md"
    private val text = "# Session\n\n<!-- agent:queue -->\n- operator row\n<!-- /agent:queue -->\n"

    private fun store(dir: Path?) = SettledShadowStore { dir }

    @Test
    fun `a saved shadow is read back by a new store, as after an IDE restart`() {
        val dir = temp.newFolder("shadows").toPath()
        assertTrue(store(dir).save(doc, text))
        assertEquals(text, store(dir).load(doc))
        // The latest settle wins.
        val advanced = text + "agent response\n"
        assertTrue(store(dir).save(doc, advanced))
        assertEquals(advanced, store(dir).load(doc))
        // Another document has its own record.
        assertNull(store(dir).load("/work/project/other.md"))
        // No temp files are left behind.
        val files = Files.list(dir).use { listing -> listing.map { it.fileName.toString() }.toList() }
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
        for (sample in listOf("a\r\nb\r\n", "é 😀\n", "")) {
            assertTrue(store(dir).save(doc, sample))
            assertEquals(sample, store(dir).load(doc))
        }
    }

    @Test
    fun `a record that does not prove itself is no record`() {
        val dir = temp.newFolder("invalid").toPath()
        val writer = store(dir)
        assertTrue(writer.save(doc, text))
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
        assertFalse(store(null).save(doc, text))
        assertNull(store(null).load(doc))
        val blocker = temp.newFile("not-a-directory").toPath()
        val blocked = store(blocker.resolve("shadows"))
        assertFalse(blocked.save(doc, text))
        assertNull(blocked.load(doc))
    }
}
