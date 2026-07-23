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
    fun `shadow copy keeps prior generation until explicit close handoff`() {
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
            // The old mapping is not unlinked while it may still be loaded.
            assertTrue(File(first!!).exists())
        } finally {
            cacheRoot.deleteRecursively()
            src.delete()
        }
    }

    @Test
    fun `mapped shadow detection distinguishes live deleted and unrelated paths`() {
        val path = "/tmp/agent-doc-native-42/libagent_doc-123.so"
        val maps = """
            7f000000-7f001000 r--p 00000000 00:00 0 $path
            7f002000-7f003000 r-xp 00000000 00:00 0 /tmp/unrelated.so
        """.trimIndent()
        assertTrue(nativePathIsMapped(path, maps))
        assertTrue(nativePathIsMapped(path, maps.replace(path, "$path (deleted)")))
        assertFalse(nativePathIsMapped("/tmp/missing.so", maps))
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

    @Test
    fun `reload transition table is exhaustive at the publish boundary`() {
        assertEquals(
            NativeReloadTransition.KeepCurrent,
            nativeReloadTransition(10L, 10L, nativeQuiesced = true, callsDrained = true),
        )
        assertEquals(
            NativeReloadTransition.KeepCurrent,
            nativeReloadTransition(10L, 0L, nativeQuiesced = true, callsDrained = true),
        )
        assertEquals(
            NativeReloadTransition.RetainOldGeneration,
            nativeReloadTransition(10L, 11L, nativeQuiesced = false, callsDrained = true),
        )
        assertEquals(
            NativeReloadTransition.RetainOldGeneration,
            nativeReloadTransition(10L, 11L, nativeQuiesced = true, callsDrained = false),
        )
        assertEquals(
            NativeReloadTransition.RetainOldGeneration,
            nativeReloadTransition(
                10L,
                11L,
                nativeQuiesced = true,
                callsDrained = true,
                replacementReady = false,
            ),
        )
        assertEquals(
            NativeReloadTransition.PublishReplacement,
            nativeReloadTransition(10L, 11L, nativeQuiesced = true, callsDrained = true),
        )
    }

    @Test
    fun `retired generation transition restores old code when dlclose leaves it mapped`() {
        assertEquals(
            NativeRetiredGenerationTransition.LoadReplacement,
            nativeRetiredGenerationTransition(oldGenerationUnmapped = true),
        )
        assertEquals(
            NativeRetiredGenerationTransition.RestoreOldForRestart,
            nativeRetiredGenerationTransition(oldGenerationUnmapped = false),
        )
    }
}
