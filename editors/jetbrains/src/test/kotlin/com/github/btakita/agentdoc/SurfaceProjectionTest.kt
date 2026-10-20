package com.github.btakita.agentdoc

import com.google.gson.Gson
import com.google.gson.JsonObject
import com.google.gson.JsonParser
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.util.concurrent.CompletableFuture

/** GH #326 phase 2: the automatic editor surface behind the native `surface_projection` derivation. */
class SurfaceProjectionTest {
    private val gson = Gson()

    private fun parityCases(): List<JsonObject> {
        val stream = javaClass.classLoader.getResourceAsStream("editor-derivations/surface_projection.json")
            ?: error("GH #326 surface projection parity fixture not on the test classpath")
        val raw = stream.bufferedReader(Charsets.UTF_8).use { it.readText() }
        return JsonParser.parseString(raw).asJsonObject.getAsJsonArray("cases").map { it.asJsonObject }
    }

    /**
     * The fixture omits fields the way an older plugin generation would (every input is optional on
     * the native side); the capture always fills them, so overlay the case on a complete capture.
     */
    private fun evidenceOf(case: JsonObject): SurfaceProjection.SurfaceProjectionEvidence {
        val complete =
            JsonParser.parseString(
                """{"backend_window_count":0,"windows":[],"window_geometry_unavailable":false,"history":[],""" +
                    """"manager_selected":[],"open_session_documents":[],"measured_layout_authority":false,""" +
                    """"preferred_session_document":false,"force_reconcile":false,"preserve_focus":false}""",
            ).asJsonObject
        for ((key, value) in case.getAsJsonObject("input").entrySet()) {
            complete.add(key, value)
        }
        complete.getAsJsonArray("windows").forEach { window ->
            val fields = window.asJsonObject
            if (!fields.has("session_tabs")) fields.add("session_tabs", com.google.gson.JsonArray())
        }
        return gson.fromJson(complete, SurfaceProjection.SurfaceProjectionEvidence::class.java)
    }

    /** The frozen floor over [case], with the case's Remote Dev fold answer and no native layout. */
    private fun frozen(case: JsonObject): SurfaceProjection.Result {
        val foldAnswer = case.get("remote_fold")?.toString()
        return SurfaceProjection.frozen(
            evidenceOf(case),
            windowLayout = { windows -> LayoutDetector.detectEditorLayout(windows, EditorDerivation.Unavailable) },
            remoteLayout = { snapshot ->
                LayoutDetector.detectEditorLayout(snapshot) { root, _ ->
                    assertEquals("/repo", root)
                    foldAnswer ?: error("${case.get("name").asString}: unexpected Remote Dev fold")
                }
            },
        )
    }

    @Test
    fun `GH 326 frozen surface projection floor answers every shared parity case`() {
        // The same file pins the native derivation in Rust
        // (gh326_surface_projection_answers_the_shared_parity_cases).
        val cases = parityCases()
        assertTrue("the parity file lost cases", cases.size >= 12)
        for (case in cases) {
            val name = case.get("name").asString
            val expected = case.getAsJsonObject("expected")
            // The frozen floor runs off the EDT exactly like the delivery worker.
            val result = CompletableFuture.supplyAsync { frozen(case) }.join()
            when (expected.get("outcome").asString) {
                "ready" -> {
                    assertTrue("$name: $result", result is SurfaceProjection.Result.Ready)
                    assertEquals(name, expected.getAsJsonObject("surface"), (result as SurfaceProjection.Result.Ready).surface)
                }
                "deferred" ->
                    assertEquals(
                        name,
                        SurfaceProjection.Result.Deferred(expected.get("reason").asString),
                        result,
                    )
                else -> error("$name: unknown outcome")
            }
        }
    }

    @Test
    fun `GH 326 surface evidence is the native wire shape`() {
        val evidence = evidenceOf(parityCases().first())
        val json = JsonParser.parseString(gson.toJson(evidence)).asJsonObject
        for (field in listOf(
            "base_path",
            "backend_window_count",
            "windows",
            "window_geometry_unavailable",
            "history",
            "manager_selected",
            "open_session_documents",
            "remote",
            "measured_layout_authority",
            "anchor_selected_session",
            "selected_text_editor_session",
            "preferred",
            "preferred_session_document",
            "authority",
            "remaining_selection_passes",
            "force_reconcile",
            "preserve_focus",
        )) {
            assertTrue("missing $field", json.has(field))
        }
        assertFalse("an absent previous selection is omitted, never empty", json.has("previous"))
        assertEquals("/repo", json.getAsJsonObject("remote").get("project_root").asString)
        assertEquals(
            "document_selection",
            SurfaceProjection.wireAuthority(EditorTabSyncListener.ObservationAuthority.DocumentSelection),
        )
        assertEquals(
            "ide_activation",
            SurfaceProjection.wireAuthority(EditorTabSyncListener.ObservationAuthority.IdeActivation),
        )
    }

    @Test
    fun `GH 326 a native surface projection answer wins and keeps fields this plugin does not know`() {
        val evidence = evidenceOf(parityCases().first())
        var asked: String? = null
        val result =
            CompletableFuture.supplyAsync {
                SurfaceProjection.derive(evidence, { derivation, inputJson ->
                    asked = derivation
                    assertTrue(inputJson.contains("open_session_documents"))
                    """{"outcome":"ready","layout_source":"windows","surface":{"focused":"/repo/b.md",""" +
                        """"visible":["/repo/b.md"],"open":["/repo/b.md"],"columns":[],"column_order":"editor",""" +
                        """"later_field":7},"observed_line":"[layout-detect] observed native","info":["i"]}"""
                }) { error("the native answer owns the layout") }
            }.join()
        assertEquals("surface_projection", asked)
        val surface = (result as SurfaceProjection.Result.Ready).surface
        assertEquals("/repo/b.md", surface.get("focused").asString)
        assertEquals(7, surface.get("later_field").asInt)

        val deferred =
            CompletableFuture.supplyAsync {
                SurfaceProjection.derive(evidence, { _, _ ->
                    """{"outcome":"deferred","reason":"later_rule","layout_source":"none"}"""
                }) { error("a deferred projection resolves no layout") }
            }.join()
        assertEquals(SurfaceProjection.Result.Deferred("later_rule"), deferred)
    }

    @Test
    fun `GH 326 no readable native answer takes the frozen floor`() {
        val case = parityCases().first()
        val evidence = evidenceOf(case)
        val floor = CompletableFuture.supplyAsync { frozen(case) }.join()
        for (answer in listOf<String?>(null, "not json", """{"outcome":"later_outcome"}""", """{"outcome":"ready"}""")) {
            val result =
                CompletableFuture.supplyAsync {
                    // Only the surface projection is unreadable; the window layout floor still answers.
                    SurfaceProjection.derive(evidence, { derivation, _ ->
                        answer.takeIf { derivation == "surface_projection" }
                    }) { error("local case") }
                }.join()
            assertEquals("answer=$answer", floor, result)
        }
        assertNull(SurfaceProjection.parseNative("""{"outcome":"ready","surface":[]}"""))
    }

    @Test
    fun `GH 326 an EDT caller is refused the native surface projection`() {
        val case = parityCases().first()
        val evidence = evidenceOf(case)
        var asked = false
        var result: SurfaceProjection.Result? = null
        javax.swing.SwingUtilities.invokeAndWait {
            result = SurfaceProjection.derive(evidence, { _, _ ->
                asked = true
                null
            }) { error("local case") }
        }
        assertFalse("native calls never run on the EDT", asked)
        assertEquals(CompletableFuture.supplyAsync { frozen(case) }.join(), result)
    }
}
