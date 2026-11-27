package com.github.btakita.agentdoc

import com.google.gson.JsonObject
import com.google.gson.JsonParser
import java.nio.file.InvalidPathException
import java.nio.file.Path

internal enum class RepoContextMode(val label: String) {
    CODE("Code"), DOCS("Docs");
    override fun toString(): String = label
}

internal data class RepoContextLocation(
    val path: Path,
    val line: Int,
    val endLine: Int,
    val name: String,
    val kind: String,
    val handle: String? = null,
) {
    override fun toString(): String = (if (kind == "source match") "Match · " else "") + "$name  —  $path:$line"
}

internal data class RepoContextAnswer(val offset: Int, val text: String)

/** Publish each set of links as it arrives so either lookup can fail independently. */
internal fun streamRepoContextLinks(
    outgoing: () -> List<RepoContextLocation>,
    incoming: () -> List<RepoContextLocation>,
    publish: (List<RepoContextLocation>, Throwable?, Boolean) -> Unit,
) {
    val outbound = runCatching(outgoing)
    publish(outbound.getOrNull().orEmpty(), null, false)
    if (Thread.currentThread().isInterrupted || outbound.exceptionOrNull() is InterruptedException) return
    val inbound = runCatching(incoming)
    publish(inbound.getOrNull().orEmpty(), outbound.exceptionOrNull() ?: inbound.exceptionOrNull(), true)
}

internal object RepoContextModel {
    private val requestIdPattern = Regex("[a-f0-9]{32}")
    private val matchLine = Regex("""^line ([1-9][0-9]*)\b""")
    private val whitespace = Regex("[\\s\\p{Z}]+")
    private val responseStart = Regex("^### Re:[ \\t]+.+")
    private val responseBoundary = Regex("^<!-- agent:boundary:[a-z0-9:-]+ -->$")
    private val answerReceipt = Regex("^<!--[ \\t]*repo-context-answer:([a-f0-9]{32})[ \\t]*-->$")

    fun pathInRepo(root: Path, file: String): Path? {
        if (file.isBlank()) return null
        val path = try {
            root.resolve(file).normalize()
        } catch (_: InvalidPathException) {
            return null
        }
        return path.takeIf { it != root && it.startsWith(root) }
    }

    private fun JsonObject.rows(key: String): List<JsonObject> =
        get(key)?.takeIf { it.isJsonArray }?.asJsonArray
            ?.mapNotNull { it.takeIf { row -> row.isJsonObject }?.asJsonObject }.orEmpty()

    private fun JsonObject.string(key: String): String? =
        get(key)?.takeIf { it.isJsonPrimitive && it.asJsonPrimitive.isString }
            ?.asString?.takeIf { it.isNotBlank() }

    private fun JsonObject.line(key: String): Int? =
        get(key)?.takeIf { it.isJsonPrimitive && it.asJsonPrimitive.isNumber }
            ?.asString?.toIntOrNull()?.takeIf { it >= 1 }

    private fun JsonObject.location(root: Path, file: Path?): RepoContextLocation? {
        val path = file ?: string("file")?.let { pathInRepo(root, it) } ?: return null
        val start = line("line") ?: return null
        val end = line("end_line") ?: start
        if (end < start) return null
        return RepoContextLocation(root.relativize(path), start, end,
            string("name") ?: return null, string("kind") ?: return null, string("handle"))
    }

    fun anchors(json: JsonObject, root: Path, mode: RepoContextMode, outlineFile: Path? = null): List<RepoContextLocation> {
        val rows = if (outlineFile != null) {
            json.get("projection")?.takeIf { it.isJsonObject }?.asJsonObject?.rows("outline").orEmpty()
        } else json.rows("symbols")
        return rows.filter { row ->
            when (mode) {
                RepoContextMode.DOCS -> row.string("kind") == "heading"
                RepoContextMode.CODE -> row.string("language") != "markdown" && row.string("kind") != "heading"
            }
        }.mapNotNull { it.location(root, outlineFile) }.distinctBy { Triple(it.path, it.line, it.name) }
    }

    fun codeMatches(json: JsonObject, root: Path, query: String): List<RepoContextLocation> =
        json.rows("hits").mapNotNull { row ->
            val path = row.string("path")?.takeUnless(::isMarkdown)?.let { pathInRepo(root, it) }
                ?: return@mapNotNull null
            val location = row.string("location") ?: return@mapNotNull null
            val line = matchLine.find(location)?.groupValues?.get(1)?.toIntOrNull()
                ?: return@mapNotNull null
            RepoContextLocation(root.relativize(path), line, line, query, "source match", row.string("artifact_id"))
        }.distinctBy { it.path to it.line }

    fun definitions(json: JsonObject, root: Path): List<RepoContextLocation> =
        json.rows("definitions").mapNotNull { it.location(root, null) }

    fun callSites(json: JsonObject, root: Path, anchor: RepoContextLocation): List<RepoContextLocation> {
        val result = mutableListOf<RepoContextLocation>()
        val definitions = definitions(json, root)
        val unique = definitions.singleOrNull()?.let { it.path == anchor.path && it.line == anchor.line } == true
        for ((key, label, nameKey) in listOf(
            Triple("callers", "Caller", "caller_name"), Triple("callees", "Callee", "callee_name"),
        )) {
            if (key == "callers" && !unique) continue
            for (row in json.rows(key)) {
                val file = row.string("caller_file")?.let { pathInRepo(root, it) } ?: continue
                val relative = root.relativize(file)
                if (key == "callees" && (relative != anchor.path || row.line("caller_line") != anchor.line)) continue
                val line = row.line("call_site_line") ?: continue
                val name = row.string(nameKey) ?: continue
                result += RepoContextLocation(relative, line, line, "$label · $name", label)
            }
        }
        return result.distinct()
    }

    fun markdownFiles(json: JsonObject, root: Path): List<Path> =
        json.rows("hits").mapNotNull { row ->
            row.string("path")?.takeIf(::isMarkdown)?.let { pathInRepo(root, it) }
        }.distinct()

    fun isMarkdown(path: String): Boolean =
        path.substringAfterLast('.', "").lowercase() in setOf("md", "mdx", "markdown")

    fun question(anchor: RepoContextLocation, revision: String, text: String, requestId: String): String {
        require(requestIdPattern.matches(requestId))
        fun inline(value: String) = whitespace.replace(value, " ").trim()
            .replace("<!--", "&lt;!--").replace("-->", "--&gt;")
        val range = if (anchor.endLine == anchor.line) "${anchor.line}" else "${anchor.line}-${anchor.endLine}"
        val handle = anchor.handle?.let { ", source handle ${inline(it)}" }.orEmpty()
        return "❯ Repo Context Explorer question about ${inline(anchor.path.toString())}:$range " +
            "(${inline(anchor.kind)} ${inline(anchor.name)}$handle, file SHA-256 $revision): ${inline(text)} " +
            "Please cite the source locations in your answer and include a standalone HTML comment " +
            "whose text is repo-context-answer:$requestId in the matching response."
    }

    fun insertion(document: String, exchange: AgentDocComponentOutline.ComponentSpan, prompt: String): String {
        val body = document.substring(exchange.openEnd, exchange.closeStart)
        // IntelliJ Documents use LF even when the file is saved as CRLF.
        val padding = when {
            body.endsWith("\n\n") -> ""
            body.endsWith("\n") -> "\n"
            else -> "\n\n"
        }
        return "$padding$prompt\n"
    }

    fun answer(document: String, exchange: AgentDocComponentOutline.ComponentSpan, requestId: String): RepoContextAnswer? {
        if (!requestIdPattern.matches(requestId)) return null
        val body = document.substring(exchange.openEnd, exchange.closeStart)
        val fences = findCodeBlockRangesUtil(body)
        var offset = 0
        var heading: Int? = null
        var receipt: String? = null
        val response = StringBuilder()
        var answer: RepoContextAnswer? = null
        for (line in body.lineSequence()) {
            val lineOffset = offset
            offset += line.length + 1
            val outsideFence = fences.none { (start, end) -> lineOffset in start until end }
            val start = outsideFence && responseStart.matches(line)
            val boundary = outsideFence && (start || responseBoundary.matches(line.trimEnd()) || line.startsWith("❯ "))
            if (boundary) {
                if (receipt == requestId) {
                    if (answer != null || response.isBlank()) return null
                    answer = RepoContextAnswer(exchange.openEnd + checkNotNull(heading), response.toString().trim())
                }
                heading = lineOffset.takeIf { start }
                receipt = null
                response.clear()
            } else if (heading != null) {
                val marker = if (outsideFence) answerReceipt.matchEntire(line.trimEnd()) else null
                if (marker?.groupValues?.get(1) == requestId) {
                    if (receipt != null) return null
                    receipt = requestId
                } else {
                    response.appendLine(line)
                }
            }
        }
        // The request is only closed by a following structural boundary.
        return answer.takeUnless { receipt == requestId }
    }

    fun decode(json: String): JsonObject = JsonParser.parseString(json).asJsonObject
}
