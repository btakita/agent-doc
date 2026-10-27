package com.github.btakita.agentdoc

import com.google.gson.Gson
import com.google.gson.JsonObject
import com.google.gson.JsonParser
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.nio.file.Files
import java.nio.file.Paths

/** `#jbshrinkrestarts`: the patch watcher's prefix repair behind the native `exchange_prefix_repair` derivation. */
class ExchangePrefixRepairTest {
    private val gson = Gson()

    private fun fixture(): JsonObject {
        val stream = javaClass.classLoader.getResourceAsStream("editor-derivations/exchange_prefix_repair.json")
            ?: error("exchange prefix repair parity fixture not on the test classpath")
        val raw = stream.bufferedReader(Charsets.UTF_8).use { it.readText() }
        return JsonParser.parseString(raw).asJsonObject
    }

    @Test
    fun `frozen prefix-repair floor answers every parity case and sends exactly its input`() {
        // The same file pins the native derivation in Rust
        // (exchange_prefix_repair_answers_the_shared_parity_cases).
        val cases = fixture().getAsJsonArray("cases").map { it.asJsonObject }
        assertTrue("the parity file lost cases", cases.size >= 120)
        for (case in cases) {
            val name = case.get("name").asString
            val input = case.getAsJsonObject("input")
            val document = input.get("document").asString
            val lines = input.getAsJsonArray("prefix_lines").map { it.asString }
            assertEquals(
                name,
                input,
                JsonParser.parseString(gson.toJson(ExchangePrefixRepair.evidence(document, lines))).asJsonObject,
            )
            val expected = case.getAsJsonObject("frozen_expected") ?: case.getAsJsonObject("expected")
            val repaired = ExchangePrefixRepair.frozen(document, lines)
            assertEquals(name, expected.get("changed").asBoolean, repaired != document)
            if (expected.get("changed").asBoolean) {
                assertEquals(name, expected.get("document").asString, repaired)
            } else {
                assertTrue(name, expected.get("document")?.isJsonNull ?: true)
            }
        }
    }

    @Test
    fun `the patch worker sends the document and targets and takes the native answer`() {
        var asked: String? = null
        var sent: String? = null
        val repaired =
            ExchangePrefixRepair.repair("doc", listOf("a")) { derivation, input ->
                asked = derivation
                sent = input
                """{"changed":true,"document":"❯ native doc"}"""
            }
        assertEquals("exchange_prefix_repair", asked)
        assertEquals("""{"document":"doc","prefix_lines":["a"]}""", sent)
        assertEquals("❯ native doc", repaired)
        assertEquals("doc", ExchangePrefixRepair.repair("doc", listOf("a")) { _, _ -> """{"changed":false}""" })
    }

    @Test
    fun `no targets ask nothing, and no or an unreadable answer takes the frozen floor`() {
        assertEquals("doc", ExchangePrefixRepair.repair("doc", emptyList()) { _, _ -> error("must not ask") })
        val document = "<!-- agent:exchange -->\nfix it\n<!-- /agent:exchange -->\n"
        val floor = ExchangePrefixRepair.frozen(document, listOf("fix it"))
        assertEquals(floor, ExchangePrefixRepair.repair(document, listOf("fix it"), EditorDerivation.Unavailable))
        assertEquals(floor, ExchangePrefixRepair.repair(document, listOf("fix it")) { _, _ -> "not json" })
        // A changed answer without its document cannot be executed.
        assertEquals(floor, ExchangePrefixRepair.repair(document, listOf("fix it")) { _, _ -> """{"changed":true}""" })
        assertNull(ExchangePrefixRepair.parseNative("""{"document":"x"}""", document))
    }

    @Test
    fun `the patch worker repairs prefixes only through the derivation`() {
        val source = Files.readString(Paths.get("src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt"))
        val wrapper = source.substringAfter("private fun normalizeExchangePrefixes(").substringBefore("/**")
        assertTrue(wrapper.contains("ExchangePrefixRepair.repair(doc, lines)"))
        val calls = Regex("""normalizeExchangePrefixesUtil\(""").findAll(source).count()
        assertEquals("only the frozen floor's own definition names it in PatchWatcher", 1, calls)
        assertFalse(source.substringBefore("internal fun normalizeExchangePrefixesUtil(").contains("normalizeExchangePrefixesUtil(doc"))
    }
}
