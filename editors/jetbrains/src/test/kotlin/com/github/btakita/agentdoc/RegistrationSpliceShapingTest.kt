package com.github.btakita.agentdoc

import com.google.gson.Gson
import com.google.gson.JsonElement
import com.google.gson.JsonObject
import com.google.gson.JsonParser
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.nio.file.Files
import java.nio.file.Paths

/**
 * GH #326 phase 5c: registration's operator-text normalization and single-splice shaping, folded
 * into the native captured-splice calls behind the `registration` input.
 */
class RegistrationSpliceShapingTest {
    private val gson = Gson()

    private fun fixture(): JsonObject {
        val stream = javaClass.classLoader.getResourceAsStream("editor-derivations/registration_splice_shaping.json")
            ?: error("GH #326 registration splice shaping parity fixture not on the test classpath")
        val raw = stream.bufferedReader(Charsets.UTF_8).use { it.readText() }
        return JsonParser.parseString(raw).asJsonObject
    }

    private data class Registration(
        val name: String,
        val shadow: String,
        val buffer: String,
        val canonical: String,
        val expected: JsonObject,
    ) {
        val contained: Boolean get() = expected.get("contained").asBoolean
        val merged: String? get() = expected.get("merged").takeUnless { it.isJsonNull }?.asString
        val shortcut: Boolean get() = expected.get("containment_shortcut").asBoolean
    }

    private fun registrations(): List<Registration> =
        fixture().getAsJsonArray("registrations").map { element ->
            val case = element.asJsonObject
            Registration(
                name = case.get("name").asString,
                shadow = case.get("shadow").asString,
                buffer = case.get("buffer").asString,
                canonical = case.get("canonical").asString,
                expected = case.getAsJsonObject("expected"),
            )
        }

    private fun wire(batch: PreparedLocalEditorBatch): JsonElement = JsonParser.parseString(gson.toJson(batch))

    private fun batch(element: JsonElement): PreparedLocalEditorBatch =
        gson.fromJson(element, PreparedLocalEditorBatch::class.java)

    @Test
    fun `GH 326 frozen operator-text normalization answers every parity case`() {
        // The same file pins native `registration_operator_text` in Rust
        // (gh326_registration_shaping_answers_the_shared_parity_cases).
        val cases = fixture().getAsJsonArray("operator_text").map { it.asJsonObject }
        assertTrue("the parity file lost normalization cases", cases.size >= 20)
        for (case in cases) {
            assertEquals(
                case.get("name").asString,
                case.get("expected").asString,
                withoutBinaryOwnedMarkersUtil(case.get("text").asString),
            )
        }
    }

    @Test
    fun `GH 326 frozen splice shaping answers every registration parity case`() {
        val cases = registrations()
        assertTrue("the parity file lost registration cases", cases.size >= 10)
        for (case in cases) {
            val shadow = withoutBinaryOwnedMarkersUtil(case.shadow)
            val buffer = withoutBinaryOwnedMarkersUtil(case.buffer)
            val canonical = withoutBinaryOwnedMarkersUtil(case.canonical)
            assertEquals(case.name, case.expected.get("operator_shadow").asString, shadow)
            assertEquals(case.name, case.expected.get("operator_buffer").asString, buffer)
            assertEquals(case.name, case.expected.get("operator_canonical").asString, canonical)
            assertEquals(case.name, case.shortcut, buffer == shadow || buffer == canonical)
            assertEquals(case.name, case.expected.get("containment_batch"), wire(singleSpliceBatchUtil(shadow, buffer)))
            assertEquals(case.name, case.expected.get("merge_batch"), wire(singleSpliceBatchUtil(case.shadow, case.buffer)))
        }
    }

    @Test
    fun `GH 326 frozen floor sends the pre-5c batches and answers what native answers`() {
        for (case in registrations()) {
            val containmentBatch = batch(case.expected.get("containment_batch"))
            val proofs = mutableListOf<Triple<String, String, PreparedLocalEditorBatch>>()
            val contained =
                RegistrationSpliceShaping.operatorEditsContained(
                    case.shadow,
                    case.buffer,
                    case.canonical,
                    channel = RegistrationSpliceShaping.Channel.Unavailable,
                ) { base, canonical, batch ->
                    proofs += Triple(base, canonical, batch)
                    case.contained
                }
            assertEquals(case.name, case.contained, contained)
            if (case.shortcut) {
                assertTrue("${case.name}: the operator-text shortcut asks no proof", proofs.isEmpty())
            } else {
                assertEquals(
                    case.name,
                    listOf(
                        Triple(
                            case.expected.get("operator_shadow").asString,
                            case.expected.get("operator_canonical").asString,
                            containmentBatch,
                        ),
                    ),
                    proofs,
                )
            }

            val rebases = mutableListOf<Triple<String, String, PreparedLocalEditorBatch>>()
            val merged =
                RegistrationSpliceShaping.cleanMerge(
                    case.shadow,
                    case.buffer,
                    case.canonical,
                    channel = RegistrationSpliceShaping.Channel.Unavailable,
                ) { base, canonical, batch ->
                    rebases += Triple(base, canonical, batch)
                    case.merged?.let { PreparedLocalEditorBatch(emptyList(), it) }
                }
            assertEquals(case.name, case.merged, merged)
            assertEquals(
                case.name,
                listOf(Triple(case.shadow, case.canonical, batch(case.expected.get("merge_batch")))),
                rebases,
            )
        }
    }

    @Test
    fun `GH 326 a library that folds the shaping gets raw texts and the registration input`() {
        val noFloor: (String, String, PreparedLocalEditorBatch) -> Nothing = { _, _, _ ->
            throw AssertionError("the frozen floor ran beside a library that folds the shaping")
        }
        for (case in registrations()) {
            val asked = mutableListOf<List<Any>>()
            val channel =
                RegistrationSpliceShaping.Channel { call, base, canonical, requestJson ->
                    asked += listOf(call, base, canonical, JsonParser.parseString(requestJson))
                    when (call) {
                        RegistrationSpliceShaping.Call.CONTAINED -> RegistrationSpliceShaping.Answer(case.contained.toString())
                        RegistrationSpliceShaping.Call.REBASE ->
                            RegistrationSpliceShaping.Answer(
                                case.merged?.let { gson.toJson(PreparedLocalEditorBatch(emptyList(), it)) },
                            )
                    }
                }
            assertEquals(
                case.name,
                case.contained,
                RegistrationSpliceShaping.operatorEditsContained(case.shadow, case.buffer, case.canonical, channel, noFloor),
            )
            assertEquals(
                case.name,
                case.merged,
                RegistrationSpliceShaping.cleanMerge(case.shadow, case.buffer, case.canonical, channel, noFloor),
            )
            val request = { operatorText: Boolean ->
                JsonParser.parseString(
                    """{"registration":{"buffer":${gson.toJson(case.buffer)},"operatorText":$operatorText}}""",
                )
            }
            assertEquals(
                case.name,
                listOf(
                    listOf(RegistrationSpliceShaping.Call.CONTAINED, case.shadow, case.canonical, request(true)),
                    listOf(RegistrationSpliceShaping.Call.REBASE, case.shadow, case.canonical, request(false)),
                ),
                asked,
            )
            // Byte-exact: the buffer crosses inside JSON and decodes to the same UTF-16 text.
            val sent = (asked[0][3] as JsonElement).asJsonObject.getAsJsonObject("registration").get("buffer").asString
            assertEquals(case.name, case.buffer, sent)
        }
    }

    @Test
    fun `GH 326 a refused or unreadable folded answer is not proven and holds the buffer`() {
        val refused = RegistrationSpliceShaping.Channel { _, _, _, _ -> RegistrationSpliceShaping.Answer(null) }
        val noFloor: (String, String, PreparedLocalEditorBatch) -> Nothing = { _, _, _ ->
            throw AssertionError("a folded refusal must not fall back to the frozen floor")
        }
        assertFalse(RegistrationSpliceShaping.operatorEditsContained("a", "b", "c", refused, noFloor))
        assertNull(RegistrationSpliceShaping.cleanMerge("a", "b", "c", refused, noFloor))
        val garbled = RegistrationSpliceShaping.Channel { _, _, _, _ -> RegistrationSpliceShaping.Answer("{not json") }
        assertFalse(RegistrationSpliceShaping.operatorEditsContained("a", "b", "c", garbled, noFloor))
        assertNull(RegistrationSpliceShaping.cleanMerge("a", "b", "c", garbled, noFloor))
    }

    @Test
    fun `GH 326 a merge without three distinct generations asks no native call`() {
        val never = RegistrationSpliceShaping.Channel { _, _, _, _ ->
            throw AssertionError("native asked without three distinct generations")
        }
        val noFloor: (String, String, PreparedLocalEditorBatch) -> Nothing = { _, _, _ ->
            throw AssertionError("frozen floor asked without three distinct generations")
        }
        for ((shadow, buffer, canonical) in listOf(
            Triple(null, "b", "c"),
            Triple("a", null, "c"),
            Triple("a", "b", null),
            Triple("a", "c", "c"),
            Triple("a", "a", "c"),
            Triple("a", "b", "a"),
        )) {
            assertNull(RegistrationSpliceShaping.cleanMerge(shadow, buffer, canonical, never, noFloor))
        }
    }

    @Test
    fun `GH 326 the registration input goes only to a library that advertises it`() {
        val native =
            Files.readString(
                listOf(
                    Paths.get("src/main/kotlin/com/github/btakita/agentdoc/NativeLib.kt"),
                    Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/NativeLib.kt"),
                ).first { Files.exists(it) },
            )
        val ask = native
            .substringAfter("internal fun askCapturedSplicesWithRegistration(")
            .substringBefore("fun mergeFrontmatter(")
        val advertised = ask.indexOf("RegistrationSpliceShaping.REGISTRATION_INPUT !in inputs) return null")
        val firstCall = ask.indexOf("lib.agent_doc_captured_splices_contained(")
        assertTrue("the advertisement is checked before any call", advertised in 0 until firstCall)
        assertTrue(
            "the call goes to the generation that advertised the input",
            ask.contains("capturedSpliceInputsProbe?.takeIf { it.first === lib }"),
        )
    }
}
