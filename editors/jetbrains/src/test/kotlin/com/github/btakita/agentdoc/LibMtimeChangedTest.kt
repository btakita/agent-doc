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

    @Test
    fun `shadow copy produces a distinct per-mtime path with matching contents`() {
        val src = File.createTempFile("libagent_doc_src", ".so")
        val cacheRoot = File(System.getProperty("java.io.tmpdir"), "agent-doc-native-test-${System.nanoTime()}")
        try {
            src.writeText("native-code-v1")
            val mtime = src.lastModified()
            val shadow = nativeShadowCopyPath(src.absolutePath, mtime, cacheRoot)
            assertNotNull(shadow)
            // Loads from a NEW path (distinct inode) so dlopen actually reloads.
            assertNotEquals(src.absolutePath, shadow)
            assertTrue(shadow!!.contains("libagent_doc-$mtime."))
            assertEquals("native-code-v1", File(shadow).readText())
        } finally {
            cacheRoot.deleteRecursively()
            src.delete()
        }
    }

    @Test
    fun `shadow copy on a new install prunes the prior stale copy`() {
        val src = File.createTempFile("libagent_doc_src", ".so")
        val cacheRoot = File(System.getProperty("java.io.tmpdir"), "agent-doc-native-test-${System.nanoTime()}")
        try {
            src.writeText("native-code-v1")
            val firstMtime = 1_000_000L
            val first = nativeShadowCopyPath(src.absolutePath, firstMtime, cacheRoot)
            assertNotNull(first)

            // Simulate a new install: different content + a new mtime.
            src.writeText("native-code-v2-longer")
            val secondMtime = 2_000_000L
            val second = nativeShadowCopyPath(src.absolutePath, secondMtime, cacheRoot)
            assertNotNull(second)

            assertNotEquals(first, second)
            assertEquals("native-code-v2-longer", File(second!!).readText())
            // The prior stale shadow copy is pruned so it cannot be reloaded.
            assertFalse(File(first!!).exists())
        } finally {
            cacheRoot.deleteRecursively()
            src.delete()
        }
    }

    @Test
    fun `shadow copy reuses the same path for an unchanged install`() {
        val src = File.createTempFile("libagent_doc_src", ".so")
        val cacheRoot = File(System.getProperty("java.io.tmpdir"), "agent-doc-native-test-${System.nanoTime()}")
        try {
            src.writeText("native-code-v1")
            val mtime = src.lastModified()
            val first = nativeShadowCopyPath(src.absolutePath, mtime, cacheRoot)
            val second = nativeShadowCopyPath(src.absolutePath, mtime, cacheRoot)
            assertEquals(first, second)
        } finally {
            cacheRoot.deleteRecursively()
            src.delete()
        }
    }
}
