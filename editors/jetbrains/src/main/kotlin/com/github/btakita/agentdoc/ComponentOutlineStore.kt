package com.github.btakita.agentdoc

import com.github.btakita.agentdoc.AgentDocComponentOutline.ComponentSpan
import com.intellij.openapi.editor.Document
import com.intellij.openapi.editor.RangeMarker
import com.intellij.openapi.project.Project
import com.intellij.util.containers.CollectionFactory

/**
 * Per-project cache of native component boundaries for open session documents (GH #19).
 *
 * The native parse runs on [VisualHighlighterManager]'s debounced background refresh, never in a
 * folding/line-marker/structure callback: those run inside read actions (some on the EDT), and a
 * native call queued behind CRDT work would block typing (`editors/PLUGIN-SPEC.md` §9.11). Each
 * span is kept as two [RangeMarker]s, so between refreshes the boundaries track edits exactly and
 * every callback reads current offsets from memory.
 *
 * Owned by the generation's [VisualHighlighterManager]; documents are weak keys, and [clear]
 * releases every marker on project disposal/plugin unload.
 */
class ComponentOutlineStore {
    private class Entry(
        val name: String,
        val attrs: Map<String, String>,
        val open: RangeMarker,
        val close: RangeMarker,
    )

    /** Spans resolved at one document stamp: the line-marker pass asks once per leaf. */
    private class Resolved(val stamp: Long, val spans: List<ComponentSpan>)

    private class Outline(val entries: List<Entry>) {
        @Volatile var resolved: Resolved? = null
    }

    private val entries = CollectionFactory.createConcurrentWeakMap<Document, Outline>()

    /**
     * Current component spans of [document], or empty when it is not a session document or no
     * native outline has been installed yet. Caller holds read access. Markers invalidated by an
     * edit that removed a marker are dropped until the next refresh replaces them.
     */
    fun spans(document: Document): List<ComponentSpan> {
        val outline = entries[document] ?: return emptyList()
        val stamp = document.modificationStamp
        outline.resolved?.let { if (it.stamp == stamp) return it.spans }
        val chars = document.charsSequence
        val spans = outline.entries.mapNotNull { e ->
            if (!e.open.isValid || !e.close.isValid) return@mapNotNull null
            val span = ComponentSpan(
                e.name,
                e.attrs,
                e.open.startOffset,
                e.open.endOffset,
                e.close.startOffset,
                e.close.endOffset,
            )
            span.takeIf {
                it.openStart < it.openEnd && it.openEnd <= it.closeStart && it.closeStart < it.closeEnd &&
                    it.closeEnd <= chars.length && chars.startsWith("<!--", it.openStart)
            }
        }
        outline.resolved = Resolved(stamp, spans)
        return spans
    }

    /**
     * Install a fresh native outline for [document] (EDT, document at the parsed stamp). Returns
     * true when the visible structure changed, i.e. folding/markers need a daemon restart; an
     * outline that matches what the markers already track is a no-op.
     */
    fun install(document: Document, spans: List<ComponentSpan>): Boolean {
        if (spans == spans(document) && (spans.isNotEmpty() || entries.containsKey(document))) {
            return false
        }
        val previous = entries[document]?.entries
        val replacement = spans.map { span ->
            Entry(
                span.name,
                span.attrs,
                document.createRangeMarker(span.openStart, span.openEnd),
                document.createRangeMarker(span.closeStart, span.closeEnd),
            )
        }
        if (replacement.isEmpty()) entries.remove(document) else entries[document] = Outline(replacement)
        previous?.forEach { dispose(it) }
        return previous != null || replacement.isNotEmpty()
    }

    /** Forget [document]: it stopped being a session document. Returns true when it had spans. */
    fun remove(document: Document): Boolean {
        val previous = entries.remove(document)?.entries ?: return false
        previous.forEach { dispose(it) }
        return true
    }

    fun clear() {
        val all = entries.values.toList()
        entries.clear()
        all.forEach { outline -> outline.entries.forEach { dispose(it) } }
    }

    private fun dispose(entry: Entry) {
        entry.open.dispose()
        entry.close.dispose()
    }

    companion object {
        /** Spans for [document] in [project]'s live generation; empty when none is attached. */
        fun spansFor(project: Project, document: Document): List<ComponentSpan> =
            VisualHighlighterManager.peek(project)?.componentOutlines?.spans(document).orEmpty()
    }
}
