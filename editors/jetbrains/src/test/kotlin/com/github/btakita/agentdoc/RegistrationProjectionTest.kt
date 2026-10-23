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

/** GH #326 phase 5: the CRDT replica registration direction behind the native `registration_projection` derivation. */
class RegistrationProjectionTest {
    private val gson = Gson()

    private fun fixture(): JsonObject {
        val stream = javaClass.classLoader.getResourceAsStream("editor-derivations/registration_projection.json")
            ?: error("GH #326 registration projection parity fixture not on the test classpath")
        val raw = stream.bufferedReader(Charsets.UTF_8).use { it.readText() }
        return JsonParser.parseString(raw).asJsonObject
    }

    private fun JsonElement?.textOrNull(): String? = this?.takeIf { !it.isJsonNull }?.asString

    private fun JsonElement?.flagOrNull(): Boolean? = this?.takeIf { !it.isJsonNull }?.asBoolean

    private fun observation(
        flags: JsonObject,
        shadow: String?,
        buffer: String?,
        canonical: String?,
    ) = RegistrationProjection.Observation(
        deferCanonicalProjectionForPendingLocal = flags.get("defer_canonical_projection_for_pending_local").asBoolean,
        canonicalProjectionRetained = flags.get("canonical_projection_retained").asBoolean,
        retainedReplicaReseedPending = flags.get("retained_replica_reseed_pending").asBoolean,
        canonicalCoversRetainedFrontier = flags.get("canonical_covers_retained_frontier").flagOrNull(),
        canonicalSupersedesRetainedFrontier =
            flags.get("canonical_supersedes_retained_frontier")?.asBoolean ?: false,
        publishedShadow = shadow,
        bufferText = buffer,
        canonicalText = canonical,
        canonicalContainsOperatorEdits = flags.get("canonical_contains_operator_edits").flagOrNull(),
        cleanMergeAvailable = flags.get("clean_merge_available").flagOrNull(),
    )

    /** The answer in the fixture's wire shape. */
    private fun wire(decision: RegistrationProjection.Decision): JsonObject =
        JsonObject().apply {
            addProperty(
                "action",
                when (decision.action) {
                    RetainedRegistrationProjectionAction.ApplyCanonical -> "apply_canonical"
                    RetainedRegistrationProjectionAction.PublishOperatorBuffer -> "publish_operator_buffer"
                    RetainedRegistrationProjectionAction.HoldOperatorBuffer -> "hold_operator_buffer"
                    RetainedRegistrationProjectionAction.DeferCanonicalProjection -> "defer_canonical_projection"
                    RetainedRegistrationProjectionAction.MergeForward -> "merge_forward"
                },
            )
            add("hold_reason", decision.holdReason?.let { gson.toJsonTree(it) } ?: com.google.gson.JsonNull.INSTANCE)
        }

    /** [input] without its null (unknown) facts: Gson omits them on the wire. */
    private fun withoutNulls(input: JsonObject): JsonObject =
        JsonObject().also { out -> input.entrySet().filter { !it.value.isJsonNull }.forEach { out.add(it.key, it.value) } }

    @Test
    fun `GH 326 registration facts match every named case and the frozen floor answers compatible cases`() {
        // The same file pins the native derivation in Rust
        // (gh326_registration_projection_answers_the_shared_parity_cases).
        val cases = fixture().getAsJsonArray("cases").map { it.asJsonObject }
        assertTrue("the parity file lost cases", cases.size >= 24)
        for (case in cases) {
            val name = case.get("name").asString
            val texts = case.getAsJsonObject("texts")
            val input = case.getAsJsonObject("input")
            val observation =
                observation(
                    input,
                    texts.get("published_shadow").textOrNull(),
                    texts.get("buffer").textOrNull(),
                    texts.get("canonical").textOrNull(),
                )
            // What the plugin sends is exactly the fixture's input: facts, never text.
            assertEquals(
                name,
                withoutNulls(input),
                JsonParser.parseString(gson.toJson(RegistrationProjection.evidence(observation))).asJsonObject,
            )
            if (case.get("native_only")?.asBoolean != true) {
                assertEquals(name, case.getAsJsonObject("expected"), wire(RegistrationProjection.frozen(observation)))
            }
        }
    }

    @Test
    fun `GH 326 frozen registration floor answers the exhaustive parity table`() {
        val table = fixture().getAsJsonObject("exhaustive")
        val dimensions = table.getAsJsonArray("dimensions").map { it.asJsonObject }
        val answers = table.get("answers").asString
        val codes = table.getAsJsonObject("codes")
        val combinations = dimensions.fold(1) { product, dimension -> product * dimension.getAsJsonArray("values").size() }
        assertEquals("one answer per combination", combinations, answers.length)
        assertTrue("the exhaustive table lost dimensions", combinations >= 5184)
        for (index in answers.indices) {
            // Odometer: the last dimension varies fastest.
            var rest = index
            val chosen = JsonObject()
            for (dimension in dimensions.asReversed()) {
                val values = dimension.getAsJsonArray("values")
                chosen.add(dimension.get("name").asString, values[rest % values.size()])
                rest /= values.size()
            }
            val observation =
                observation(
                    chosen,
                    chosen.get("published_shadow").textOrNull(),
                    chosen.get("buffer").textOrNull(),
                    chosen.get("canonical").textOrNull(),
                )
            val expected = codes.getAsJsonObject(answers[index].toString())
            assertEquals("combination $index: $chosen", expected, wire(RegistrationProjection.frozen(observation)))
        }
    }

    private val divergent =
        RegistrationProjection.Observation(
            deferCanonicalProjectionForPendingLocal = false,
            canonicalProjectionRetained = false,
            retainedReplicaReseedPending = false,
            canonicalCoversRetainedFrontier = null,
            publishedShadow = "# doc\n\nbase\n",
            bufferText = "# doc\n\nbase plus operator\n",
            canonicalText = "# doc\n\nbase plus agent\n",
            canonicalContainsOperatorEdits = null,
            cleanMergeAvailable = null,
        )

    @Test
    fun `the plugin sends the facts and executes the native answer`() {
        var asked: String? = null
        var sent: String? = null
        val decision =
            RegistrationProjection.decide(divergent) { derivation, input ->
                asked = derivation
                sent = input
                // A rule the frozen floor does not have: the native answer wins.
                """{"action":"merge_forward","hold_reason":null}"""
            }
        assertEquals("registration_projection", asked)
        assertEquals(RetainedRegistrationProjectionAction.MergeForward, decision.action)
        assertNull(decision.holdReason)
        val wire = sent!!
        for (text in listOf(divergent.publishedShadow!!, divergent.bufferText!!, divergent.canonicalText!!)) {
            assertFalse("document text never crosses the boundary", wire.contains(text.trim()))
        }
        assertEquals(
            JsonParser.parseString(gson.toJson(RegistrationProjection.evidence(divergent))),
            JsonParser.parseString(wire),
        )
    }

    @Test
    fun `a native hold carries its own reason and falls back to the frozen reason when it has none`() {
        val native =
            RegistrationProjection.decide(divergent) { _, _ ->
                """{"action":"hold_operator_buffer","hold_reason":"a-newer-diagnosis"}"""
            }
        assertEquals(RetainedRegistrationProjectionAction.HoldOperatorBuffer, native.action)
        assertEquals("a-newer-diagnosis", native.holdReason)

        val bare = RegistrationProjection.decide(divergent) { _, _ -> """{"action":"hold_operator_buffer"}""" }
        assertEquals(RegistrationProjection.AMBIGUOUS_REREGISTER_PROJECTION, bare.holdReason)
    }

    @Test
    fun `no native answer, an unknown action, or an unreadable answer takes the frozen floor`() {
        val floor = RegistrationProjection.frozen(divergent)
        assertEquals(RetainedRegistrationProjectionAction.HoldOperatorBuffer, floor.action)
        assertEquals(RegistrationProjection.AMBIGUOUS_REREGISTER_PROJECTION, floor.holdReason)
        assertEquals(floor, RegistrationProjection.decide(divergent, EditorDerivation.Unavailable))
        // A newer library may name an action this plugin generation cannot execute.
        assertEquals(floor, RegistrationProjection.decide(divergent) { _, _ -> """{"action":"rebase_later"}""" })
        assertEquals(floor, RegistrationProjection.decide(divergent) { _, _ -> """{"action":null}""" })
        assertEquals(floor, RegistrationProjection.decide(divergent) { _, _ -> "not json" })
    }

    @Test
    fun `forwarderFor decides through the derivation and keeps executing the answer itself`() {
        val source =
            Files.readString(Paths.get("src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt"))
        val registration =
            source.substringAfter("    private fun forwarderFor(")
                .substringBefore("    /** Complete the causal projection decision")
        assertTrue(registration.contains("RegistrationProjection.decide("))
        assertFalse(
            "the rule is asked through the derivation, never called directly",
            registration.contains("retainedRegistrationProjectionActionForAttachUtil("),
        )
        assertTrue(registration.contains("registrationProjection.holdReason"))
        // The publication-edge rechecks and their order are not part of the derivation.
        assertTrue(
            registration.indexOf("RegistrationProjection.decide(") <
                registration.indexOf("retainedProjectionHoldPaths.add(filePath)"),
        )
        assertFalse(registration.contains("publishedShadowAtRegistration!!"))
        assertFalse(registration.contains("bufferTextAtRegistration!!"))
    }
}
