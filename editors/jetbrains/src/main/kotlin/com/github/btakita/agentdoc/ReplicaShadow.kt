package com.github.btakita.agentdoc

import java.io.OutputStream
import java.io.OutputStreamWriter
import java.nio.charset.StandardCharsets.UTF_8
import java.security.DigestOutputStream
import java.security.MessageDigest
import java.util.HexFormat

/**
 * An immutable AVL rope whose pieces share their backing strings (and surrogate indexes).
 * A captured edit copies only the changed pieces and the branches leading to them.
 */
internal class ReplicaShadow private constructor(private val root: Node?) {
    constructor(text: String) : this(piece(text))

    val length: Int get() = root?.length ?: 0

    fun replace(start: Int, end: Int, inserted: String): ReplicaShadow {
        if (start == end && inserted.isEmpty()) return this
        val (prefix, tail) = split(root, start)
        val (_, suffix) = split(tail, end - start)
        return ReplicaShadow(join(join(prefix, piece(inserted)), suffix))
    }

    fun regionMatches(start: Int, expected: String): Boolean {
        var offset = 0
        return visit(root, start, start + expected.length) { piece, from, to ->
            val matches = piece.source.text.regionMatches(from, expected, offset, to - from)
            offset += to - from
            matches
        }
    }

    fun matches(text: String): Boolean =
        intactText() === text || (length == text.length && regionMatches(0, text))

    fun matches(other: ReplicaShadow): Boolean {
        if (root === other.root) return true
        if (length != other.length) return false
        val text = intactText()
        return if (text == null) matches(other.toString()) else other.matches(text)
    }

    fun codePointCount(start: Int, end: Int): Int =
        end - start - countPairs(root, start, end)

    private fun intactText(): String? =
        if (root is Piece && root.start == 0 && root.length == root.source.text.length) root.source.text else null

    override fun toString(): String = intactText() ?: buildString(length) {
        visit(root, 0, this@ReplicaShadow.length) { piece, from, to ->
            append(piece.source.text, from, to)
            true
        }
    }

    /** Preserve the wire hash of one UTF-8 string, even when a pair crosses pieces. */
    val contentHash: String by lazy(LazyThreadSafetyMode.PUBLICATION) { computeContentHash() }

    private fun computeContentHash(): String {
        val digest = MessageDigest.getInstance("SHA-256")
        val chars = CharArray(4096)
        OutputStreamWriter(DigestOutputStream(OutputStream.nullOutputStream(), digest), UTF_8).use { writer ->
            visit(root, 0, length) { piece, from, to ->
                var offset = from
                while (offset < to) {
                    val end = minOf(to, offset + chars.size)
                    piece.source.text.toCharArray(chars, 0, offset, end)
                    writer.write(chars, 0, end - offset)
                    offset = end
                }
                true
            }
        }
        return HexFormat.of().formatHex(digest.digest())
    }

    private class Source(val text: String) {
        val pairs by lazy(LazyThreadSafetyMode.PUBLICATION) { scanSurrogatePairStartsUtil(text) }
    }

    private sealed class Node {
        abstract val length: Int
        abstract val height: Int
        abstract val first: Char
        abstract val last: Char
        abstract val pairs: Int
    }

    private class Piece(val source: Source, val start: Int, val end: Int) : Node() {
        override val length = end - start
        override val height = 1
        override val first: Char get() = source.text[start]
        override val last: Char get() = source.text[end - 1]
        override val pairs by lazy(LazyThreadSafetyMode.PUBLICATION) { pairsIn(0, length) }

        fun pairsIn(from: Int, to: Int): Int {
            if (to - from < 2) return 0
            val pairs = source.pairs
            return lowerBound(pairs, start + to - 1) - lowerBound(pairs, start + from)
        }
    }

    private class Branch(val left: Node, val right: Node) : Node() {
        override val length = left.length + right.length
        override val height = maxOf(left.height, right.height) + 1
        override val first = left.first
        override val last = right.last
        val pairsAtJoin = if (left.last.isHighSurrogate() && right.first.isLowSurrogate()) 1 else 0
        override val pairs = left.pairs + right.pairs + pairsAtJoin
    }

    private companion object {
        const val MAX_MERGED_PIECE = 256

        fun piece(text: String): Node? =
            if (text.isEmpty()) null else Piece(Source(text), 0, text.length)

        fun split(node: Node?, at: Int): Pair<Node?, Node?> {
            if (node == null || at == 0) return null to node
            if (at == node.length) return node to null
            return when (node) {
                is Piece -> {
                    val cut = node.start + at
                    Piece(node.source, node.start, cut) to Piece(node.source, cut, node.end)
                }
                is Branch -> when {
                    at < node.left.length -> {
                        val (prefix, tail) = split(node.left, at)
                        prefix to join(tail, node.right)
                    }
                    at > node.left.length -> {
                        val (head, suffix) = split(node.right, at - node.left.length)
                        join(node.left, head) to suffix
                    }
                    else -> node.left to node.right
                }
            }
        }

        fun join(left: Node?, right: Node?): Node? {
            if (left == null) return right
            if (right == null) return left
            if (left is Piece && right is Piece && left.length + right.length <= MAX_MERGED_PIECE) {
                return piece(buildString(left.length + right.length) {
                    append(left.source.text, left.start, left.end)
                    append(right.source.text, right.start, right.end)
                })
            }
            if (left is Branch && left.height > right.height + 1) {
                return balance(left.left, join(left.right, right)!!)
            }
            if (right is Branch && right.height > left.height + 1) {
                return balance(join(left, right.left)!!, right.right)
            }
            return Branch(left, right)
        }

        fun balance(left: Node, right: Node): Node {
            if (left is Branch && left.height > right.height + 1) {
                if (left.right is Branch && left.right.height > left.left.height) {
                    val inner = left.right
                    return Branch(Branch(left.left, inner.left), Branch(inner.right, right))
                }
                return Branch(left.left, Branch(left.right, right))
            }
            if (right is Branch && right.height > left.height + 1) {
                if (right.left is Branch && right.left.height > right.right.height) {
                    val inner = right.left
                    return Branch(Branch(left, inner.left), Branch(inner.right, right.right))
                }
                return Branch(Branch(left, right.left), right.right)
            }
            return Branch(left, right)
        }

        fun countPairs(node: Node?, start: Int, end: Int): Int {
            if (node == null || end - start < 2) return 0
            if (start == 0 && end == node.length) return node.pairs
            return when (node) {
                is Piece -> node.pairsIn(start, end)
                is Branch -> {
                    val boundary = node.left.length
                    when {
                        end <= boundary -> countPairs(node.left, start, end)
                        start >= boundary -> countPairs(node.right, start - boundary, end - boundary)
                        else -> countPairs(node.left, start, boundary) + node.pairsAtJoin +
                            countPairs(node.right, 0, end - boundary)
                    }
                }
            }
        }

        fun visit(node: Node?, start: Int, end: Int, range: (Piece, Int, Int) -> Boolean): Boolean {
            if (node == null || start == end) return true
            return when (node) {
                is Piece -> range(node, node.start + start, node.start + end)
                is Branch -> {
                    val boundary = node.left.length
                    (start >= boundary || visit(node.left, start, minOf(end, boundary), range)) &&
                        (end <= boundary || visit(node.right, maxOf(start, boundary) - boundary, end - boundary, range))
                }
            }
        }

        fun lowerBound(values: IntArray, value: Int): Int {
            val index = values.binarySearch(value)
            return if (index < 0) -index - 1 else index
        }
    }
}
