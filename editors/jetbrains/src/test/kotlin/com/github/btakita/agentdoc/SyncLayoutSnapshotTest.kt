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
import java.util.concurrent.CompletableFuture

/** GH #326 phase 3: the Sync Tmux Layout `--col` snapshot behind the native `sync_layout` derivation. */
class SyncLayoutSnapshotTest {
    private val gson = Gson()

    private fun parityCases(): List<JsonObject> {
        val stream = javaClass.classLoader.getResourceAsStream("editor-derivations/sync_layout.json")
            ?: error("GH #326 sync layout parity fixture not on the test classpath")
        val raw = stream.bufferedReader(Charsets.UTF_8).use { it.readText() }
        return JsonParser.parseString(raw).asJsonObject.getAsJsonArray("cases").map { it.asJsonObject }
    }

    /**
     * The fixture omits fields the way an older plugin generation would (every input is optional on
     * the native side); the sync always fills them, so overlay the case on a complete observation.
     */
    private fun evidenceOf(case: JsonObject): SyncLayoutSnapshot.Evidence {
        val complete = JsonParser.parseString("""{"visible":[],"foreground":true}""").asJsonObject
        for ((key, value) in case.getAsJsonObject("input").entrySet()) {
            complete.add(key, value)
        }
        return gson.fromJson(complete, SyncLayoutSnapshot.Evidence::class.java)
    }

    private fun nearestRoots(case: JsonObject): (String) -> String? {
        val roots = case.getAsJsonObject("nearest_roots")?.entrySet()?.associate { it.key to it.value.asString }.orEmpty()
        return { file -> roots[file] }
    }

    /** The answer in the fixture's wire shape, so both derivations compare against one file. */
    private fun wire(result: SyncLayoutSnapshot.Result): JsonObject =
        JsonObject().apply {
            addProperty("project_root", result.projectRoot)
            add(
                "layout",
                result.editorLayout?.let { layout ->
                    JsonObject().apply {
                        add("columns", gson.toJsonTree(layout.columns.map { mapOf("files" to it.files) }))
                        addProperty("column_order", layout.columnOrder)
                    }
                } ?: com.google.gson.JsonNull.INSTANCE,
            )
            add("columns", gson.toJsonTree(result.columns))
            addProperty("column_order", result.columnOrder)
            addProperty(
                "decision",
                when (result.decision) {
                    is ExactVisibleSyncDecision.Publish -> "publish"
                    ExactVisibleSyncDecision.RefuseUnknownLayout -> "refuse_unknown_layout"
                    ExactVisibleSyncDecision.RefuseStaleFocusProjection -> "refuse_stale_focus_projection"
                },
            )
        }

    @Test
    fun `GH 326 frozen sync layout floor answers every shared parity case`() {
        // The same file pins the native derivation in Rust
        // (gh326_sync_layout_answers_the_shared_parity_cases).
        val cases = parityCases()
        assertTrue("the parity file lost cases", cases.size >= 12)
        for (case in cases) {
            val name = case.get("name").asString
            // The frozen floor runs off the EDT exactly like the sync's background thread.
            val result =
                CompletableFuture.supplyAsync { SyncLayoutSnapshot.frozen(evidenceOf(case), nearestRoots(case)) }.join()
            assertEquals(name, case.getAsJsonObject("expected"), wire(result))
            if (result.decision is ExactVisibleSyncDecision.Publish) {
                assertEquals(name, result.columns, (result.decision as ExactVisibleSyncDecision.Publish).columns)
            }
        }
    }

    @Test
    fun `GH 326 a native sync layout answer is used as derived`() {
        val case = parityCases().first { it.getAsJsonObject("expected").get("decision").asString == "publish" }
        // A native answer that differs from the floor proves the floor did not answer.
        val nativeAnswer = case.getAsJsonObject("expected").deepCopy().apply {
            addProperty("project_root", "/native-root")
            addProperty("later_field", "ignored")
        }
        var asked: Pair<String, String>? = null
        val result =
            CompletableFuture.supplyAsync {
                SyncLayoutSnapshot.derive(
                    evidenceOf(case),
                    derivation = { derivation, input ->
                        asked = derivation to input
                        nativeAnswer.toString()
                    },
                    nearestRoot = { error("the native answer needs no filesystem lookup") },
                )
            }.join()
        assertEquals("sync_layout", asked?.first)
        val input = JsonParser.parseString(asked!!.second).asJsonObject
        assertEquals(case.getAsJsonObject("input").get("focused"), input.get("focused"))
        assertEquals("/native-root", result.projectRoot)
        val expected = case.getAsJsonObject("expected")
        assertEquals(expected.get("columns"), gson.toJsonTree(result.columns))
        assertTrue(result.decision is ExactVisibleSyncDecision.Publish)
        assertEquals(expected.get("column_order").asString, result.columnOrder)
    }

    @Test
    fun `GH 326 an absent or unreadable native answer takes the frozen floor`() {
        val case = parityCases().first { it.get("name").asString.startsWith("a cross-root split") }
        val expected = case.getAsJsonObject("expected")
        for (answer in listOf<String?>(null, "{", """{"project_root":"/x","columns":[],"decision":"later"}""")) {
            val result =
                CompletableFuture.supplyAsync {
                    SyncLayoutSnapshot.derive(evidenceOf(case), { _, _ -> answer }, nearestRoots(case))
                }.join()
            assertEquals("answer=$answer", expected, wire(result))
        }
    }

    @Test
    fun `GH 326 the native sync layout is never asked on the EDT`() {
        val case = parityCases().first()
        var asked = false
        val result =
            CompletableFuture.supplyAsync {
                var answered: SyncLayoutSnapshot.Result? = null
                javax.swing.SwingUtilities.invokeAndWait {
                    answered =
                        SyncLayoutSnapshot.derive(evidenceOf(case), { _, _ -> asked = true; null }, nearestRoots(case))
                }
                answered!!
            }.join()
        assertFalse("native code must not run on the EDT", asked)
        assertEquals(case.getAsJsonObject("expected"), wire(result))
    }

    @Test
    fun `GH 326 a native layout keeps empty columns and defaults its order`() {
        val parsed =
            SyncLayoutSnapshot.parseNative(
                """{"project_root":"/r","layout":{"columns":[{"files":[]},{"files":["/r/a.md"]}]},""" +
                    """"columns":["","/r/a.md"],"column_order":"editor","decision":"publish"}""",
            )!!
        assertEquals(
            EditorLayout(listOf(LayoutColumn(emptyList()), LayoutColumn(listOf("/r/a.md")))),
            parsed.editorLayout,
        )
        assertEquals(ExactVisibleSyncDecision.Publish(listOf("", "/r/a.md")), parsed.decision)
        assertNull(
            SyncLayoutSnapshot.parseNative("""{"project_root":"/r","columns":["/r/a.md"],"decision":"refuse_unknown_layout"}""")
                ?.editorLayout,
        )
    }

    @Test
    fun `GH 326 the automatic surface roots its payload through sync_layout`() {
        val source =
            Files.readString(
                Paths.get("src/main/kotlin/com/github/btakita/agentdoc/EditorTabSyncListener.kt")
                    .takeIf { Files.exists(it) }
                    ?: Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/EditorTabSyncListener.kt"),
            )
        val resolve = source.substringAfter("private fun resolveSurface(").substringBefore("internal class CommandSurfaceCapture")
        assertTrue(resolve.contains("SyncLayoutSnapshot.derive("))
        for (rule in listOf("chooseSyncProjectRoot(", "normalizeEditorLayout(", "absolutizeEditorLayout(")) {
            assertFalse("GH #326 phase 3: the surface worker must not run the frozen $rule", resolve.contains(rule))
        }
    }

    @Test
    fun `GH 326 layout evidence captured once derives the layout the direct read derives`() {
        val windows =
            LayoutDetector.WindowLayoutEvidence(
                basePath = "/repo",
                windows =
                    listOf(
                        LayoutDetector.EditorWindowEvidence(640, 0, "/repo/b.md", listOf("/repo/b.md")),
                        LayoutDetector.EditorWindowEvidence(0, 0, "/repo/a.md", listOf("/repo/a.md")),
                    ),
                history = emptyList(),
            )
        val derived =
            CompletableFuture.supplyAsync {
                LayoutDetector.detectEditorLayout(
                    LayoutDetector.EditorLayoutEvidence.Windows(windows),
                    EditorDerivation.Unavailable,
                )
            }.join()
        assertEquals(
            CompletableFuture.supplyAsync {
                LayoutDetector.detectEditorLayout(windows, EditorDerivation.Unavailable)
            }.join(),
            derived,
        )
        assertEquals(listOf(listOf("a.md"), listOf("b.md")), derived?.columns?.map { it.files })
    }
}
