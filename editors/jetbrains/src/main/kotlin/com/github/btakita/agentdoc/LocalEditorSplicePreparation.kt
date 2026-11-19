package com.github.btakita.agentdoc

import java.util.concurrent.atomic.AtomicLong

internal data class PreparedReplicaSpliceBatch(
    val edits: List<PreparedLocalEditorEdit>,
    val resultingShadow: ReplicaShadow,
) {
    fun toTextBatch() = PreparedLocalEditorBatch(edits, resultingShadow.toString())
}

internal fun PreparedLocalEditorBatch.toReplicaBatch() =
    PreparedReplicaSpliceBatch(edits, ReplicaShadow(resultingText))

/** String boundary for registration and native recovery, whose wire batches include full text. */
internal fun prepareLocalEditorEditsUtil(
    before: String,
    edits: List<CapturedLocalEditorEdit>,
): PreparedLocalEditorBatch? =
    prepareLocalEditorEditsUtil(ReplicaShadow(before), edits)?.toTextBatch()

/**
 * Validate the captured UTF-16 ranges and preserve the CRDT's code-point
 * offsets, including the previous-edit anchor around unpaired surrogates.
 */
internal fun prepareLocalEditorEditsUtil(
    before: ReplicaShadow,
    edits: List<CapturedLocalEditorEdit>,
): PreparedReplicaSpliceBatch? {
    var current = before
    var anchorUtf16 = before.length
    var anchorCodePoints = before.codePointCount(0, anchorUtf16)
    val prepared = ArrayList<PreparedLocalEditorEdit>(edits.size)
    for (edit in edits) {
        val start = edit.offsetUtf16
        val length = current.length
        // `#splicebaselength`: the splice must address the captured document length.
        if (edit.beforeLengthUtf16 >= 0 && edit.beforeLengthUtf16 != length) return null
        if (start !in 0..length || edit.oldFragment.length > length - start) return null
        if (!current.regionMatches(start, edit.oldFragment)) return null
        val offsetCodePoints =
            if (start >= anchorUtf16) {
                anchorCodePoints + current.codePointCount(anchorUtf16, start)
            } else {
                anchorCodePoints - current.codePointCount(start, anchorUtf16)
            }
        prepared.add(
            PreparedLocalEditorEdit(
                offsetCodePoints = offsetCodePoints,
                deleteCodePoints = edit.oldFragment.codePointCount(0, edit.oldFragment.length),
                insert = edit.newFragment,
            ),
        )
        current = current.replace(start, start + edit.oldFragment.length, edit.newFragment)
        anchorUtf16 = start + edit.newFragment.length
        anchorCodePoints = offsetCodePoints + edit.newFragment.codePointCount(0, edit.newFragment.length)
    }
    return PreparedReplicaSpliceBatch(prepared, current)
}

/** Number of UTF-16 code units inspected to build piece indexes. */
internal val surrogatePairScannedUtf16 = AtomicLong()

internal fun scanSurrogatePairStartsUtil(text: String): IntArray {
    surrogatePairScannedUtf16.addAndGet(text.length.toLong())
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
