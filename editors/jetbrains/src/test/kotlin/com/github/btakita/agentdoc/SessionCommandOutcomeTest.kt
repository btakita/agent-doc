package com.github.btakita.agentdoc

import com.google.gson.Gson
import com.google.gson.JsonElement
import com.google.gson.JsonNull
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
 * `#jbsessioncmdoutcome`: the Clear Session Context and Restart output rules behind the native
 * `session_command_outcome` derivation.
 */
class SessionCommandOutcomeTest {
    private val gson = Gson()

    private fun fixture(): JsonObject {
        val stream = javaClass.classLoader.getResourceAsStream("editor-derivations/session_command_outcome.json")
            ?: error("session command outcome parity fixture not on the test classpath")
        val raw = stream.bufferedReader(Charsets.UTF_8).use { it.readText() }
        return JsonParser.parseString(raw).asJsonObject
    }

    private fun Long?.json(): JsonElement = this?.let { gson.toJsonTree(it) } ?: JsonNull.INSTANCE

    /** The answer in the fixture's wire shape. */
    private fun wire(result: SessionCommandOutcome.Result): JsonObject =
        JsonObject().apply {
            addProperty("kind", SessionCommandOutcome.token(result.kind))
            add("summary", result.summary?.let { gson.toJsonTree(it) } ?: JsonNull.INSTANCE)
            add(
                "clear_queued",
                result.clearQueued?.let {
                    JsonObject().apply {
                        addProperty("file", it.file)
                        addProperty("pane", it.pane)
                        addProperty("evidence", it.evidence)
                        add("evidence_age_secs", it.evidenceAgeSecs.json())
                        addProperty("rearmed", it.rearmed)
                        add("settles_in_secs", it.settlesInSecs.json())
                        add("pre_turn_grace_secs", it.preTurnGraceSecs.json())
                    }
                } ?: JsonNull.INSTANCE,
            )
            add(
                "clear_refused",
                result.clearRefused?.let {
                    JsonObject().apply {
                        addProperty("file", it.file)
                        addProperty("pane", it.pane)
                        addProperty("source", it.source)
                        addProperty("current_command", it.currentCommand)
                        addProperty("tail", it.tail)
                        addProperty("protected_reason", it.protectedReason)
                        addProperty("live_turn_evidence", it.liveTurnEvidence)
                    }
                } ?: JsonNull.INSTANCE,
            )
            add(
                "restart_refused_busy",
                result.restartRefusedBusy?.let {
                    JsonObject().apply {
                        addProperty("file", it.file)
                        addProperty("pane", it.pane)
                        addProperty("source", it.source)
                        addProperty("current_command", it.currentCommand)
                        addProperty("tail", it.tail)
                    }
                } ?: JsonNull.INSTANCE,
            )
            add(
                "restart_refused_starting",
                result.restartRefusedStarting?.let {
                    JsonObject().apply {
                        addProperty("file", it.file)
                        addProperty("reason", it.reason)
                    }
                } ?: JsonNull.INSTANCE,
            )
            add(
                "restart_refused_editor_holds_pane",
                result.restartRefusedEditorHoldsPane?.let {
                    JsonObject().apply {
                        addProperty("file", it.file)
                        addProperty("pane", it.pane)
                        addProperty("editor", it.editor)
                        addProperty("source", it.source)
                        addProperty("current_command", it.currentCommand)
                        addProperty("tail", it.tail)
                    }
                } ?: JsonNull.INSTANCE,
            )
        }

    private fun command(token: String): SessionCommandOutcome.Command? =
        SessionCommandOutcome.Command.values().firstOrNull { it.token == token }

    @Test
    fun `frozen session-command floor answers every parity case and sends exactly its input`() {
        // The same file pins the native derivation in Rust
        // (session_command_outcome_answers_the_shared_parity_cases).
        val cases = fixture().getAsJsonArray("cases").map { it.asJsonObject }
        assertTrue("the parity file lost cases", cases.size >= 30)
        var answered = 0
        for (case in cases) {
            val name = case.get("name").asString
            val input = case.getAsJsonObject("input")
            // A command this plugin never sends is native-only: it pins the Rust `generic` answer.
            val command = command(input.get("command").asString) ?: continue
            val succeeded = input.get("succeeded").asBoolean
            val force = input.get("force").asBoolean
            val output = input.get("output").asString
            val relativePath = input.get("relative_path").asString
            assertEquals(
                name,
                input,
                JsonParser.parseString(
                    gson.toJson(SessionCommandOutcome.evidence(command, succeeded, force, output, relativePath)),
                ).asJsonObject,
            )
            val frozenExpected = case.getAsJsonObject("frozen_expected") ?: case.getAsJsonObject("expected")
            assertEquals(name, frozenExpected, wire(SessionCommandOutcome.frozen(command, succeeded, force, output, relativePath)))
            answered++
        }
        assertTrue("the floor answered too few cases", answered >= 30)
    }

    @Test
    fun `every kind the plugin executes has a wire token the parity file uses`() {
        val used = fixture().getAsJsonArray("cases").map { it.asJsonObject.getAsJsonObject("expected").get("kind").asString }.toSet()
        for (kind in SessionCommandOutcome.Kind.values()) {
            assertTrue("no parity case for ${kind.name}", SessionCommandOutcome.token(kind) in used)
        }
    }

    @Test
    fun `the plugin sends its evidence and executes the native answer`() {
        var asked: String? = null
        var sent: String? = null
        val result =
            SessionCommandOutcome.derive(SessionCommandOutcome.Command.CLEAR, false, false, "Error: new shape", "plan.md") {
                    derivation, input ->
                asked = derivation
                sent = input
                // A shape and wording the frozen floor does not have: the native answer wins.
                """{"kind":"clear_refused","summary":"Pane %7 is busy with plan.md.","clear_queued":null,""" +
                    """"clear_refused":{"file":"/r/plan.md","pane":"%7","source":"registry","current_command":"codex",""" +
                    """"tail":"","protected_reason":"","live_turn_evidence":""},"restart_refused_busy":null,""" +
                    """"restart_refused_starting":null,"restart_refused_editor_holds_pane":null}"""
            }
        assertEquals("session_command_outcome", asked)
        assertEquals(
            """{"command":"clear","succeeded":false,"force":false,"output":"Error: new shape","relative_path":"plan.md"}""",
            sent,
        )
        assertEquals(SessionCommandOutcome.Kind.CLEAR_REFUSED, result.kind)
        assertEquals("Pane %7 is busy with plan.md.", result.summary)
        assertEquals("%7", result.clearRefused?.pane)
        assertEquals("codex", result.clearRefused?.currentCommand)
    }

    @Test
    fun `a kind this generation cannot execute, a refusal without details, or no answer takes the frozen floor`() {
        val output = "Error: session_restart refused for /r/plan.md because pane %2 is alive-busy (source=registry)"
        val floor = SessionCommandOutcome.frozen(SessionCommandOutcome.Command.RESTART, false, false, output, "plan.md")
        assertEquals(SessionCommandOutcome.Kind.RESTART_REFUSED_BUSY, floor.kind)
        fun derive(answer: String?, derivation: EditorDerivation = EditorDerivation { _, _ -> answer }) =
            SessionCommandOutcome.derive(SessionCommandOutcome.Command.RESTART, false, false, output, "plan.md", derivation)
        assertEquals(floor, derive("""{"kind":"brand_new_kind","summary":"x"}"""))
        assertEquals(floor, derive("""{"kind":"restart_refused_starting","summary":"x","restart_refused_starting":null}"""))
        assertEquals(floor, derive(null, EditorDerivation.Unavailable))
        assertEquals(floor, derive("not json"))
    }

    @Test
    fun `a summary an older library leaves out takes the floor's`() {
        val output = "session_clear deferred for /r/plan.md"
        val result =
            SessionCommandOutcome.derive(SessionCommandOutcome.Command.CLEAR, true, false, output, "plan.md") { _, _ ->
                """{"kind":"clear_deferred"}"""
            }
        assertEquals(SessionCommandOutcome.Kind.CLEAR_DEFERRED, result.kind)
        assertEquals(TerminalUtil.buildDeferredQueuePreemptClearMessage("plan.md"), result.summary)
        assertNull(result.clearQueued)
    }

    @Test
    fun `clear and restart derive their outcome natively and only the floor reads their output`() {
        val source = Files.readString(Paths.get("src/main/kotlin/com/github/btakita/agentdoc/TerminalUtil.kt"))
        val restart = source.substringAfter("private fun runRestartCommand(").substringBefore("internal fun interruptAndRestartSession(")
        val clear = source.substringAfter("fun clearSessionContext(").substringBefore("fun refreshAndRetryClearSessionContext(")
        assertTrue(restart.contains("SessionCommandOutcome.derive("))
        assertTrue(clear.contains("SessionCommandOutcome.derive("))
        val parsers = listOf(
            "parseBusySessionClearRefusal(",
            "parseLiveTurnQueuedClear(",
            "isDeferredQueuePreemptClear(",
            "parseBusySessionRestartRefusal(",
            "parseStartingSessionRestartRefusal(",
            "parseEditorHoldsPaneRestartRefusal(",
        )
        for (parser in parsers) {
            assertFalse("restart re-reads its output with $parser", restart.contains(parser))
            assertFalse("clear re-reads its output with $parser", clear.contains(parser))
        }
        // The floor is the only caller of the pre-move parsers outside TerminalUtil.
        val main = Paths.get("src/main/kotlin/com/github/btakita/agentdoc")
        Files.list(main).use { files ->
            files.filter { it.fileName.toString().endsWith(".kt") }.forEach { file ->
                val name = file.fileName.toString()
                if (name != "SessionCommandOutcome.kt" && name != "TerminalUtil.kt") {
                    val text = Files.readString(file)
                    for (parser in parsers) {
                        assertFalse("$name reads session command output outside session_command_outcome", text.contains(parser))
                    }
                }
            }
        }
        // Outside the floor's own definitions, TerminalUtil calls none of them either.
        val callers = source.substringBefore("internal fun isDeferredQueuePreemptClear(")
        for (parser in parsers) {
            assertFalse("TerminalUtil reads session command output with $parser", callers.contains("$parser"))
        }
    }
}
