package com.github.btakita.agentdoc

import com.google.gson.Gson
import com.google.gson.JsonNull
import com.google.gson.JsonObject
import com.google.gson.JsonParser
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.nio.file.Files
import java.nio.file.Paths

/** `#jbshrinkrestarts`: the Run Agent Doc route-output rules behind the native `route_outcome` derivation. */
class RouteOutcomeTest {
    private val gson = Gson()

    private fun fixture(): JsonObject {
        val stream = javaClass.classLoader.getResourceAsStream("editor-derivations/route_outcome.json")
            ?: error("route outcome parity fixture not on the test classpath")
        val raw = stream.bufferedReader(Charsets.UTF_8).use { it.readText() }
        return JsonParser.parseString(raw).asJsonObject
    }

    /** The answer in the fixture's wire shape. */
    private fun wire(result: RouteOutcome.Result): JsonObject =
        JsonObject().apply {
            addProperty("kind", RouteOutcome.token(result.kind))
            add("summary", result.summary?.let { gson.toJsonTree(it) } ?: JsonNull.INSTANCE)
            add(
                "queue_paused",
                result.queuePaused?.let {
                    JsonObject().apply {
                        addProperty("reason", it.reason)
                        addProperty("restart_supervisor_redirect", it.restartSupervisorRedirect)
                        addProperty("stale_pid", it.stalePid)
                    }
                } ?: JsonNull.INSTANCE,
            )
            add(
                "agent_switch_deferred",
                result.agentSwitchDeferred?.let {
                    JsonObject().apply {
                        addProperty("previous_harness", it.previousHarness)
                        addProperty("target_harness", it.targetHarness)
                        addProperty("queue_paused", it.queuePaused)
                        addProperty("force_required", it.forceRequired)
                        addProperty("supervisor_unavailable", it.supervisorUnavailable)
                        addProperty("restart_in_flight", it.restartInFlight)
                    }
                } ?: JsonNull.INSTANCE,
            )
            addProperty("snapshot_path", result.snapshotPath)
            add(
                "owned_by_other_lane_notice",
                result.ownedByOtherLaneNotice?.let { gson.toJsonTree(it) } ?: JsonNull.INSTANCE,
            )
            add(
                "pane_not_visible_notice",
                result.paneNotVisibleNotice?.let { gson.toJsonTree(it) } ?: JsonNull.INSTANCE,
            )
        }

    @Test
    fun `frozen route-outcome floor answers every parity case and sends exactly its input`() {
        // The same file pins the native derivation in Rust
        // (route_outcome_answers_the_shared_parity_cases).
        val cases = fixture().getAsJsonArray("cases").map { it.asJsonObject }
        assertTrue("the parity file lost cases", cases.size >= 32)
        for (case in cases) {
            val name = case.get("name").asString
            val input = case.getAsJsonObject("input")
            val output = input.get("output").asString
            val relativePath = input.get("relative_path").asString
            assertEquals(
                name,
                input,
                JsonParser.parseString(gson.toJson(RouteOutcome.evidence(output, relativePath))).asJsonObject,
            )
            val frozenExpected = case.getAsJsonObject("frozen_expected") ?: case.getAsJsonObject("expected")
            assertEquals(name, frozenExpected, wire(RouteOutcome.frozen(output, relativePath)))
        }
    }

    @Test
    fun `every kind the plugin executes has a wire token the parity file uses`() {
        val used = fixture().getAsJsonArray("cases").map { it.asJsonObject.getAsJsonObject("expected").get("kind").asString }.toSet()
        for (kind in TerminalUtil.RunAgentDocRouteFailureKind.values()) {
            assertTrue("no parity case for ${kind.name}", RouteOutcome.token(kind) in used)
        }
    }

    @Test
    fun `the plugin sends the output and executes the native answer`() {
        var asked: String? = null
        var sent: String? = null
        val result =
            RouteOutcome.derive("Error: something new", "plan.md") { derivation, input ->
                asked = derivation
                sent = input
                // A wording the frozen floor does not have: the native answer wins.
                """{"kind":"busy_running","summary":"Still busy with plan.md.","queue_paused":null,""" +
                    """"agent_switch_deferred":null,"snapshot_path":"/tmp/s.txt",""" +
                    """"owned_by_other_lane_notice":null,"pane_not_visible_notice":null}"""
            }
        assertEquals("route_outcome", asked)
        assertEquals("""{"output":"Error: something new","relative_path":"plan.md"}""", sent)
        assertEquals(TerminalUtil.RunAgentDocRouteFailureKind.BUSY_RUNNING, result.kind)
        assertEquals("Still busy with plan.md.", result.summary)
        assertEquals("/tmp/s.txt", result.snapshotPath)
    }

    @Test
    fun `a kind this generation cannot execute, no answer, or an unreadable one takes the frozen floor`() {
        val output = "Error: controller socket closed"
        val floor = RouteOutcome.frozen(output, "plan.md")
        assertEquals(floor, RouteOutcome.derive(output, "plan.md") { _, _ -> """{"kind":"brand_new_kind","summary":"x"}""" })
        assertEquals(floor, RouteOutcome.derive(output, "plan.md", EditorDerivation.Unavailable))
        assertEquals(floor, RouteOutcome.derive(output, "plan.md") { _, _ -> "not json" })
    }

    @Test
    fun `a field an older library leaves out takes the floor's value`() {
        val output = "[route] ok\n[route] pane-not-visible: pane %4 is hidden"
        val result = RouteOutcome.derive(output, "plan.md") { _, _ -> """{"kind":"persistent"}""" }
        assertEquals(TerminalUtil.RunAgentDocRouteFailureKind.PERSISTENT, result.kind)
        assertEquals("[route] pane-not-visible: pane %4 is hidden", result.paneNotVisibleNotice)
    }

    @Test
    fun `Run Agent Doc derives its route outcome natively and executes it on the route thread`() {
        val source = Files.readString(Paths.get("src/main/kotlin/com/github/btakita/agentdoc/TerminalUtil.kt"))
        val route = source.substringAfter("val startedAt = System.currentTimeMillis()")
            .substringBefore("private fun routeAttemptError")
        assertTrue(route.contains("val outcome = RouteOutcome.derive(output, relativePath)"))
        assertFalse(
            "the route must not re-read its output in Kotlin",
            route.contains("classifyRunAgentDocRouteFailure(") ||
                route.contains("routeOwnedByOtherLaneNotice(output)") ||
                route.contains("routePaneNotVisibleNotice(output)"),
        )
        // The floor is the only caller of the pre-move rules outside their own tests.
        val main = Paths.get("src/main/kotlin/com/github/btakita/agentdoc")
        Files.list(main).use { files ->
            files.filter { it.fileName.toString().endsWith(".kt") }.forEach { file ->
                val text = Files.readString(file)
                val name = file.fileName.toString()
                if (name != "RouteOutcome.kt" && name != "TerminalUtil.kt") {
                    assertFalse("$name reads route output outside route_outcome", text.contains("classifyRunAgentDocRouteFailure("))
                }
            }
        }
    }
}
