package com.github.btakita.agentdoc

import java.lang.ref.WeakReference
import java.util.concurrent.atomic.AtomicLong

/**
 * Validate and translate the editor's causal splice stream against the retained
 * CRDT shadow. Ordinary editor events already carry the changed UTF-16 range;
 * never widen them into a whole-buffer diff.
 *
 * `#kotlinflushprep`: the work is proportional to the burst, not the document.
 * The burst is applied to an [EditWindowText] (the untouched shadow prefix, one
 * edited window, the untouched shadow suffix), so validating a splice and
 * converting its UTF-16 offset to code points read only the window and the
 * shadow's cached [surrogatePairStartsUtil] index. The one whole-document cost
 * left is the bulk copy that builds [PreparedLocalEditorBatch.resultingText],
 * the next shadow. The prepared edits are exactly what the earlier
 * `StringBuilder` + `Character.codePointCount` walk produced, including the
 * anchor arithmetic around unpaired surrogates (`LocalEditorSplicePreparationTest`
 * runs both side by side).
 */
internal fun prepareLocalEditorEditsUtil(
    before: String,
    edits: List<CapturedLocalEditorEdit>,
): PreparedLocalEditorBatch? {
    val basePairs = surrogatePairStartsUtil(before)
    val current = EditWindowText(before, basePairs)
    var anchorUtf16 = before.length
    // `String.codePointCount` counts each well-formed pair once and every other
    // char once: the length minus the number of pairs.
    var anchorCodePoints = before.length - basePairs.size
    val prepared = ArrayList<PreparedLocalEditorEdit>(edits.size)
    for (edit in edits) {
        val start = edit.offsetUtf16
        val end = start + edit.oldFragment.length
        val length = current.length
        // `#splicebaselength`: the splice was typed into a document of a
        // different length, so its offset does not address this text.
        if (edit.beforeLengthUtf16 >= 0 && edit.beforeLengthUtf16 != length) return null
        if (start < 0 || start > length || end > length) return null
        current.cover(start, end)
        if (!current.windowRegionEquals(start, edit.oldFragment)) return null
        val deleteCodePoints = edit.oldFragment.codePointCount(0, edit.oldFragment.length)
        val insertCodePoints = edit.newFragment.codePointCount(0, edit.newFragment.length)
        val offsetCodePoints =
            if (start >= anchorUtf16) {
                anchorCodePoints + current.codePointCount(anchorUtf16, start)
            } else {
                anchorCodePoints - current.codePointCount(start, anchorUtf16)
            }
        prepared.add(
            PreparedLocalEditorEdit(
                offsetCodePoints = offsetCodePoints,
                deleteCodePoints = deleteCodePoints,
                insert = edit.newFragment,
            ),
        )
        current.replaceCovered(start, end, edit.newFragment)
        anchorUtf16 = start + edit.newFragment.length
        anchorCodePoints = offsetCodePoints + insertCodePoints
    }
    val resultingText = current.materialize()
    if (resultingText !== before) {
        SurrogatePairIndexCache.remember(resultingText, current.resultingPairStarts(resultingText))
    }
    return PreparedLocalEditorBatch(prepared, resultingText)
}

/**
 * The UTF-16 index of every well-formed surrogate pair's high half in [text]
 * (a high surrogate immediately followed by a low one), ascending.
 *
 * `#kotlinflushprep`: answered from an identity cache. A flush stores the index
 * of the text it produces, and that text becomes the next flush's shadow, so a
 * typing session scans a shadow only when it arrives from somewhere else (a
 * remote apply, a registration).
 */
internal fun surrogatePairStartsUtil(text: String): IntArray =
    SurrogatePairIndexCache.lookup(text) ?: scanSurrogatePairStartsUtil(text).also {
        SurrogatePairIndexCache.remember(text, it)
    }

/** How many times a whole text was scanned for surrogate pairs (cache misses). */
internal val surrogatePairFullScans = AtomicLong()

internal fun scanSurrogatePairStartsUtil(text: String): IntArray {
    surrogatePairFullScans.incrementAndGet()
    var starts = NO_PAIRS
    var count = 0
    var i = 0
    val last = text.length - 1
    while (i < last) {
        if (Character.isHighSurrogate(text[i]) && Character.isLowSurrogate(text[i + 1])) {
            if (count == starts.size) starts = starts.copyOf(maxOf(8, count * 2))
            starts[count++] = i
            i += 2
        } else {
            i++
        }
    }
    return if (count == starts.size) starts else starts.copyOf(count)
}

private val NO_PAIRS = IntArray(0)

/**
 * A small identity-keyed cache: a shadow is the same String instance from the
 * flush that produced it to the flush that consumes it, so equality (and its
 * O(document) hash) is never needed. Weak keys keep the cache from retaining a
 * shadow its document has already replaced.
 */
private object SurrogatePairIndexCache {
    private const val SLOTS = 16
    private val texts = arrayOfNulls<WeakReference<String>>(SLOTS)
    private val starts = arrayOfNulls<IntArray>(SLOTS)
    private var next = 0

    @Synchronized
    fun lookup(text: String): IntArray? {
        for (slot in 0 until SLOTS) {
            if (texts[slot]?.get() === text) return starts[slot]
        }
        return null
    }

    @Synchronized
    fun remember(text: String, pairStarts: IntArray) {
        var target = -1
        for (slot in 0 until SLOTS) {
            val held = texts[slot]?.get()
            if (held === text) {
                starts[slot] = pairStarts
                return
            }
            if (held == null && target < 0) target = slot
        }
        if (target < 0) {
            target = next
            next = (next + 1) % SLOTS
        }
        texts[target] = WeakReference(text)
        starts[target] = pairStarts
    }
}

/**
 * The text a burst is being applied to, kept as `base[0, windowStart)` +
 * [window] + `base[suffixStart, base.length)`. Only [window] is ever copied or
 * scanned while the burst is applied; positions outside it are read from [base]
 * and its pair index. While the window is empty and `windowStart ==
 * suffixStart`, the text is exactly [base] and the window may move freely.
 */
internal class EditWindowText(
    private val base: String,
    private val basePairs: IntArray,
) {
    private val window = StringBuilder()
    private var windowStart = base.length
    private var suffixStart = base.length

    val length: Int
        get() = windowStart + window.length + (base.length - suffixStart)

    private val windowEnd: Int
        get() = windowStart + window.length

    /** Where a suffix position of this text sits in [base]. */
    private val suffixShift: Int
        get() = windowEnd - suffixStart

    /** Grow the window so it holds `[start, end)` of this text. */
    fun cover(start: Int, end: Int) {
        if (window.isEmpty() && windowStart == suffixStart) {
            windowStart = start
            suffixStart = start
        }
        if (start < windowStart) {
            window.insert(0, base, start, windowStart)
            windowStart = start
        }
        val coveredEnd = windowEnd
        if (end > coveredEnd) {
            val take = end - coveredEnd
            window.append(base, suffixStart, suffixStart + take)
            suffixStart += take
        }
    }

    /** True when `[start, start + fragment.length)` (already [cover]ed) spells [fragment]. */
    fun windowRegionEquals(start: Int, fragment: String): Boolean {
        val offset = start - windowStart
        for (i in fragment.indices) {
            if (window[offset + i] != fragment[i]) return false
        }
        return true
    }

    fun replaceCovered(start: Int, end: Int, replacement: String) {
        window.replace(start - windowStart, end - windowStart, replacement)
    }

    private fun charAt(index: Int): Char =
        when {
            index < windowStart -> base[index]
            index < windowEnd -> window[index - windowStart]
            else -> base[index - suffixShift]
        }

    /** `Character.codePointCount` over `[from, to)` of this text. */
    fun codePointCount(from: Int, to: Int): Int = (to - from) - pairCount(from, to)

    /**
     * Well-formed pairs lying entirely inside `[from, to)`, for any range of this text.
     *
     * [prepareLocalEditorEditsUtil] covers a splice before counting, and its anchor always
     * lies inside the window after the first splice, so today every counted range starts at or
     * after [windowStart]: only the window scan and (for the first splice, whose anchor is the
     * end of the shadow) the suffix index are reached. The prefix branch keeps the count exact
     * for any caller order rather than relying on that.
     */
    private fun pairCount(from: Int, to: Int): Int {
        val lastStart = to - 2
        if (lastStart < from) return 0
        var count = 0
        val prefixLast = minOf(lastStart, windowStart - 2)
        if (prefixLast >= from) count += indexedPairs(from, prefixLast)
        // Pairs that start on the char before the window, inside it, or on its
        // last char straddle a seam and are read from the text itself.
        val scanLast = minOf(lastStart, windowEnd - 1)
        var i = maxOf(from, windowStart - 1, 0)
        while (i <= scanLast) {
            if (Character.isHighSurrogate(charAt(i)) && Character.isLowSurrogate(charAt(i + 1))) count++
            i++
        }
        val suffixFirst = maxOf(from, windowEnd)
        if (lastStart >= suffixFirst) count += indexedPairs(suffixFirst - suffixShift, lastStart - suffixShift)
        return count
    }

    /** [basePairs] entries in `[first, last]` of [base]. */
    private fun indexedPairs(first: Int, last: Int): Int = lowerBound(last + 1) - lowerBound(first)

    private fun lowerBound(value: Int): Int {
        var low = 0
        var high = basePairs.size
        while (low < high) {
            val mid = (low + high) ushr 1
            if (basePairs[mid] < value) low = mid + 1 else high = mid
        }
        return low
    }

    /** The finished text: [base] itself when the burst left it unchanged. */
    fun materialize(): String {
        if (window.isEmpty() && windowStart == suffixStart) return base
        val out = StringBuilder(length)
        out.append(base, 0, windowStart)
        out.append(window)
        out.append(base, suffixStart, base.length)
        return out.toString()
    }

    /** The pair index of [resulting] (this text, materialized), derived from [basePairs]. */
    fun resultingPairStarts(resulting: String): IntArray {
        val prefixCount = lowerBound(windowStart - 1)
        val suffixFrom = lowerBound(suffixStart)
        val shift = suffixShift
        var seam = NO_PAIRS
        var seamCount = 0
        var i = maxOf(windowStart - 1, 0)
        val scanLast = minOf(windowEnd - 1, resulting.length - 2)
        while (i <= scanLast) {
            if (Character.isHighSurrogate(resulting[i]) && Character.isLowSurrogate(resulting[i + 1])) {
                if (seamCount == seam.size) seam = seam.copyOf(maxOf(4, seamCount * 2))
                seam[seamCount++] = i
            }
            i++
        }
        val suffixCount = basePairs.size - suffixFrom
        val out = IntArray(prefixCount + seamCount + suffixCount)
        System.arraycopy(basePairs, 0, out, 0, prefixCount)
        System.arraycopy(seam, 0, out, prefixCount, seamCount)
        for (k in 0 until suffixCount) out[prefixCount + seamCount + k] = basePairs[suffixFrom + k] + shift
        return out
    }
}
