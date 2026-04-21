package com.github.btakita.agentdoc

import org.junit.Assert.*
import org.junit.Test
import java.io.File

class LibMtimeChangedTest {

    @Test
    fun `returns false when mtime unchanged`() {
        val tmp = File.createTempFile("libagent_doc_test", ".so")
        try {
            assertFalse(libMtimeChanged(tmp.absolutePath, tmp.lastModified()))
        } finally {
            tmp.delete()
        }
    }

    @Test
    fun `returns true when file modified`() {
        val tmp = File.createTempFile("libagent_doc_test", ".so")
        try {
            val oldMtime = tmp.lastModified()
            Thread.sleep(1100)
            tmp.writeText("updated")
            assertTrue(libMtimeChanged(tmp.absolutePath, oldMtime))
        } finally {
            tmp.delete()
        }
    }

    @Test
    fun `returns false for nonexistent file`() {
        assertFalse(libMtimeChanged("/tmp/nonexistent_lib_${System.nanoTime()}.so", 99999L))
    }

    @Test
    fun `returns false when storedMtime matches current`() {
        val tmp = File.createTempFile("libagent_doc_test", ".so")
        try {
            tmp.writeText("content")
            val mtime = tmp.lastModified()
            assertFalse(libMtimeChanged(tmp.absolutePath, mtime))
        } finally {
            tmp.delete()
        }
    }
}
