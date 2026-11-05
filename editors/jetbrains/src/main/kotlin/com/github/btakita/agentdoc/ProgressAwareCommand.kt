package com.github.btakita.agentdoc

import java.io.File
import java.util.concurrent.CompletableFuture
import java.util.concurrent.TimeUnit

/**
 * `#starttimeoutload`: bounded, progress-aware wait for an agent-doc bootstrap command
 * (`tmux start`, `tmux ensure`).
 *
 * The command runs with `AGENT_DOC_PROGRESS=1` and reports each phase it enters as a
 * `[progress] phase=<name>` stderr line. The wait extends while phases keep arriving and
 * expires only when no phase arrived for [ProgressWaitBudget.idleTimeoutMs] or the whole
 * command outlived [ProgressWaitBudget.overallCapMs]. A timeout names the last phase seen,
 * instead of the bare synthetic `exit 124` a fixed wall-clock kill used to report.
 *
 * A binary that predates progress reporting prints no phase, so its wait is the idle
 * bound measured from launch: the old fixed bound.
 */
internal object ProgressAwareCommand {
    const val PROGRESS_ENV = "AGENT_DOC_PROGRESS"
    const val PROGRESS_LINE_PREFIX = "[progress] phase="
    const val TIMEOUT_EXIT_CODE = 124

    private const val STDERR_DRAIN_MS = 2_000L
    private const val STDOUT_DRAIN_MS = 1_000L
    private const val KILL_GRACE_MS = 500L

    /** The phase a `[progress] phase=<name>` line names, or null for any other line. */
    fun parseProgressPhase(line: String): String? =
        line.trim()
            .takeIf { it.startsWith(PROGRESS_LINE_PREFIX) }
            ?.removePrefix(PROGRESS_LINE_PREFIX)
            ?.trim()
            ?.takeIf { it.isNotEmpty() && it.none(Char::isWhitespace) }

    /** The last phase reported in [lines], or null when none was. */
    fun lastProgressPhase(lines: List<String>): String? =
        lines.asReversed().firstNotNullOfOrNull(::parseProgressPhase)

    /** [lines] without progress lines, joined: the command's own diagnostics. */
    fun diagnosticText(lines: List<String>): String =
        lines.filter { parseProgressPhase(it) == null }.joinToString("\n").trim()

    /**
     * Whether to keep waiting at [nowMs], and for how long at most before deciding again.
     * Pure, so the extension and expiry rules are tested without a clock.
     */
    fun decide(
        nowMs: Long,
        startedAtMs: Long,
        lastProgressAtMs: Long,
        budget: ProgressWaitBudget,
    ): ProgressWaitDecision {
        val idleDeadline = maxOf(startedAtMs, lastProgressAtMs) + budget.idleTimeoutMs
        val capDeadline = startedAtMs + budget.overallCapMs
        return when {
            nowMs >= capDeadline -> ProgressWaitDecision.Expire(ProgressExpiry.OVERALL_CAP)
            nowMs >= idleDeadline -> ProgressWaitDecision.Expire(ProgressExpiry.NO_PROGRESS)
            else -> ProgressWaitDecision.Wait(minOf(idleDeadline, capDeadline) - nowMs)
        }
    }

    /** The failure detail an editor shows for a timed-out bootstrap command. */
    fun timeoutMessage(
        label: String,
        lastPhase: String?,
        expiry: ProgressExpiry,
        elapsedMs: Long,
        budget: ProgressWaitBudget,
    ): String {
        val where = lastPhase?.let { "in phase '$it'" } ?: "before it reported any phase"
        val why = when (expiry) {
            ProgressExpiry.NO_PROGRESS ->
                "no progress for ${seconds(budget.idleTimeoutMs)}"
            ProgressExpiry.OVERALL_CAP ->
                "still running at the ${seconds(budget.overallCapMs)} cap"
        }
        return "$label timed out $where ($why; ${seconds(elapsedMs)} elapsed)"
    }

    /** The failure detail for a finished or timed-out command; null when it succeeded. */
    fun failureDetail(label: String, result: ProgressCommandResult, budget: ProgressWaitBudget): String? {
        if (result.expiry != null) {
            val message = timeoutMessage(label, result.lastPhase, result.expiry, result.elapsedMs, budget)
            return result.errorOutput.takeIf { it.isNotBlank() }?.let { "$message: $it" } ?: message
        }
        if (result.exitCode == 0) return null
        return result.errorOutput.ifBlank {
            result.output.ifBlank { "$label failed (exit ${result.exitCode})" }
        }
    }

    fun run(
        cmd: List<String>,
        cwd: String,
        budget: ProgressWaitBudget,
        clock: () -> Long = System::currentTimeMillis,
    ): ProgressCommandResult {
        val builder = ProcessBuilder(cmd)
            .directory(File(cwd))
            .redirectErrorStream(false)
        builder.environment()[PROGRESS_ENV] = "1"
        val startedAt = clock()
        val process = builder.start()

        val stderrLines = mutableListOf<String>()
        val lock = Any()
        var lastProgressAt = startedAt
        val stderrReader = CompletableFuture.runAsync {
            try {
                process.errorStream.bufferedReader().useLines { lines ->
                    for (line in lines) {
                        synchronized(lock) {
                            stderrLines.add(line)
                            if (parseProgressPhase(line) != null) lastProgressAt = clock()
                        }
                    }
                }
            } catch (_: Exception) {
                // The stream closes when the process is killed; what was read is kept.
            }
        }
        val stdout = CompletableFuture.supplyAsync {
            process.inputStream.bufferedReader().readText()
        }

        var expiry: ProgressExpiry? = null
        while (process.isAlive) {
            val progressAt = synchronized(lock) { lastProgressAt }
            when (val decision = decide(clock(), startedAt, progressAt, budget)) {
                is ProgressWaitDecision.Wait ->
                    process.waitFor(decision.forMs.coerceAtLeast(1), TimeUnit.MILLISECONDS)
                is ProgressWaitDecision.Expire -> {
                    expiry = decision.reason
                    process.destroy()
                    if (!process.waitFor(KILL_GRACE_MS, TimeUnit.MILLISECONDS)) {
                        process.destroyForcibly()
                        process.waitFor(KILL_GRACE_MS, TimeUnit.MILLISECONDS)
                    }
                    break
                }
            }
        }
        val elapsed = clock() - startedAt
        // Bounded: a grandchild that inherited stderr can hold the pipe open.
        try {
            stderrReader.get(STDERR_DRAIN_MS, TimeUnit.MILLISECONDS)
        } catch (_: Exception) {
            // Keep the lines read so far.
        }
        val lines = synchronized(lock) { stderrLines.toList() }
        val output = try {
            stdout.get(STDOUT_DRAIN_MS, TimeUnit.MILLISECONDS).trim()
        } catch (_: Exception) {
            ""
        }
        return ProgressCommandResult(
            exitCode = if (expiry != null) TIMEOUT_EXIT_CODE else process.exitValue(),
            output = output,
            errorOutput = diagnosticText(lines),
            lastPhase = lastProgressPhase(lines),
            expiry = expiry,
            elapsedMs = elapsed,
        )
    }

    private fun seconds(ms: Long): String =
        if (ms % 1_000L == 0L) "${ms / 1_000L}s" else String.format(java.util.Locale.ROOT, "%.1fs", ms / 1_000.0)
}

internal data class ProgressWaitBudget(
    /** Longest wait with no new phase reported. */
    val idleTimeoutMs: Long,
    /** Longest wait in total, however steadily phases arrive. */
    val overallCapMs: Long,
)

internal enum class ProgressExpiry { NO_PROGRESS, OVERALL_CAP }

internal sealed interface ProgressWaitDecision {
    data class Wait(val forMs: Long) : ProgressWaitDecision
    data class Expire(val reason: ProgressExpiry) : ProgressWaitDecision
}

internal data class ProgressCommandResult(
    val exitCode: Int,
    val output: String,
    /** Stderr without progress lines. */
    val errorOutput: String,
    val lastPhase: String?,
    /** Set when the wait expired and the command was killed. */
    val expiry: ProgressExpiry?,
    val elapsedMs: Long,
)
