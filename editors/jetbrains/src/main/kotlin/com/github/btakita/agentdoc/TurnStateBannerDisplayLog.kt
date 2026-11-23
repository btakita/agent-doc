package com.github.btakita.agentdoc

import java.util.concurrent.ConcurrentHashMap

/**
 * GH #436: one `idea.log` line each time a turn banner label is DRAWN for a document.
 *
 * `[turn-state] projection changed` is written once per change of the cached presentation, but
 * the banner is drawn from that cache on every collection edge: a document reopened, shown in
 * another editor or Remote Dev client, or still waiting for input the next day draws
 * `⚠ agent-doc: input required` from a change line that may sit in an earlier day's or a rotated
 * `idea.log`. This writes `[turn-state] banner shown for <file>: <label> source=<edge>
 * via=<frame reason> age_ms=<projection age>` the first time a label is drawn for a document, and
 * again only after it was hidden, replaced, or the document was released, so redraws stay quiet.
 *
 * Plain JDK state, so the dedup and the line are unit-testable without the IntelliJ platform.
 */
internal class TurnStateBannerDisplayLog {
    /** What last changed the cached presentation: the frame reason and when (epoch millis). */
    data class Change(val reason: String, val atMillis: Long)

    private val displayed = ConcurrentHashMap<String, String>()

    /**
     * Record that [label] is drawn for [filePath]. Returns the line to log when this label is newly
     * drawn for the document, or null when it is a redraw of the label already logged.
     */
    fun shown(filePath: String, label: String, change: Change?, source: String, nowMillis: Long): String? {
        if (displayed.put(filePath, label) == label) return null
        val age = change?.let { (nowMillis - it.atMillis).coerceAtLeast(0L).toString() } ?: "unknown"
        return "[turn-state] banner shown for $filePath: $label source=$source " +
            "via=${change?.reason ?: "unknown"} age_ms=$age"
    }

    /** The banner is not drawn for [filePath] now; its next display logs again. */
    fun hidden(filePath: String) {
        displayed.remove(filePath)
    }

    fun forget(filePath: String) {
        displayed.remove(filePath)
    }

    fun clear() {
        displayed.clear()
    }
}
