package com.github.btakita.agentdoc

/**
 * Pure, IDE-free model behind the component folding, gutter markers, and structure view
 * (GH #19, plugin UX phases 5-7).
 *
 * Component boundaries are never parsed here: they come from the shared native parser
 * (`agent_doc_parse_components`, see [NativePatching.componentSpansOrNull]) so the editor and
 * the binary agree on what a component is. This object only converts the native UTF-8 byte
 * offsets to editor UTF-16 offsets and derives presentation (items, placeholders, tooltips,
 * tree nodes) from a component body.
 */
object AgentDocComponentOutline {

    /** One `<!-- agent:NAME ... -->` … `<!-- /agent:NAME -->` component, in UTF-16 offsets. */
    data class ComponentSpan(
        val name: String,
        val attrs: Map<String, String>,
        val openStart: Int,
        val openEnd: Int,
        val closeStart: Int,
        val closeEnd: Int,
    ) {
        fun contains(other: ComponentSpan): Boolean =
            other !== this && openStart <= other.openStart && other.closeEnd <= closeEnd &&
                !(openStart == other.openStart && closeEnd == other.closeEnd)
    }

    enum class ItemKind { HEADING, LIST_ITEM }

    /** A structure entry inside a component body: an ATX heading or a top-level list item. */
    data class ComponentItem(val kind: ItemKind, val label: String, val offset: Int)

    /** A fold region request: [start, end) collapses to [placeholder]. */
    data class FoldSpec(val name: String, val start: Int, val end: Int, val placeholder: String)

    /** A structure-view node for one component; [children] holds nested components. */
    data class ComponentNode(
        val span: ComponentSpan,
        val items: List<ComponentItem>,
        val children: List<ComponentNode>,
    ) {
        val presentableText: String get() = "agent:${span.name}"
        val locationString: String get() = itemCountLabel(items.size)
    }

    private const val MAX_LABEL = 80

    // ---- native JSON -> UTF-16 spans ------------------------------------------------------

    /**
     * Decode the `agent_doc_parse_components` JSON (`name`, `attrs`, `open_start`, `open_end`,
     * `close_start`, `close_end` as UTF-8 byte offsets) into spans with UTF-16 offsets for [doc].
     */
    fun spansFromNativeJson(json: String, doc: CharSequence): List<ComponentSpan> {
        val root = com.google.gson.JsonParser.parseString(json).asJsonArray
        data class Raw(val name: String, val attrs: Map<String, String>, val o: IntArray)
        val raws = root.mapNotNull { element ->
            val obj = element.asJsonObject
            val name = obj.get("name")?.asString ?: return@mapNotNull null
            val attrs = obj.getAsJsonObject("attrs")
                ?.entrySet()
                ?.associate { (k, v) -> k to (if (v.isJsonPrimitive) v.asString else v.toString()) }
                .orEmpty()
            val offsets = intArrayOf(
                obj.get("open_start")?.asInt ?: return@mapNotNull null,
                obj.get("open_end")?.asInt ?: return@mapNotNull null,
                obj.get("close_start")?.asInt ?: return@mapNotNull null,
                obj.get("close_end")?.asInt ?: return@mapNotNull null,
            )
            Raw(name, attrs.toSortedMap(), offsets)
        }
        val mapped = utf8ToUtf16Offsets(doc, raws.flatMap { it.o.toList() })
        return raws.map { raw ->
            ComponentSpan(
                name = raw.name,
                attrs = raw.attrs,
                openStart = mapped.getValue(raw.o[0]),
                openEnd = mapped.getValue(raw.o[1]),
                closeStart = mapped.getValue(raw.o[2]),
                closeEnd = mapped.getValue(raw.o[3]),
            )
        }.sortedBy { it.openStart }
    }

    /**
     * Map UTF-8 byte offsets in [doc] to UTF-16 code-unit offsets in one pass. Mirrors the Rust
     * `utf8_offsets_to_utf16_offsets` used by `agent_doc_visual_tokens_json`; offsets past the
     * end clamp to the document length.
     */
    fun utf8ToUtf16Offsets(doc: CharSequence, byteOffsets: Collection<Int>): Map<Int, Int> {
        val targets = byteOffsets.toSortedSet().toIntArray()
        val mapped = HashMap<Int, Int>(targets.size * 2)
        var t = 0
        var utf8 = 0
        var i = 0
        while (t < targets.size && i < doc.length) {
            while (t < targets.size && targets[t] <= utf8) {
                mapped[targets[t]] = i
                t++
            }
            val c = doc[i]
            if (Character.isHighSurrogate(c) && i + 1 < doc.length && Character.isLowSurrogate(doc[i + 1])) {
                utf8 += 4
                i += 2
            } else {
                utf8 += when {
                    c.code < 0x80 -> 1
                    c.code < 0x800 -> 2
                    else -> 3
                }
                i += 1
            }
        }
        while (t < targets.size) {
            mapped[targets[t]] = i
            t++
        }
        return mapped
    }

    // ---- items ----------------------------------------------------------------------------

    private val HEADING = Regex("""^(#{1,6})(?:[ \t]+(.*?))?[ \t#]*$""")
    private val LIST_ITEM = Regex("""^(?:[-*+]|\d{1,9}[.)])[ \t]+(.*)$""")
    private val CHECKBOX = Regex("""^\[[ xX]\][ \t]+""")

    /**
     * Structure entries of [span]'s body in [doc]. Bodies with headings list their headings
     * (exchange/done-style sections); otherwise the top-level (unindented) list items
     * (backlog/queue-style lists). Lines in fenced code and in nested components are skipped.
     */
    fun items(doc: CharSequence, span: ComponentSpan, all: List<ComponentSpan>): List<ComponentItem> {
        val bodyStart = span.openEnd.coerceIn(0, doc.length)
        val bodyEnd = span.closeStart.coerceIn(bodyStart, doc.length)
        val nested = all.filter { span.contains(it) }
        val headings = ArrayList<ComponentItem>()
        val listItems = ArrayList<ComponentItem>()
        var fence: String? = null
        var lineStart = bodyStart
        while (lineStart < bodyEnd) {
            var lineEnd = lineStart
            while (lineEnd < bodyEnd && doc[lineEnd] != '\n') lineEnd++
            val inNested = nested.any { lineStart >= it.openStart && lineStart < it.closeEnd }
            if (!inNested) {
                val line = doc.subSequence(lineStart, lineEnd).toString().trimEnd('\r')
                val trimmed = line.trimStart()
                val indent = line.length - trimmed.length
                val fenceToken = if (indent <= 3) fenceToken(trimmed) else null
                when {
                    fence != null -> if (fenceToken != null && fenceToken.startsWith(fence) &&
                        trimmed.trim().all { it == fence!![0] }
                    ) {
                        fence = null
                    }
                    fenceToken != null -> fence = fenceToken
                    indent == 0 -> {
                        HEADING.matchEntire(line)?.let { m ->
                            headings += ComponentItem(ItemKind.HEADING, label(m.groupValues[2].ifBlank { m.groupValues[1] }), lineStart)
                        } ?: LIST_ITEM.matchEntire(line)?.let { m ->
                            listItems += ComponentItem(
                                ItemKind.LIST_ITEM,
                                label(m.groupValues[1].replaceFirst(CHECKBOX, "")),
                                lineStart,
                            )
                        }
                    }
                }
            }
            lineStart = lineEnd + 1
        }
        return if (headings.isNotEmpty()) headings else listItems
    }

    private fun fenceToken(trimmed: String): String? {
        val ch = trimmed.firstOrNull() ?: return null
        if (ch != '`' && ch != '~') return null
        val run = trimmed.takeWhile { it == ch }
        return if (run.length >= 3) run else null
    }

    private fun label(raw: String): String {
        val text = raw.trim().ifEmpty { "(empty)" }
        return if (text.length <= MAX_LABEL) text else text.take(MAX_LABEL - 1) + "…"
    }

    fun itemCountLabel(count: Int): String =
        when (count) {
            0 -> "empty"
            1 -> "1 item"
            else -> "$count items"
        }

    // ---- folding --------------------------------------------------------------------------

    /**
     * Fold the whole component (open marker through close marker, excluding the close marker's
     * trailing newline) to `agent:NAME · N items`. Single-line components are not folded.
     */
    fun foldSpecs(doc: CharSequence, spans: List<ComponentSpan>): List<FoldSpec> =
        spans.mapNotNull { span ->
            val start = span.openStart
            var end = span.closeEnd.coerceAtMost(doc.length)
            while (end > start && (doc[end - 1] == '\n' || doc[end - 1] == '\r')) end--
            if (end <= start) return@mapNotNull null
            if ((start until end).none { doc[it] == '\n' }) return@mapNotNull null
            FoldSpec(span.name, start, end, placeholder(span, items(doc, span, spans).size))
        }

    fun placeholder(span: ComponentSpan, itemCount: Int): String =
        "<!-- agent:${span.name} · ${itemCountLabel(itemCount)} -->"

    // ---- gutter markers -------------------------------------------------------------------

    /**
     * The component whose open marker starts inside the leaf range [leafStart, leafEnd), or
     * null. Leaves partition the document, so each open marker is claimed by exactly one leaf.
     */
    fun componentOpeningIn(spans: List<ComponentSpan>, leafStart: Int, leafEnd: Int): ComponentSpan? {
        if (leafEnd <= leafStart) return null
        var lo = 0
        var hi = spans.size
        while (lo < hi) {
            val mid = (lo + hi) ushr 1
            if (spans[mid].openStart < leafStart) lo = mid + 1 else hi = mid
        }
        val candidate = spans.getOrNull(lo) ?: return null
        return candidate.takeIf { it.openStart < leafEnd }
    }

    /** The innermost component whose full range contains [offset]. */
    fun componentAt(spans: List<ComponentSpan>, offset: Int): ComponentSpan? =
        spans.filter { offset >= it.openStart && offset < it.closeEnd }
            .minByOrNull { it.closeEnd - it.openStart }

    fun tooltip(doc: CharSequence, span: ComponentSpan, all: List<ComponentSpan>): String {
        val sb = StringBuilder("<html><b>agent:").append(escape(span.name)).append("</b> · ")
            .append(itemCountLabel(items(doc, span, all).size))
        if (span.attrs.isNotEmpty()) {
            sb.append("<br>").append(span.attrs.entries.joinToString(" ") { (k, v) -> escape("$k=$v") })
        }
        return sb.append("<br>Click to fold or unfold</html>").toString()
    }

    private fun escape(s: String): String =
        s.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;").replace("\"", "&quot;")

    // ---- structure view -------------------------------------------------------------------

    /** Component tree for the structure view: top-level components, nested ones as children. */
    fun structure(doc: CharSequence, spans: List<ComponentSpan>): List<ComponentNode> {
        val sorted = spans.sortedWith(compareBy({ it.openStart }, { -(it.closeEnd - it.openStart) }))
        fun parentOf(span: ComponentSpan): ComponentSpan? =
            sorted.filter { it.contains(span) }.minByOrNull { it.closeEnd - it.openStart }
        fun build(parent: ComponentSpan?): List<ComponentNode> =
            sorted.filter { parentOf(it) == parent }.map { span ->
                ComponentNode(span, items(doc, span, sorted), build(span))
            }
        return build(null)
    }
}
