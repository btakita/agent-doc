package com.github.btakita.agentdoc

import com.intellij.openapi.diagnostic.Logger
import java.io.File
import java.time.Instant
import java.util.concurrent.atomic.AtomicLong

internal object RunAgentDocAttemptLedger {
    private val LOG = Logger.getInstance(RunAgentDocAttemptLedger::class.java)
    private const val ATTEMPT_DIAGNOSTICS_DIR = ".agent-doc/state/editor-route-attempts"
    private const val MAX_EVENT_LINES = 80
    private val sequence = AtomicLong(0)
    private val active = mutableMapOf<String, Attempt>()

    internal data class Attempt(
        val id: String,
        val cwd: String,
        val relativePath: String,
        val filePath: String,
        val focusedFile: String?,
        val startedAtMillis: Long,
        val previousAttemptId: String?,
    ) {
        val routeKey: String = RunAgentDocAttemptLedger.routeKey(cwd, relativePath)

        fun isCurrent(): Boolean = RunAgentDocAttemptLedger.isCurrent(this)

        fun record(stage: String, command: List<String>? = null, error: String? = null) {
            RunAgentDocAttemptLedger.record(this, stage, command, error)
        }

        fun recordIfCurrent(stage: String, command: List<String>? = null, error: String? = null) {
            if (isCurrent()) {
                record(stage, command, error)
            }
        }

        fun finishIfCurrent(stage: String, command: List<String>? = null, error: String? = null) {
            RunAgentDocAttemptLedger.finishIfCurrent(this, stage, command, error)
        }
    }

    fun begin(cwd: String, relativePath: String, filePath: String, focusedFile: String?): Attempt {
        val now = System.currentTimeMillis()
        val key = routeKey(cwd, relativePath)
        val attempt = Attempt(
            id = "${now}-${sequence.incrementAndGet()}",
            cwd = cwd,
            relativePath = relativePath,
            filePath = filePath,
            focusedFile = focusedFile,
            startedAtMillis = now,
            previousAttemptId = null,
        )
        val superseded = synchronized(active) {
            val previous = active.put(key, attempt)
            previous
        }
        if (superseded != null) {
            writeStage(
                attempt = superseded,
                stage = "superseded",
                command = null,
                error = "replaced_by=${attempt.id}",
                onlyIfCurrent = false,
            )
        }
        val latest = if (superseded == null) {
            attempt
        } else {
            attempt.copy(previousAttemptId = superseded.id)
        }
        synchronized(active) {
            active[key] = latest
        }
        latest.record("clicked")
        return latest
    }

    internal fun routeKey(cwd: String, relativePath: String): String = "$cwd::$relativePath"

    internal fun attemptDiagnosticsFile(cwd: String, relativePath: String): File {
        val diagnosticsDir = File(cwd, ATTEMPT_DIAGNOSTICS_DIR)
        val routeHash = Integer.toUnsignedString(routeKey(cwd, relativePath).hashCode(), 16)
        val sanitized = relativePath
            .replace('\\', '/')
            .replace(Regex("[^A-Za-z0-9._/-]+"), "_")
            .replace("/", "__")
            .ifBlank { "route-attempt" }
        return File(diagnosticsDir, "$sanitized-$routeHash.txt")
    }

    private fun isCurrent(attempt: Attempt): Boolean =
        synchronized(active) { active[attempt.routeKey]?.id == attempt.id }

    private fun finishIfCurrent(
        attempt: Attempt,
        stage: String,
        command: List<String>? = null,
        error: String? = null,
    ) {
        if (!isCurrent(attempt)) {
            return
        }
        writeStage(attempt, stage, command, error, onlyIfCurrent = true)
        synchronized(active) {
            if (active[attempt.routeKey]?.id == attempt.id) {
                active.remove(attempt.routeKey)
            }
        }
    }

    private fun record(
        attempt: Attempt,
        stage: String,
        command: List<String>? = null,
        error: String? = null,
    ) {
        writeStage(attempt, stage, command, error, onlyIfCurrent = false)
    }

    private fun writeStage(
        attempt: Attempt,
        stage: String,
        command: List<String>?,
        error: String?,
        onlyIfCurrent: Boolean,
    ) {
        if (onlyIfCurrent && !isCurrent(attempt)) {
            return
        }
        val updatedAtMillis = System.currentTimeMillis()
        val elapsedMillis = updatedAtMillis - attempt.startedAtMillis
        val diagnostics = attemptDiagnosticsFile(attempt.cwd, attempt.relativePath)
        try {
            diagnostics.parentFile?.mkdirs()
            val previousEvents = readEventLines(diagnostics)
            val event = eventLine(
                attempt = attempt,
                stage = stage,
                updatedAtMillis = updatedAtMillis,
                elapsedMillis = elapsedMillis,
                command = command,
                error = error,
            )
            val events = (previousEvents + event).takeLast(MAX_EVENT_LINES)
            diagnostics.writeText(
                buildSnapshot(
                    attempt = attempt,
                    stage = stage,
                    updatedAtMillis = updatedAtMillis,
                    elapsedMillis = elapsedMillis,
                    command = command,
                    error = error,
                    events = events,
                )
            )
        } catch (e: Exception) {
            LOG.warn("[run] failed to persist Run Agent Doc attempt stage=$stage file=${diagnostics.path}: ${e.message}", e)
        }
    }

    private fun buildSnapshot(
        attempt: Attempt,
        stage: String,
        updatedAtMillis: Long,
        elapsedMillis: Long,
        command: List<String>?,
        error: String?,
        events: List<String>,
    ): String = buildString {
        appendLine("attempt_id=${escape(attempt.id)}")
        appendLine("route_key=${escape(attempt.routeKey)}")
        appendLine("cwd=${escape(attempt.cwd)}")
        appendLine("relative_path=${escape(attempt.relativePath)}")
        appendLine("file_path=${escape(attempt.filePath)}")
        appendLine("focused_file=${escape(attempt.focusedFile.orEmpty())}")
        appendLine("stage=${escape(stage)}")
        appendLine("started_at=${Instant.ofEpochMilli(attempt.startedAtMillis)}")
        appendLine("updated_at=${Instant.ofEpochMilli(updatedAtMillis)}")
        appendLine("elapsed_ms=$elapsedMillis")
        appendLine("plugin_version=${escape(pluginVersion())}")
        appendLine("thread=${escape(Thread.currentThread().name)}")
        appendLine("command=${escape(command?.joinToString(" ").orEmpty())}")
        appendLine("error=${escape(error.orEmpty())}")
        appendLine("previous_attempt_id=${escape(attempt.previousAttemptId.orEmpty())}")
        appendLine("events:")
        for (event in events) {
            appendLine(event)
        }
    }

    private fun eventLine(
        attempt: Attempt,
        stage: String,
        updatedAtMillis: Long,
        elapsedMillis: Long,
        command: List<String>?,
        error: String?,
    ): String {
        val parts = mutableListOf(
            Instant.ofEpochMilli(updatedAtMillis).toString(),
            "attempt_id=${escape(attempt.id)}",
            "stage=${escape(stage)}",
            "elapsed_ms=$elapsedMillis",
            "thread=${escape(Thread.currentThread().name)}",
        )
        if (!command.isNullOrEmpty()) {
            parts.add("command=${escape(command.joinToString(" "))}")
        }
        if (!error.isNullOrBlank()) {
            parts.add("error=${escape(error)}")
        }
        return parts.joinToString(" ")
    }

    private fun readEventLines(diagnostics: File): List<String> {
        if (!diagnostics.isFile) {
            return emptyList()
        }
        return try {
            val lines = diagnostics.readLines()
            val marker = lines.indexOf("events:")
            if (marker < 0) {
                emptyList()
            } else {
                lines.drop(marker + 1).filter { it.isNotBlank() }
            }
        } catch (e: Exception) {
            LOG.warn("[run] failed to read Run Agent Doc attempt history ${diagnostics.path}: ${e.message}", e)
            emptyList()
        }
    }

    private fun pluginVersion(): String =
        RunAgentDocAttemptLedger::class.java.`package`?.implementationVersion ?: "unknown"

    private fun escape(value: String): String =
        value.replace("\r", "\\r").replace("\n", "\\n")
}
