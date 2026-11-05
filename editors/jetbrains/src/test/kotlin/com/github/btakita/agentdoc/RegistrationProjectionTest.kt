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
        disk: String? = null,
    ) = RegistrationProjection.Observation(
        deferCanonicalProjectionForPendingLocal = flags.get("defer_canonical_projection_for_pending_local").asBoolean,
        canonicalProjectionRetained = flags.get("canonical_projection_retained").asBoolean,
        retainedReplicaReseedPending = flags.get("retained_replica_reseed_pending").asBoolean,
        canonicalCoversRetainedFrontier = flags.get("canonical_covers_retained_frontier").flagOrNull(),
        publishedShadow = shadow,
        bufferText = buffer,
        canonicalText = canonical,
        canonicalContainsOperatorEdits = flags.get("canonical_contains_operator_edits").flagOrNull(),
        cleanMergeAvailable = flags.get("clean_merge_available").flagOrNull(),
        diskText = disk,
        attachHeldForOperator = flags.get("attach_held_for_operator").flagOrNull() == true,
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
                    RetainedRegistrationProjectionAction.AdoptOperatorBuffer -> "adopt_operator_buffer"
                },
            )
            add("hold_reason", decision.holdReason?.let { gson.toJsonTree(it) } ?: com.google.gson.JsonNull.INSTANCE)
        }

    /** [input] without its null (unknown) facts: Gson omits them on the wire. */
    private fun withoutNulls(input: JsonObject): JsonObject =
        JsonObject().also { out -> input.entrySet().filter { !it.value.isJsonNull }.forEach { out.add(it.key, it.value) } }

    @Test
    fun `GH 326 frozen registration floor answers every named parity case and captures its facts exactly`() {
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
                    texts.get("disk").textOrNull(),
                )
            // What the plugin sends is exactly the fixture's input: facts, never text.
            assertEquals(
                name,
                withoutNulls(input),
                JsonParser.parseString(gson.toJson(RegistrationProjection.evidence(observation))).asJsonObject,
            )
            // A native rule that diverged on purpose keeps the frozen answer beside it.
            val frozenExpected = case.getAsJsonObject("frozen_expected") ?: case.getAsJsonObject("expected")
            assertEquals(name, frozenExpected, wire(RegistrationProjection.frozen(observation)))
            // The native answer is executed as given (this plugin knows every native action).
            assertEquals(
                name,
                case.getAsJsonObject("expected"),
                wire(RegistrationProjection.parseNative(gson.toJson(case.getAsJsonObject("expected")), observation)!!),
            )
        }
    }

    private fun emptyCanonicalOver(buffer: String, reseed: Boolean, shadow: String?) =
        RegistrationProjection.Observation(
            deferCanonicalProjectionForPendingLocal = false,
            canonicalProjectionRetained = false,
            retainedReplicaReseedPending = reseed,
            canonicalCoversRetainedFrontier = null,
            publishedShadow = shadow,
            bufferText = buffer,
            canonicalText = "",
            canonicalContainsOperatorEdits = null,
            cleanMergeAvailable = null,
            diskText = buffer,
        )

    @Test
    fun `an empty canonical is never adopted over a non-empty buffer whichever rules answer`() {
        // #emptycanonicaldeadlock: the frozen floor, an older native generation that still answers
        // apply/merge/hold, and the current native answer all end in adopting the operator buffer.
        val answers =
            listOf(
                """{"action":"apply_canonical","hold_reason":null}""",
                """{"action":"merge_forward","hold_reason":null}""",
                """{"action":"hold_operator_buffer","hold_reason":"retained-reseed-missing-settled-shadow"}""",
                """{"action":"adopt_operator_buffer","hold_reason":null}""",
            )
        for (reseed in listOf(false, true)) {
            for (shadow in listOf(null, "older")) {
                val observation = emptyCanonicalOver("# doc\n\noperator text\n", reseed, shadow)
                val floor = RegistrationProjection.decide(observation, EditorDerivation.Unavailable)
                // Either way the buffer becomes canonical: the floor may already publish it from
                // a proven shadow; anything that would project, merge, or hold becomes an adoption.
                assertTrue(
                    "floor reseed=$reseed shadow=$shadow: ${floor.action}",
                    floor.action in
                        setOf(
                            RetainedRegistrationProjectionAction.AdoptOperatorBuffer,
                            RetainedRegistrationProjectionAction.PublishOperatorBuffer,
                        ),
                )
                assertNull(floor.holdReason)
                for (answer in answers) {
                    val decision = RegistrationProjection.decide(observation) { _, _ -> answer }
                    assertEquals(answer, RetainedRegistrationProjectionAction.AdoptOperatorBuffer, decision.action)
                }
            }
        }
        // An empty buffer over an empty canonical is convergence, not a clobber.
        val bothEmpty = emptyCanonicalOver("", reseed = true, shadow = null)
        assertEquals(
            RetainedRegistrationProjectionAction.ApplyCanonical,
            RegistrationProjection.decide(bothEmpty) { _, _ -> """{"action":"apply_canonical","hold_reason":null}""" }.action,
        )
        assertTrue(emptyCanonicalWouldClobberBufferUtil("", "x"))
        assertFalse(emptyCanonicalWouldClobberBufferUtil("", ""))
        assertFalse(emptyCanonicalWouldClobberBufferUtil(null, "x"))
        assertFalse(emptyCanonicalWouldClobberBufferUtil("c", "x"))
    }

    @Test
    fun `the plugin reports emptiness, the saved document, and a prior hold as facts`() {
        val observation =
            divergent.copy(diskText = divergent.bufferText, attachHeldForOperator = true)
        val evidence = RegistrationProjection.evidence(observation)
        assertEquals(false, evidence.canonicalEmpty)
        assertEquals(false, evidence.bufferEmpty)
        assertEquals(true, evidence.bufferEqualsDisk)
        assertEquals(true, evidence.attachHeldForOperator)
        val unknown = RegistrationProjection.evidence(divergent)
        assertNull("an unread disk is unknown", unknown.bufferEqualsDisk)
        assertNull("no prior hold is not reported", unknown.attachHeldForOperator)
        val wire = gson.toJson(RegistrationProjection.evidence(observation))
        assertFalse("document text never crosses the boundary", wire.contains("base plus operator"))
    }

    @Test
    fun `every remote editor apply refuses an empty canonical over a non-empty buffer`() {
        val source =
            Files.readString(Paths.get("src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt"))
        val queue = source.substringAfter("    private fun queueRemoteTextApply(").substringBefore("    private fun recoverRejectedRemoteCanonical(")
        assertTrue(queue.contains("emptyCanonicalWouldClobberBufferUtil(converged"))
        assertTrue(
            "the guard runs before the remote text can be queued",
            queue.indexOf("emptyCanonicalWouldClobberBufferUtil(") < queue.indexOf("remoteEditorApplies.ingress("),
        )
        val edt = source.substringAfter("    private fun applyRemoteTextOnEdt(").substringBefore("\n    private fun ")
        assertTrue(edt.contains("emptyCanonicalWouldClobberBufferUtil(pending.targetText, before)"))
        val finalize = source.substringAfter("    private fun finalizeRegistrationProjection(").substringBefore("    private fun retainCanonicalProjectionAfterRegistration(")
        assertTrue(finalize.contains("RetainedRegistrationProjectionAction.AdoptOperatorBuffer ->"))
    }

    @Test
    fun `GH 326 frozen registration floor answers the exhaustive parity table`() {
        val table = fixture().getAsJsonObject("exhaustive")
        val dimensions = table.getAsJsonArray("dimensions").map { it.asJsonObject }
        // `#restartadoptscanonical`: where the native rules diverged on purpose, the frozen floor's
        // own answers sit beside them in `frozen_answers` (same order, same codes).
        val answers = (table.get("frozen_answers") ?: table.get("answers")).asString
        assertEquals("one frozen answer per native answer", table.get("answers").asString.length, answers.length)
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

    private fun restartedOver(
        buffer: String,
        canonical: String?,
        retained: Boolean,
        contains: Boolean? = null,
        defer: Boolean = false,
    ) = RegistrationProjection.Observation(
        deferCanonicalProjectionForPendingLocal = defer,
        canonicalProjectionRetained = retained,
        retainedReplicaReseedPending = false,
        canonicalCoversRetainedFrontier = null,
        publishedShadow = null,
        bufferText = buffer,
        canonicalText = canonical,
        canonicalContainsOperatorEdits = contains,
        cleanMergeAvailable = null,
        diskText = buffer,
    )

    @Test
    fun `a restarted editor never adopts a differing canonical whichever rules answer`() {
        // #restartadoptscanonical: a plain IDE restart while the controller hub survives. The
        // frozen floor and an older native generation still answer the restarted-IDE adoption; the
        // execution guard holds the saved operator buffer instead, with the same reason the native
        // rules give.
        val saved = "# doc\n\noperator text saved before the restart\n"
        val hub = "# doc\n\nsurviving controller text\n"
        for (retained in listOf(false, true)) {
            val observation = restartedOver(saved, hub, retained)
            assertTrue(unprovenCanonicalOverShadowlessBufferUtil(observation))
            val reason = if (retained) "ambiguous-retained-projection" else RegistrationProjection.AMBIGUOUS_REREGISTER_PROJECTION
            assertEquals(
                "the frozen floor alone still adopts (retained=$retained)",
                RetainedRegistrationProjectionAction.ApplyCanonical,
                RegistrationProjection.frozen(observation).action,
            )
            val floor = RegistrationProjection.decide(observation, EditorDerivation.Unavailable)
            assertEquals(RegistrationProjection.Decision(RetainedRegistrationProjectionAction.HoldOperatorBuffer, reason), floor)
            val older = RegistrationProjection.decide(observation) { _, _ -> """{"action":"apply_canonical","hold_reason":null}""" }
            assertEquals(RegistrationProjection.Decision(RetainedRegistrationProjectionAction.HoldOperatorBuffer, reason), older)
            val native =
                RegistrationProjection.decide(observation) { _, _ -> """{"action":"hold_operator_buffer","hold_reason":"$reason"}""" }
            assertEquals(RegistrationProjection.Decision(RetainedRegistrationProjectionAction.HoldOperatorBuffer, reason), native)
        }
        val adopt = """{"action":"apply_canonical","hold_reason":null}"""
        val apply = RetainedRegistrationProjectionAction.ApplyCanonical
        // Proof that canonical holds every operator edit still adopts.
        val proven = restartedOver(saved, saved + "agent response\n", retained = false, contains = true)
        assertEquals(apply, RegistrationProjection.decide(proven) { _, _ -> adopt }.action)
        // Adoption that loses nothing still adopts: convergence, an empty buffer, an unknown canonical.
        assertEquals(apply, RegistrationProjection.decide(restartedOver(hub, hub, retained = true)) { _, _ -> adopt }.action)
        assertEquals(apply, RegistrationProjection.decide(restartedOver("", hub, retained = true)) { _, _ -> adopt }.action)
        assertEquals(apply, RegistrationProjection.decide(restartedOver(saved, null, retained = false)) { _, _ -> adopt }.action)
        // A restored settled shadow is an ordinary causal decision again.
        val restored = restartedOver(saved, saved + "agent response\n", retained = false).copy(publishedShadow = saved)
        assertFalse(unprovenCanonicalOverShadowlessBufferUtil(restored))
        assertEquals(apply, RegistrationProjection.decide(restored) { _, _ -> adopt }.action)
        // A captured local delta that owns the buffer defers; the guard never rewrites that.
        assertFalse(unprovenCanonicalOverShadowlessBufferUtil(restartedOver(saved, hub, retained = false, defer = true)))
    }

    @Test
    fun `registration restores the persisted settled shadow and every settle persists it`() {
        // #restartadoptscanonical: the settled shadow survives an IDE restart.
        val source =
            Files.readString(Paths.get("src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt"))
        val registration =
            source.substringAfter("    private fun forwarderFor(")
                .substringBefore("    /** Complete the causal projection decision")
        assertTrue(
            "registration falls back to the durable shadow after memory",
            registration.contains(
                "settledShadows[filePath] ?: nativeReloadSettledShadows[filePath] ?: restorePersistedSettledShadow(filePath)",
            ),
        )
        assertTrue(
            "the restored shadow is captured before the decision",
            registration.indexOf("restorePersistedSettledShadow(filePath)") < registration.indexOf("RegistrationProjection.decide("),
        )
        val settle =
            source.substringAfter("    private fun projectSettledVisibleState(")
                .substringBefore("    private fun restorePersistedSettledShadow(")
        assertTrue(settle.contains("settledShadowStore.save(filePath, visibleText)"))
        assertTrue(
            "only an accepted projection is persisted",
            settle.indexOf("if (projected) {") < settle.indexOf("settledShadowStore.save("),
        )
        // #shadowstorecost: the save is backgrounded, so a closing project writes it before a
        // restart can need it, and the generation closes the store on unload and IDE exit.
        val dispose = source.substringAfter("    override fun dispose() {").substringBefore("\n    }\n")
        assertTrue("dispose flushes the pending settled shadows", dispose.contains("settledShadowStore.flush()"))
        val store =
            Files.readString(Paths.get("src/main/kotlin/com/github/btakita/agentdoc/SettledShadowStore.kt"))
        assertTrue(
            "the generation closes the store",
            store.contains("PluginGeneration.registerResource(\"settled-shadow-store\") { store.close() }"),
        )
        val restore = source.substringAfter("    private fun restorePersistedSettledShadow(").substringBefore("\n    }\n")
        assertTrue(
            "the restored shadow joins the in-memory map the publication-edge rechecks read",
            restore.contains("settledShadows.putIfAbsent(filePath, restored)"),
        )
    }
}
