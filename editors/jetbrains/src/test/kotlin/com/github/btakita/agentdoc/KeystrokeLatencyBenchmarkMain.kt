package com.github.btakita.agentdoc

import com.google.gson.GsonBuilder
import com.google.gson.JsonObject
import com.google.gson.JsonParser
import java.io.File
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit

/**
 * `#jbshrinkrestarts`: the per-keystroke latency gate for moving plugin rules behind the
 * hot-reloaded native library.
 *
 * A composition root only, like [main] in `CrossEditorHarnessMain.kt`: every timed call goes
 * through the shipped [AgentDocLib] proxy (generation guard, lane dispatch, JNA marshalling,
 * UTF-8 conversion) and the shipped Kotlin helpers, against a real `libagent_doc` resolved the
 * production way (`agent-doc lib-path` on `PATH`). Controller and socket IPC are excluded on
 * purpose: the document lives outside any `.agent-doc` project, so the observed-content FFI does
 * its marshalling and session check but never reaches a controller, and the replica flush stops
 * at the incremental diff the forwarder would push. What remains is exactly what a plugin-side
 * rule move could change.
 *
 * Cases (microseconds per operation):
 * - `keystroke.typing_report`: TypingTracker's quiet-boundary report, the whole buffer through
 *   `agent_doc_lazily_current_observed_v1`.
 * - `keystroke.replica_flush_1` / `keystroke.replica_flush_16`: CrdtReplicaManager's splice
 *   flush for a 1- and a 16-keystroke burst: [prepareLocalEditorEditsUtil], one
 *   `agent_doc_replica_apply_local` per splice, then the state vector and incremental diff.
 * - `boundary.worker_hop`: one hand-off to a single-thread worker and back, the fixed cost of
 *   leaving the EDT for any native call.
 * - `boundary.derive_capabilities`: the smallest `agent_doc_editor_derive_json` round trip
 *   (request JSON, JNA, native JSON parse, answer JSON, Gson parse).
 * - `move.*`: a moved rule as production asks it (`native`: the derivation channel, its answer
 *   parsed over the frozen floor) beside the frozen Kotlin floor alone (`frozen`), on one input.
 *
 * Environment:
 * - `AGENT_DOC_BENCH_FIXTURE`: a session document to type into (a copy, never a live document).
 *   Absent, a deterministic ~120 KB synthetic session document is generated.
 * - `AGENT_DOC_BENCH_ROUNDS` (default 5): the cases run interleaved, once per round, so a slow
 *   stretch of the machine lands on every case. `p50_best` is the median of the fastest round.
 * - `AGENT_DOC_BENCH_ITERATIONS` (default 1000 per round; the replica flush takes an eighth) and
 *   `AGENT_DOC_BENCH_WARMUP` (default 500, first round only).
 * - `AGENT_DOC_BENCH_OUT`: where to write the JSON report (also printed to stdout).
 * - `AGENT_DOC_BENCH_BASELINE`: an earlier report; each shared case prints its p50/p99 delta and
 *   the run fails when a keystroke case regresses past the gate.
 */
fun main() {
    val iterations = System.getenv("AGENT_DOC_BENCH_ITERATIONS")?.toIntOrNull() ?: 1000
    val warmup = System.getenv("AGENT_DOC_BENCH_WARMUP")?.toIntOrNull() ?: 500
    val rounds = System.getenv("AGENT_DOC_BENCH_ROUNDS")?.toIntOrNull() ?: 5
    val fixturePath = System.getenv("AGENT_DOC_BENCH_FIXTURE")?.takeIf { it.isNotBlank() }
    val fixture = fixturePath?.let { File(it).readText(Charsets.UTF_8) } ?: KeystrokeBenchmark.syntheticSessionDocument()
    val lib = AgentDocLib.get() ?: error("no libagent_doc resolved: put an agent-doc build on PATH")
    val bench = KeystrokeBenchmark(lib, fixture, iterations, warmup, rounds = rounds)
    val report = bench.run()
    report.addProperty("fixture", fixturePath ?: "synthetic")
    val gson = GsonBuilder().setPrettyPrinting().create()
    val json = gson.toJson(report)
    println(json)
    System.getenv("AGENT_DOC_BENCH_OUT")?.takeIf { it.isNotBlank() }?.let { File(it).writeText(json + "\n") }
    val baseline = System.getenv("AGENT_DOC_BENCH_BASELINE")?.takeIf { it.isNotBlank() }
    if (baseline != null) {
        val verdict = KeystrokeBenchmark.compare(JsonParser.parseString(File(baseline).readText()).asJsonObject, report)
        verdict.lines.forEach(::println)
        if (!verdict.passed) {
            System.err.println("[bench] keystroke latency regressed past the gate")
            kotlin.system.exitProcess(1)
        }
    }
    kotlin.system.exitProcess(0)
}

internal class KeystrokeBenchmark(
    private val lib: AgentDocLib,
    private val fixture: String,
    private val iterations: Int,
    private val warmup: Int,
    /** The replica flush costs milliseconds per burst, so it runs fewer samples. */
    private val flushIterations: Int = maxOf(1, iterations / 8),
    private val flushWarmup: Int = maxOf(1, warmup / 8),
    private val rounds: Int = 5,
) {
    /** A rule asked through the derivation channel beside its frozen Kotlin floor. */
    internal class MovedRule(
        val name: String,
        val native: () -> Any?,
        val frozen: () -> Any?,
    )

    internal data class Verdict(val passed: Boolean, val lines: List<String>)

    /** Every sample of each case, in measurement order, and the median of each round. */
    private val cases = linkedMapOf<String, MutableList<Long>>()
    private val roundMedians = linkedMapOf<String, MutableList<Long>>()

    /** Moved rules the measured library does not answer. */
    private val skipped = mutableListOf<String>()

    /** Per-step samples of a composite case, recorded only outside its warmup. */
    private var recording = false

    /** One timed operation; [prepare] runs once, before its first warmup. */
    private inner class Case(
        val name: String,
        val count: Int,
        val warm: Int,
        val prepare: () -> Unit = {},
        val close: () -> Unit = {},
        val op: () -> Any?,
    )

    fun run(): JsonObject {
        val worker = Executors.newSingleThreadExecutor { r -> Thread(r, "bench-native-worker").apply { isDaemon = true } }
        val flush1 = ReplicaFlush("keystroke.replica_flush_1", burst = 1)
        val flush16 = ReplicaFlush("keystroke.replica_flush_16", burst = 16)
        val keystroke =
            listOf(
                Case("keystroke.typing_report", iterations, warmup) { typingReport() },
                Case(flush1.name, flushIterations, flushWarmup, flush1::open, flush1::close) { flush1.flush() },
                Case(flush16.name, flushIterations, flushWarmup, flush16::open, flush16::close) { flush16.flush() },
                Case("boundary.worker_hop", iterations, warmup) { worker.submit<Int> { 1 }.get() },
                Case("boundary.derive_capabilities", iterations, warmup) {
                    NativeAdminControls.deriveEditor("capabilities", "{}") ?: error("derive_json unavailable")
                },
            )
        // A library that predates a moved rule (a baseline build) skips its cases, so one harness
        // measures both sides of a move.
        val listed = NativeAdminControls.deriveEditor("capabilities", "{}").orEmpty()
        val moved =
            movedRules().flatMap { rule ->
                if (!listed.contains("\"${rule.name}\"")) {
                    skipped += rule.name
                    emptyList()
                } else {
                    listOf(
                        Case("move.${rule.name}.native", iterations, warmup) { rule.native() },
                        Case("move.${rule.name}.frozen", iterations, warmup) { rule.frozen() },
                    )
                }
            }
        val all = keystroke + moved
        try {
            all.forEach { it.prepare() }
            // The keystroke phase runs alone and first, so the gated cases see the same process
            // whatever moved rules a build adds. Within a phase, rounds interleave the cases, so
            // a slow stretch of the machine lands on every case. The gate reads the best round.
            for (phase in listOf(keystroke, moved)) {
                System.gc()
                for (round in 1..rounds) {
                    for (case in phase) {
                        recording = false
                        if (round == 1) repeat(case.warm) { case.op() }
                        recording = true
                        val samples = LongArray(case.count)
                        for (i in 0 until case.count) {
                            val started = System.nanoTime()
                            case.op()
                            samples[i] = System.nanoTime() - started
                        }
                        recording = false
                        cases.getOrPut(case.name) { ArrayList() }.addAll(samples.asList())
                        roundMedians.getOrPut(case.name) { ArrayList() } += samples.sortedArray()[(samples.size - 1) / 2]
                    }
                    if (phase === keystroke) for (flush in listOf(flush1, flush16)) flush.closeRound()
                }
            }
        } finally {
            all.forEach { it.close() }
            worker.shutdownNow()
            worker.awaitTermination(1, TimeUnit.SECONDS)
        }
        return JsonObject().apply {
            addProperty("benchmark", "jetbrains_keystroke_latency_v1")
            addProperty("unit", "microseconds")
            val about = AgentDocLib.aboutSnapshot()
            addProperty("library", about.path)
            addProperty("library_version", about.version)
            addProperty("fixture_bytes", fixture.toByteArray(Charsets.UTF_8).size)
            addProperty("fixture_utf16", fixture.length)
            addProperty("rounds", rounds)
            add("skipped_moves", com.google.gson.JsonArray().apply { skipped.forEach(::add) })
            addProperty("iterations", iterations)
            addProperty("warmup", warmup)
            addProperty("flush_iterations", flushIterations)
            addProperty("flush_warmup", flushWarmup)
            addProperty("java", System.getProperty("java.version"))
            add(
                "cases",
                JsonObject().apply {
                    cases.forEach { (name, samples) -> add(name, summary(samples.toLongArray(), roundMedians[name].orEmpty())) }
                },
            )
        }
    }

    /** The rules moved behind the native boundary; each one adds its case here. */
    private fun movedRules(): List<MovedRule> =
        listOf(
            // TerminalUtil: one finished Run Agent Doc route's output, read once per route.
            MovedRule(
                "route_outcome",
                // What production runs: the native answer parsed over the frozen floor.
                native = { RouteOutcome.derive(ROUTE_OUTPUT, "plan.md") },
                frozen = { RouteOutcome.frozen(ROUTE_OUTPUT, "plan.md") },
            ),
            // TerminalUtil: one finished Clear Session Context command's output, read once per press.
            MovedRule(
                "session_command_outcome",
                native = {
                    SessionCommandOutcome.derive(SessionCommandOutcome.Command.CLEAR, false, false, CLEAR_REFUSAL_OUTPUT, "plan.md")
                },
                frozen = {
                    SessionCommandOutcome.frozen(SessionCommandOutcome.Command.CLEAR, false, false, CLEAR_REFUSAL_OUTPUT, "plan.md")
                },
            ),
            // PatchWatcher: the prefix repair of a patched document, once per patch that names
            // targets. The worst case: the whole document crosses both ways.
            MovedRule(
                "exchange_prefix_repair",
                native = { ExchangePrefixRepair.repair(unprefixedFixture, PREFIX_TARGETS) },
                frozen = { ExchangePrefixRepair.frozen(unprefixedFixture, PREFIX_TARGETS) },
            ),
        )

    /** The fixture with its operator line unprefixed, so the repair changes it. */
    private val unprefixedFixture: String by lazy {
        val anchor = typingAnchor(fixture)
        val unprefixed = fixture.substring(0, anchor) + PREFIX_TARGETS.single() + "\n" + fixture.substring(anchor)
        check(ExchangePrefixRepair.frozen(unprefixed, PREFIX_TARGETS) != unprefixed) {
            "the prefix case must change the document"
        }
        unprefixed
    }

    private fun typingReport() {
        lib.agent_doc_lazily_current_observed_v1(
            BENCH_DOCUMENT_PATH,
            fixture,
            "jetbrains:bench",
            "jetbrains",
            "bench",
            "",
            1,
        )
    }

    /**
     * Type [burst] characters at the end of the exchange's operator region, then flush them the
     * way the replica worker does: the state vector the forwarder diffs against, the splice
     * preparation, one `apply_local` per splice, and the incremental diff it would push. Every
     * flush types into the text the previous one produced. Each step is also its own case.
     */
    private inner class ReplicaFlush(val name: String, private val burst: Int) {
        private val clientId = BENCH_CLIENT_ID_BASE + burst
        private val node = NativeReplicaNode()
        private var shadow = fixture
        private var anchor = typingAnchor(fixture)
        private var typed = 0L
        private val steps = linkedMapOf<String, LongArray>()
        private var stepCount = 0

        fun open() {
            lib.agent_doc_replica_close(clientId)
            check(node.open(clientId, null)) { "replica_open failed" }
            check(node.applyLocal(clientId, 0, 0, fixture)) { "replica seed failed" }
        }

        fun close() {
            node.close(clientId)
        }

        fun flush() {
            val edits = ArrayList<CapturedLocalEditorEdit>(burst)
            var length = shadow.length
            repeat(burst) {
                // One in eight keystrokes is a backspace over the character just typed.
                edits +=
                    if (typed % 8 == 7L) {
                        CapturedLocalEditorEdit(anchor - 1, "x", "", 0L, length).also {
                            anchor -= 1
                            length -= 1
                        }
                    } else {
                        CapturedLocalEditorEdit(anchor, "", "x", 0L, length).also {
                            anchor += 1
                            length += 1
                        }
                    }
                typed += 1
            }
            val t0 = System.nanoTime()
            val before = node.stateVector() ?: error("state vector unavailable")
            val t1 = System.nanoTime()
            val batch = prepareLocalEditorEditsUtil(shadow, edits) ?: error("burst did not prepare")
            val t2 = System.nanoTime()
            for (edit in batch.edits) {
                check(node.applyLocal(clientId, edit.offsetCodePoints, edit.deleteCodePoints, edit.insert))
            }
            val t3 = System.nanoTime()
            node.diff(before) ?: error("diff unavailable")
            val t4 = System.nanoTime()
            shadow = batch.resultingText
            if (recording) {
                record("state_vector", t1 - t0)
                record("prepare", t2 - t1)
                record("apply_local", t3 - t2)
                record("diff", t4 - t3)
                stepCount += 1
            }
        }

        private fun record(step: String, nanos: Long) {
            val samples = steps.getOrPut(step) { LongArray(flushIterations) }
            samples[stepCount] = nanos
        }

        /** Publish this round's per-step samples as `<case>.<step>` cases. */
        fun closeRound() {
            for ((step, samples) in steps) {
                val taken = samples.copyOf(stepCount)
                cases.getOrPut("$name.$step") { ArrayList() }.addAll(taken.asList())
                roundMedians.getOrPut("$name.$step") { ArrayList() } += taken.sortedArray()[(taken.size - 1) / 2]
            }
            steps.clear()
            stepCount = 0
        }
    }

    private fun summary(samples: LongArray, medians: List<Long>): JsonObject {
        val sorted = samples.sortedArray()
        fun pct(p: Double): Double = micros(sorted[((sorted.size - 1) * p).toInt()])
        return JsonObject().apply {
            // The median of the fastest round: the gate's figure, least disturbed by the machine.
            addProperty("p50_best", micros(medians.minOrNull() ?: sorted[(sorted.size - 1) / 2]))
            addProperty("p50", pct(0.50))
            addProperty("p90", pct(0.90))
            addProperty("p99", pct(0.99))
            addProperty("max", micros(sorted.last()))
            addProperty("mean", micros(sorted.average().toLong()))
        }
    }

    private fun micros(nanos: Long): Double = Math.round(nanos / 100.0) / 10.0

    internal companion object {
        const val BENCH_CLIENT_ID_BASE = 0x6B65_7900L
        /** Outside every `.agent-doc` project, so no controller is ever contacted. */
        val BENCH_DOCUMENT_PATH: String =
            File(System.getProperty("java.io.tmpdir"), "agent-doc-keystroke-bench/session.md").absolutePath

        /** An operator prompt the prefix case types, unprefixed, into the operator region. */
        val PREFIX_TARGETS = listOf("please run the next step")

        /** A representative route output: an unproven dispatch with its snapshot and attempt. */
        val ROUTE_OUTPUT: String =
            "[route] preserved dispatch-start proof snapshot for plan.md pane %12 harness=codex " +
                "phase=direct_pane_dispatch_start_unproven capture_len=1234 capture_hash=e5b665883cc3 " +
                "snapshot_path=/repo/.agent-doc/logs/route-submit/1781670099279-unproven.txt " +
                "editor_attempt_id=1781670087547-4\n" +
                "Error: dispatch-only codex reopen for plan.md was accepted in pane %12 via direct_pane_submit " +
                "(tmux_text_enter), but only pane-input acceptance proof was available after waiting 10s; " +
                "treating this as not dispatched because no dispatch-start proof was recorded."

        /** A representative clear output: a busy-pane refusal with its source, command, and quoted tail. */
        val CLEAR_REFUSAL_OUTPUT: String =
            "agent-doc command failed (exit 1): Error: session_clear refused for /repo/plan.md because pane %12 " +
                "is alive-busy (source=authoritative_actor, current_command=agent-doc, " +
                "tail=\"gpt-5.5 high \u00b7 ~/repo \u00b7 Context 15% used\"). Run `agent-doc session status /repo/plan.md` " +
                "and wait for an idle prompt, or inspect/stop the pane explicitly before clearing or restarting it."

        /** A keystroke case may grow by this much (or this fraction) before the gate fails. */
        const val GATE_ABSOLUTE_MICROS = 5.0
        const val GATE_RELATIVE = 0.15

        /** Where an operator types: the end of the exchange, before its boundary marker. */
        fun typingAnchor(text: String): Int {
            val boundary = text.lastIndexOf("<!-- agent:boundary:")
            if (boundary > 0) return boundary
            val close = text.lastIndexOf("<!-- /agent:exchange -->")
            return if (close > 0) close else text.length
        }

        /** The cases the gate fails on; their `.<step>` breakdowns are reported only. */
        val GATED_CASES =
            setOf("keystroke.typing_report", "keystroke.replica_flush_1", "keystroke.replica_flush_16")

        fun compare(baseline: JsonObject, current: JsonObject): Verdict {
            val before = baseline.getAsJsonObject("cases")
            val after = current.getAsJsonObject("cases")
            val lines = mutableListOf<String>()
            var passed = true
            for (name in after.keySet().filter(before::has)) {
                val b = before.getAsJsonObject(name)
                val a = after.getAsJsonObject(name)
                val gated = name in GATED_CASES
                val parts =
                    listOf("p50_best", "p99").filter { b.has(it) && a.has(it) }.map { key ->
                        val bv = b.get(key).asDouble
                        val av = a.get(key).asDouble
                        val regressed = av - bv > maxOf(GATE_ABSOLUTE_MICROS, bv * GATE_RELATIVE)
                        if (gated && key == "p50_best" && regressed) passed = false
                        "$key ${"%.1f".format(bv)} -> ${"%.1f".format(av)}us" + if (regressed) " (slower)" else ""
                    }
                lines += "[bench] ${if (gated) "gate" else "info"} $name: ${parts.joinToString(", ")}"
            }
            lines += "[bench] verdict: ${if (passed) "no keystroke regression" else "REGRESSION"}"
            return Verdict(passed, lines)
        }

        /**
         * A deterministic ~120 KB session document shaped like a long-lived live one: frontmatter,
         * a status/queue/backlog head, and an exchange of prompt and `### Re:` response cycles with
         * multi-byte text, ending in the operator region and a boundary marker.
         */
        fun syntheticSessionDocument(targetBytes: Int = 120 * 1024): String {
            val out = StringBuilder(targetBytes + 4096)
            out.append("---\nagent_doc_session: keystroke-bench\nagent: claude\nagent_doc_mode: template\n---\n\n")
            out.append("# Keystroke benchmark session\n\n")
            out.append("<!-- agent:status -->\nIdle — last cycle closed.\n<!-- /agent:status -->\n\n")
            out.append("<!-- agent:queue -->\n")
            for (i in 1..40) out.append("- do [#bench$i]\n")
            out.append("<!-- /agent:queue -->\n\n<!-- agent:backlog -->\n")
            for (i in 1..60) {
                out.append("- [ ] **[#bench$i]** Investigate the cycle $i receipt — keep the ❯ prefix and ")
                out.append("the héading attribution stable across reloads.\n")
            }
            out.append("<!-- /agent:backlog -->\n\n<!-- agent:exchange -->\n")
            var cycle = 0
            while (out.length < targetBytes) {
                cycle += 1
                out.append("❯ please fix the route notice for cycle $cycle and verify it\n\n")
                out.append("### Re: route notice cycle $cycle (HEAD)\n\n")
                repeat(6) { line ->
                    out.append("- Updated `TerminalUtil` step $line so the operator sees the outcome — ")
                    out.append("no silent success; ✓ verified in the sandbox.\n")
                }
                out.append("\nVerification: `make check` green.\n\n")
            }
            out.append("❯ next prompt goes here\n\n<!-- agent:boundary:bench0001 -->\n<!-- /agent:exchange -->\n")
            return out.toString()
        }
    }
}
