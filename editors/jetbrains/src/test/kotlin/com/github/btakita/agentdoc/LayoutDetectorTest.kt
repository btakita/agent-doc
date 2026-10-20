package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicReference
import javax.swing.SwingUtilities

class LayoutDetectorTest {

    @Test
    fun `GH 157 zero-window detection entered on EDT runs native fold on worker`() {
        val nativeFoldRan = AtomicBoolean(false)
        val detected = AtomicReference<EditorLayout?>()

        SwingUtilities.invokeAndWait {
            assertTrue(SwingUtilities.isEventDispatchThread())
            detected.set(
                LayoutDetector.detectEditorLayout(
                    LayoutDetector.RemoteLayoutSnapshot(
                        projectRoot = "/repo",
                        clients = listOf(
                            LayoutDetector.RemoteClientSessionEditors(
                                visible = listOf("tasks/a.md"),
                                selected = listOf("tasks/a.md"),
                                open = listOf("tasks/a.md", "tasks/b.md"),
                            ),
                        ),
                        focusedSessionFiles = listOf("tasks/a.md"),
                    ),
                    nativeFold = { projectRoot, evidenceJson ->
                        nativeFoldRan.set(true)
                        assertFalse(SwingUtilities.isEventDispatchThread())
                        assertEquals("/repo", projectRoot)
                        assertTrue(evidenceJson.contains("tasks/b.md"))
                        """{"columns":[{"files":["tasks/a.md"]},{"files":["tasks/b.md"]}],"source":"retained_remote_columns","column_order":"retained","reason":"single_selection_within_retained_layout"}"""
                    },
                ),
            )
        }

        assertTrue("the GH #134 retained-split fold must run", nativeFoldRan.get())
        assertEquals(
            EditorLayout(
                listOf(
                    LayoutColumn(listOf("tasks/a.md")),
                    LayoutColumn(listOf("tasks/b.md")),
                ),
                columnOrder = "retained",
            ),
            detected.get(),
        )
    }

    @Test
    fun `remote client session evidence wins over a backend source window`() {
        assertTrue(
            LayoutDetector.shouldUseRemoteClientLayout(
                backendWindowCount = 1,
                remoteClientSessionCount = 1,
            ),
        )
        assertFalse(
            LayoutDetector.shouldUseRemoteClientLayout(
                backendWindowCount = 1,
                remoteClientSessionCount = 0,
            ),
        )
    }

    @Test
    fun `buildColumnsFromSnapshots keeps screen order when focused window is listed first`() {
        val columns = LayoutDetector.buildColumnsFromSnapshots(
            listOf(
                LayoutDetector.LayoutWindowSnapshot(x = 600, y = 0, file = "right.md"),
                LayoutDetector.LayoutWindowSnapshot(x = 0, y = 0, file = "left.md"),
            )
        )

        assertEquals(
            listOf(
                LayoutColumn(listOf("left.md")),
                LayoutColumn(listOf("right.md")),
            ),
            columns,
        )
    }

    @Test
    fun `buildColumnsFromSnapshots stacks windows in the same column by y position`() {
        val columns = LayoutDetector.buildColumnsFromSnapshots(
            listOf(
                LayoutDetector.LayoutWindowSnapshot(x = 0, y = 400, file = "bottom.md"),
                LayoutDetector.LayoutWindowSnapshot(x = 0, y = 0, file = "top.md"),
            )
        )

        assertEquals(
            listOf(LayoutColumn(listOf("top.md", "bottom.md"))),
            columns,
        )
    }

    @Test
    fun `buildColumnsFromSnapshots preserves empty columns for non markdown editor panes`() {
        val columns = LayoutDetector.buildColumnsFromSnapshots(
            listOf(
                LayoutDetector.LayoutWindowSnapshot(x = 0, y = 0, file = null),
                LayoutDetector.LayoutWindowSnapshot(x = 600, y = 0, file = "right.md"),
            )
        )

        assertEquals(
            listOf(
                LayoutColumn(emptyList()),
                LayoutColumn(listOf("right.md")),
            ),
            columns,
        )
    }

    @Test
    fun `buildColumnsFromSnapshots keeps unlaid-out splits apart instead of stacking them`() {
        // GH #77: a JetBrains Remote Dev backend never lays the editor splitters out,
        // so both splits report origin (0,0). Grouping by x folded them into one
        // column, the controller kept only the first document, and tmux converged to
        // a single pane swapped on every switch.
        val columns = LayoutDetector.buildColumnsFromSnapshots(
            listOf(
                LayoutDetector.LayoutWindowSnapshot(x = 0, y = 0, file = "tasks/agent-doc/agent-doc.md"),
                LayoutDetector.LayoutWindowSnapshot(x = 0, y = 0, file = "tasks/pmt2/mr/1109.md"),
            )
        )

        assertEquals(
            listOf(
                LayoutColumn(listOf("tasks/agent-doc/agent-doc.md")),
                LayoutColumn(listOf("tasks/pmt2/mr/1109.md")),
            ),
            columns,
        )
    }

    @Test
    fun `observed layout line names windows origins documents and columns`() {
        // GH #81 discriminator: tells "the IDE exposed one window" from a lossy join.
        val line = LayoutDetector.observedLayoutLine(
            2,
            listOf(
                LayoutDetector.LayoutWindowSnapshot(x = 0, y = 0, file = "a.md"),
                LayoutDetector.LayoutWindowSnapshot(x = 0, y = 0, file = null),
            ),
            listOf(LayoutColumn(listOf("a.md")), LayoutColumn(emptyList())),
        )
        assertEquals(
            "[layout-detect] observed windows=2 snapshots=[(0,0) a.md, (0,0) <none>] " +
                "columns=2 [a.md | <empty>]",
            line,
        )
    }

    @Test
    fun `remote observed line names each clients visible selected and open evidence`() {
        val line = LayoutDetector.observedRemoteLayoutLine(
            snapshots = listOf(LayoutDetector.LayoutWindowSnapshot(0, 0, "tasks/b.md")),
            columns = listOf(LayoutColumn(listOf("tasks/b.md"))),
            source = "retained_remote_columns reason=selected_visible_change_replaced_focus_column",
            clients = listOf(
                LayoutDetector.RemoteClientSessionEditors(
                    visible = listOf("tasks/a.md", "tasks/b.md"),
                    selected = listOf("tasks/b.md"),
                    open = listOf("tasks/a.md", "tasks/b.md", "tasks/c.md"),
                ),
                LayoutDetector.RemoteClientSessionEditors(
                    visible = emptyList(),
                    selected = emptyList(),
                    open = listOf("tasks/d.md"),
                ),
            ),
        )

        assertTrue(line.startsWith("[layout-detect] observed windows=0"))
        assertTrue(line.contains("remote_clients=2"))
        assertTrue(
            line.contains(
                "0:{visible=[tasks/a.md,tasks/b.md] selected=[tasks/b.md] " +
                    "open=[tasks/a.md,tasks/b.md,tasks/c.md]}",
            ),
        )
        assertTrue(
            line.contains("1:{visible=[<none>] selected=[<none>] open=[tasks/d.md]}"),
        )
    }

    private fun remoteLayoutCases(): List<com.google.gson.JsonObject> {
        val stream = javaClass.classLoader.getResourceAsStream("editor-derivations/remote_layout.json")
            ?: error("GH #326 remote layout contract fixture not on the test classpath")
        val raw = stream.bufferedReader(Charsets.UTF_8).use { it.readText() }
        return com.google.gson.JsonParser.parseString(raw).asJsonObject.getAsJsonArray("cases")
            .map { it.asJsonObject }
    }

    /** The EDT snapshot a contract step's input describes. */
    private fun remoteSnapshotOf(input: com.google.gson.JsonObject): LayoutDetector.RemoteLayoutSnapshot {
        fun strings(array: com.google.gson.JsonArray?) = array?.map { it.asString }.orEmpty()
        return LayoutDetector.RemoteLayoutSnapshot(
            projectRoot = input.get("project_root")?.asString ?: "",
            clients = input.getAsJsonArray("clients")?.map { client ->
                val fields = client.asJsonObject
                LayoutDetector.RemoteClientSessionEditors(
                    visible = strings(fields.getAsJsonArray("visible")),
                    selected = strings(fields.getAsJsonArray("selected")),
                    open = strings(fields.getAsJsonArray("open")),
                )
            }.orEmpty(),
            focusedSessionFiles = strings(input.getAsJsonArray("focused")),
        )
    }

    @Test
    fun `GH 326 the plugin sends each remote_layout contract step and publishes its answer`() {
        // The same file pins the native fold in Rust
        // (gh326_remote_layout_answers_the_shared_contract_cases).
        val cases = remoteLayoutCases()
        assertTrue("the contract file lost cases", cases.size >= 8)
        for (case in cases) {
            val name = case.get("name").asString
            for (step in case.getAsJsonArray("steps").map { it.asJsonObject }) {
                val input = step.getAsJsonObject("input")
                val expected = step.getAsJsonObject("expected")
                var sent: com.google.gson.JsonObject? = null
                val layout = LayoutDetector.detectEditorLayout(remoteSnapshotOf(input)) { root, evidenceJson ->
                    LayoutDetector.nativeRemoteLayout(root, evidenceJson, { derivation, inputJson ->
                        assertEquals(name, "remote_layout", derivation)
                        sent = com.google.gson.JsonParser.parseString(inputJson).asJsonObject
                        expected.toString()
                    }) { _, _ -> error("$name: the legacy fold is asked only without the derivation") }
                }
                if (input.has("project_root")) assertEquals(name, input, sent)
                val columns = expected.getAsJsonArray("columns").map { column ->
                    LayoutColumn(column.asJsonObject.getAsJsonArray("files").map { it.asString })
                }
                assertEquals(
                    name,
                    columns.takeIf { it.isNotEmpty() }?.let {
                        EditorLayout(it, expected.get("column_order").asString)
                    },
                    layout,
                )
            }
        }
    }

    @Test
    fun `GH 326 with no native answer the Remote Dev layout is unknown`() {
        // Phase 4 retired the memoryless Kotlin fallback: a split the fold would detect is not
        // re-derived in Kotlin. The layout stays unknown, which is structurally inert (GH #105).
        for (case in remoteLayoutCases()) {
            for (step in case.getAsJsonArray("steps").map { it.asJsonObject }) {
                val layout = LayoutDetector.detectEditorLayout(remoteSnapshotOf(step.getAsJsonObject("input"))) { root, json ->
                    LayoutDetector.nativeRemoteLayout(root, json, EditorDerivation.Unavailable) { _, _ -> null }
                }
                assertEquals(case.get("name").asString, null, layout)
            }
        }
        val line = LayoutDetector.unknownRemoteLayoutLine(
            focusedSessionFiles = listOf("tasks/laptop.md"),
            remoteSelections = listOf(listOf("tasks/laptop.md", "tasks/1099.md")),
            reason = LayoutDetector.NO_NATIVE_REMOTE_LAYOUT_REASON,
        )
        assertEquals(
            "[layout-detect] unknown windows=0 source=remote_client_selected_files " +
                "focused=[tasks/laptop.md] remote_clients=1 selections=[0:[tasks/laptop.md,tasks/1099.md]] " +
                "reason=native_remote_layout_unavailable",
            line,
        )
        assertFalse(line.contains("columns="))
    }

    @Test
    fun `GH 326 the remote_layout derivation is asked before the pre-326 fold symbol`() {
        val evidence = LayoutDetector.remoteLayoutEvidenceJson(
            listOf(LayoutDetector.RemoteClientSessionEditors(listOf("a.md", "b.md"), listOf("a.md"), listOf("a.md"))),
            focusedSessionFiles = listOf("a.md"),
        )
        val answer = """{"columns":[{"files":["a.md"]},{"files":["b.md"]}],"source":"remote_client_visible_editors"}"""
        var legacyAsked: Pair<String, String>? = null
        val legacy = { root: String, json: String ->
            legacyAsked = root to json
            answer
        }
        assertEquals(answer, LayoutDetector.nativeRemoteLayout("/repo", evidence, { _, _ -> answer }, legacy))
        assertEquals("a library that answers the derivation never reaches the symbol", null, legacyAsked)

        assertEquals(answer, LayoutDetector.nativeRemoteLayout("/repo", evidence, EditorDerivation.Unavailable, legacy))
        assertEquals("/repo" to evidence, legacyAsked)

        val input = com.google.gson.JsonParser.parseString(
            LayoutDetector.remoteLayoutDerivationInput("/repo", evidence),
        ).asJsonObject
        assertEquals("/repo", input.get("project_root").asString)
        assertEquals(
            listOf("a.md", "b.md"),
            input.getAsJsonArray("clients")[0].asJsonObject.getAsJsonArray("visible").map { it.asString },
        )
    }

    @Test
    fun `GH 134 remote evidence and native resolution round-trip`() {
        val json = LayoutDetector.remoteLayoutEvidenceJson(
            listOf(
                LayoutDetector.RemoteClientSessionEditors(
                    visible = listOf("tasks/a.md", "tasks/b.md"),
                    selected = listOf("tasks/a.md"),
                    open = listOf("tasks/a.md", "tasks/b.md"),
                ),
            ),
            focusedSessionFiles = listOf("tasks/a.md"),
        )
        val evidence = com.google.gson.JsonParser.parseString(json).asJsonObject
        assertEquals(
            listOf("tasks/a.md", "tasks/b.md"),
            evidence.getAsJsonArray("clients")[0].asJsonObject.getAsJsonArray("visible").map { it.asString },
        )
        assertEquals("tasks/a.md", evidence.getAsJsonArray("focused")[0].asString)

        val resolved = LayoutDetector.parseRemoteLayoutResolution(
            """{"columns":[{"files":["tasks/a.md"]},{"files":["tasks/b.md"]}],""" +
                """"source":"retained_remote_columns","reason":"single_selection_within_retained_layout"}""",
        )
        assertEquals(
            listOf(LayoutColumn(listOf("tasks/a.md")), LayoutColumn(listOf("tasks/b.md"))),
            resolved?.columns,
        )
        assertEquals("retained_remote_columns", resolved?.source)
        assertEquals("single_selection_within_retained_layout", resolved?.reason)

        val unknown = LayoutDetector.parseRemoteLayoutResolution(
            """{"columns":[],"source":"unknown","reason":"no_split_evidence"}""",
        )
        assertEquals(emptyList<LayoutColumn>(), unknown?.columns)
        assertEquals(null, LayoutDetector.parseRemoteLayoutResolution("not json"))

        val line = LayoutDetector.unknownRemoteLayoutLine(
            focusedSessionFiles = listOf("tasks/a.md"),
            remoteSelections = listOf(listOf("tasks/a.md")),
            visibleSelections = listOf(listOf("tasks/a.md")),
            reason = "no_split_evidence",
        )
        assertTrue(line, line.contains("visible=[0:[tasks/a.md]]"))
        assertTrue(line, line.endsWith("reason=no_split_evidence"))
    }

    @Test
    fun `one local editor window is a known one-column layout`() {
        assertEquals(
            EditorLayout(listOf(LayoutColumn(listOf("tasks/local.md")))),
            LayoutDetector.knownSingleWindowLayout("tasks/local.md"),
        )
        assertEquals(null, LayoutDetector.knownSingleWindowLayout(null))
    }

    @Test
    fun `an unchanged layout observation is re-logged on a heartbeat`() {
        // GH #88: a single deduplicated line could not tell "never ran" from "stuck".
        assertTrue(LayoutDetector.shouldLogObservedLayout(changed = true, observation = 7))
        assertFalse(LayoutDetector.shouldLogObservedLayout(changed = false, observation = 7))
        assertTrue(
            LayoutDetector.shouldLogObservedLayout(
                changed = false,
                observation = LayoutDetector.OBSERVED_LAYOUT_HEARTBEAT,
            ),
        )
    }

    @Test
    fun `stickyMarkdownForWindow prefers the live selection`() {
        assertEquals(
            "/repo/tasks/now.md",
            LayoutDetector.stickyMarkdownForWindow(
                selectedPath = "/repo/tasks/now.md",
                windowMarkdownTabsMruLast =
                    listOf("/repo/tasks/stale.md", "/repo/tasks/now.md"),
            ),
        )
    }

    @Test
    fun `stickyMarkdownForWindow ignores a selected non-session markdown plan`() {
        assertEquals(
            "/repo/tasks/backend.md",
            LayoutDetector.stickyMarkdownForWindow(
                selectedPath = "/repo/docs/backend-fpe-contracts-sdk-pr-plan.md",
                windowMarkdownTabsMruLast = listOf("/repo/tasks/backend.md"),
            ),
        )
    }

    @Test
    fun `stickyMarkdownForWindow falls back to the last document when source is selected`() {
        // #stickymdpane: the operator opened source in this column. The column
        // still stands for the document it was showing, so the tmux mirror
        // keeps that pane instead of collapsing.
        assertEquals(
            "/repo/tasks/recent.md",
            LayoutDetector.stickyMarkdownForWindow(
                selectedPath = "/repo/src/Thing.kt",
                windowMarkdownTabsMruLast =
                    listOf("/repo/tasks/older.md", "/repo/tasks/recent.md"),
            ),
        )
    }

    @Test
    fun `stickyMarkdownForWindow invents nothing for a source-only window`() {
        assertEquals(
            null,
            LayoutDetector.stickyMarkdownForWindow(
                selectedPath = "/repo/src/Thing.kt",
                windowMarkdownTabsMruLast = emptyList(),
            ),
        )
        assertEquals(
            null,
            LayoutDetector.stickyMarkdownForWindow(
                selectedPath = null,
                windowMarkdownTabsMruLast = emptyList(),
            ),
        )
    }

    // `#clearnoticeverify` (2026-10-11): a scratch doc opened in a detached editor
    // frame was counted with the main frame's two splits, measured against the
    // wrong splitters at (-56,-117), and published first: tmux showed 3 panes for
    // a 2-split editor. Only the active frame's windows are columns.
    @Test
    fun `detached editor frame windows are not columns of the active frame`() {
        data class Win(val file: String, val owner: String)
        val main = "main-splitters"
        val detached = "detached-splitters"
        val windows = listOf(
            Win("src/agent-doc/tmp/scratch.md", detached),
            Win("tasks/software/chromium-bridge.md", main),
            Win("src/haiven-dev/tasks/frontend.md", main),
        )

        val mainFrame = LayoutDetector.windowsInActiveFrame(windows, { it.owner }, main)
        assertEquals(
            listOf("tasks/software/chromium-bridge.md", "src/haiven-dev/tasks/frontend.md"),
            mainFrame.map { it.file },
        )
        val detachedFrame = LayoutDetector.windowsInActiveFrame(windows, { it.owner }, detached)
        assertEquals(listOf("src/agent-doc/tmp/scratch.md"), detachedFrame.map { it.file })
        // An unresolved or unmatched owner never hides the layout.
        assertEquals(windows, LayoutDetector.windowsInActiveFrame(windows, { it.owner }, null))
        assertEquals(windows, LayoutDetector.windowsInActiveFrame(windows, { it.owner }, "other"))
    }

    private fun windowLayoutParityCases(): List<com.google.gson.JsonObject> {
        val stream = javaClass.classLoader.getResourceAsStream("editor-derivations/window_layout.json")
            ?: error("GH #326 parity fixture not on the test classpath")
        val raw = stream.bufferedReader(Charsets.UTF_8).use { it.readText() }
        return com.google.gson.JsonParser.parseString(raw).asJsonObject
            .getAsJsonArray("cases")
            .map { it.asJsonObject }
    }

    private fun evidenceOf(case: com.google.gson.JsonObject): LayoutDetector.WindowLayoutEvidence =
        com.google.gson.Gson().fromJson(case.get("input"), LayoutDetector.WindowLayoutEvidence::class.java)

    private fun com.google.gson.JsonElement.nullableString(): String? = if (isJsonNull) null else asString

    @Test
    fun `GH 326 frozen window layout floor answers every shared parity case`() {
        // The same file pins the native `window_layout` derivation in Rust
        // (gh326_window_layout_answers_the_shared_parity_cases).
        val cases = windowLayoutParityCases()
        assertTrue("the parity file lost cases", cases.size >= 8)
        for (case in cases) {
            val name = case.get("name").asString
            val expected = case.getAsJsonObject("expected")
            val resolution = LayoutDetector.frozenWindowLayout(evidenceOf(case))
            assertEquals(
                name,
                expected.getAsJsonArray("columns").map { column -> column.asJsonArray.map { it.asString } },
                resolution.layout?.columns?.map(LayoutColumn::files).orEmpty(),
            )
            resolution.layout?.let { layout ->
                assertEquals(name, expected.get("column_order").asString, layout.columnOrder)
            }
            assertEquals(name, expected.get("reason").nullableString(), resolution.reason)
            assertEquals(name, expected.get("observed_line").nullableString(), resolution.observedLine)
        }
    }

    @Test
    fun `GH 326 window evidence is the native wire shape`() {
        val json = com.google.gson.Gson().toJson(
            LayoutDetector.WindowLayoutEvidence(
                basePath = "/repo",
                windows = listOf(
                    LayoutDetector.EditorWindowEvidence(
                        x = null,
                        y = null,
                        selected = "/repo/a.md",
                        sessionTabs = listOf("/repo/a.md"),
                    ),
                ),
                history = listOf("/repo/a.md"),
            ),
        )
        val root = com.google.gson.JsonParser.parseString(json).asJsonObject
        assertEquals("/repo", root.get("base_path").asString)
        val window = root.getAsJsonArray("windows")[0].asJsonObject
        assertEquals("/repo/a.md", window.get("selected").asString)
        assertEquals("/repo/a.md", window.getAsJsonArray("session_tabs")[0].asString)
        assertFalse("an unmeasured origin is absent, never 0", window.has("x"))
    }

    @Test
    fun `GH 326 a native window layout answer wins and keeps empty columns`() {
        val evidence = evidenceOf(windowLayoutParityCases().first())
        var asked: String? = null
        val layout = LayoutDetector.detectEditorLayout(evidence) { derivation, inputJson ->
            asked = derivation
            assertTrue(inputJson.contains("session_tabs"))
            """{"columns":[{"files":[]},{"files":["tasks/right.md"]}],"column_order":"retained",""" +
                """"snapshots":[],"observed_line":"[layout-detect] observed native"}"""
        }
        assertEquals("window_layout", asked)
        assertEquals(
            EditorLayout(
                listOf(LayoutColumn(emptyList()), LayoutColumn(listOf("tasks/right.md"))),
                columnOrder = "retained",
            ),
            layout,
        )
        assertEquals(
            null,
            LayoutDetector.detectEditorLayout(evidence) { _, _ ->
                """{"columns":[],"column_order":"editor","reason":"later_rule"}"""
            },
        )
    }

    @Test
    fun `GH 326 no native answer takes the frozen floor`() {
        val case = windowLayoutParityCases().first()
        val evidence = evidenceOf(case)
        val frozen = LayoutDetector.frozenWindowLayout(evidence).layout
        assertEquals(frozen, LayoutDetector.detectEditorLayout(evidence, EditorDerivation.Unavailable))
        assertEquals(frozen, LayoutDetector.detectEditorLayout(evidence) { _, _ -> "not json" })
    }

    @Test
    fun `GH 326 an EDT caller runs the native window layout derivation off the EDT`() {
        val evidence = evidenceOf(windowLayoutParityCases().first())
        val ranOffEdt = AtomicBoolean(false)
        val detected = AtomicReference<EditorLayout?>()
        SwingUtilities.invokeAndWait {
            detected.set(
                LayoutDetector.detectEditorLayout(evidence) { _, _ ->
                    ranOffEdt.set(!SwingUtilities.isEventDispatchThread())
                    """{"columns":[{"files":["a.md"]},{"files":["b.md"]}],"column_order":"editor"}"""
                },
            )
        }
        assertTrue("the native derivation must never run on the EDT", ranOffEdt.get())
        assertEquals(
            EditorLayout(listOf(LayoutColumn(listOf("a.md")), LayoutColumn(listOf("b.md")))),
            detected.get(),
        )
    }
}
