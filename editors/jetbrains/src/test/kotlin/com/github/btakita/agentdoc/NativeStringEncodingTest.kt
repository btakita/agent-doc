package com.github.btakita.agentdoc

import com.sun.jna.Library
import com.sun.jna.Native
import com.sun.jna.Platform
import com.sun.jna.Pointer
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test
import java.io.File
import java.lang.reflect.Proxy
import java.nio.charset.StandardCharsets
import kotlin.random.Random

/**
 * GH #274: the agent-doc JNA boundary is byte-exact UTF-8 regardless of the JVM locale.
 *
 * The defect reproduced here: with JNA's default string encoding at US-ASCII (what an IDE
 * backend under `locale=en_US` / `sun.jnu.encoding=ANSI_X3.4-1968` gets), a `String` argument
 * reaches native code with every non-ASCII character replaced by `?`.
 */
class NativeStringEncodingTest {
    interface CLib : Library {
        fun strlen(text: String): Long

        fun strdup(text: String): Pointer

        fun free(ptr: Pointer)
    }

    private val issueLine =
        "### Re: upgrade to 0.35.508 and viewpane issue dispatched — opus-5.5 · " +
            "2026-10-15T11:20-04:00 — 0.35.505 → 0.35.508, gh 261–267 ● ❯ 😀"

    private fun <T> withAsciiJnaDefault(block: () -> T): T {
        val previous = System.getProperty("jna.encoding")
        System.setProperty("jna.encoding", "US-ASCII")
        try {
            return block()
        } finally {
            if (previous == null) System.clearProperty("jna.encoding") else System.setProperty("jna.encoding", previous)
        }
    }

    private fun libc(): CLib {
        val handler = Library.Handler(Platform.C_LIBRARY_NAME, CLib::class.java, NATIVE_LIBRARY_OPTIONS)
        return Proxy.newProxyInstance(CLib::class.java.classLoader, arrayOf(CLib::class.java), handler) as CLib
    }

    @Test
    fun `an ASCII JNA default is the lossy encoder from the issue`() {
        withAsciiJnaDefault {
            assertEquals("US-ASCII", Native.getDefaultStringEncoding())
            val lossy = String(Native.toByteArray(issueLine), StandardCharsets.UTF_8).trimEnd('\u0000')
            assertTrue(lossy, lossy.contains("dispatched ? opus-5.5 ? 2026-10-15T11:20-04:00"))
            assertTrue(lossy, lossy.contains("0.35.505 ? 0.35.508"))
        }
    }

    @Test
    fun `the agent-doc binding crosses non-ASCII text byte-exact under an ASCII default`() {
        withAsciiJnaDefault {
            val lib = libc()
            assertEquals(
                issueLine.toByteArray(StandardCharsets.UTF_8).size.toLong(),
                lib.strlen(issueLine),
            )
            val copy = lib.strdup(issueLine)
            try {
                assertEquals(issueLine, copy.readNativeUtf8())
                assertFalse(
                    "an un-encoded getString is the lossy decode readNativeUtf8 replaces",
                    issueLine == copy.getString(0),
                )
            } finally {
                lib.free(copy)
            }
        }
    }

    @Test
    fun `property - random non-ASCII documents round-trip through the binding`() {
        val alphabet =
            listOf(
                "a", "Z", "0", " ", "\n", "#", "`", "?", "—", "·", "→", "–",
                "●", "❯", "あ", "😀", "🤖", "é",
            )
        withAsciiJnaDefault {
            val lib = libc()
            val random = Random(274)
            repeat(500) { iteration ->
                val text = buildString { repeat(random.nextInt(1, 80)) { append(alphabet[random.nextInt(alphabet.size)]) } }
                assertEquals(
                    "iteration $iteration byte length",
                    text.toByteArray(StandardCharsets.UTF_8).size.toLong(),
                    lib.strlen(text),
                )
                val copy = lib.strdup(text)
                try {
                    assertEquals("iteration $iteration round trip", text, copy.readNativeUtf8())
                } finally {
                    lib.free(copy)
                }
            }
        }
    }

    @Test
    fun `an unpaired surrogate fails closed instead of crossing as a question mark`() {
        assertTrue(isWellFormedUtf16(issueLine))
        assertTrue(isWellFormedUtf16(""))
        assertFalse(isWellFormedUtf16("a\uD83Db"))
        assertFalse(isWellFormedUtf16("tail\uD83D"))
        assertFalse(isWellFormedUtf16("\uDE00lead"))

        requireLosslessNativeStringArgs("agent_doc_replica_apply_local", arrayOf(1L, 0, 0, issueLine))
        requireLosslessNativeStringArgs("agent_doc_free_string", null)
        try {
            requireLosslessNativeStringArgs("agent_doc_replica_apply_local", arrayOf(1L, 0, 0, "a\uD83Db"))
            fail("an unpaired surrogate must refuse the native call")
        } catch (error: LossyNativeStringException) {
            assertTrue(error.message!!.contains("agent_doc_replica_apply_local"))
            assertTrue(error.message!!.contains("argument 3"))
        }
    }

    @Test
    fun `invalid UTF-8 from native code fails closed instead of decoding to U+FFFD`() {
        val memory = com.sun.jna.Memory(4)
        memory.write(0, byteArrayOf('a'.code.toByte(), 0xB7.toByte(), 'b'.code.toByte(), 0), 0, 4)
        try {
            memory.readNativeUtf8()
            fail("a Latin-1 byte must not decode")
        } catch (_: java.nio.charset.CharacterCodingException) {
        }
    }

    @Test
    fun `every native string crossing in the plugin goes through the UTF-8 rule`() {
        val main = File("src/main/kotlin/com/github/btakita/agentdoc")
        val offenders =
            main.walkTopDown()
                .filter { it.isFile && it.extension == "kt" }
                .flatMap { file ->
                    file.readLines().mapIndexedNotNull { index, line ->
                        val trimmed = line.trim()
                        val crossing =
                            trimmed.contains(".getString(") ||
                                trimmed.contains("Native.load(") ||
                                trimmed.contains("Native.toString(") ||
                                (trimmed.contains("Library.Handler(") && !trimmed.contains("NATIVE_LIBRARY_OPTIONS"))
                        if (crossing && !trimmed.startsWith("*") && !trimmed.startsWith("//")) {
                            "${file.name}:${index + 1}: $trimmed"
                        } else {
                            null
                        }
                    }
                }.toList()
        assertTrue("lossy-capable JNA string crossings:\n${offenders.joinToString("\n")}", offenders.isEmpty())

        val native = File(main, "NativeLib.kt").readText()
        assertTrue(native.contains("Library.Handler(path, AgentDocLib::class.java, NATIVE_LIBRARY_OPTIONS)"))
        assertTrue(native.contains("requireLosslessNativeStringArgs(method.name, args)"))
    }
}
