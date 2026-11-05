package com.github.btakita.agentdoc

import com.google.gson.Gson
import com.google.gson.JsonParser
import com.google.gson.annotations.SerializedName
import com.intellij.openapi.diagnostic.Logger
import java.io.ByteArrayOutputStream
import java.nio.charset.StandardCharsets
import java.nio.file.AtomicMoveNotSupportedException
import java.nio.file.Files
import java.nio.file.Path
import java.nio.file.Paths
import java.nio.file.StandardCopyOption
import java.nio.file.attribute.FileTime
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean

/**
 * `#restartadoptscanonical`: the durable copy of `CrdtReplicaManager.settledShadows`, the last
 * text this editor PROVED the controller accepted for each document.
 *
 * The settled shadow lived only in JVM memory (a native-library reload carries it in
 * `nativeReloadSettledShadows`, an IDE restart discarded it). A plain IDE restart while the
 * controller hub survived therefore registered with no settled ancestor, and the registration
 * rules took the restarted-IDE adoption fallback: the surviving canonical was projected over the
 * buffer, even when the buffer held operator text that was saved to disk but never reached the
 * controller. The native rules now hold such a buffer instead; this store restores the ancestor
 * so a restart is an ordinary causal decision again (adopt a canonical that advanced past an
 * unchanged buffer, publish a buffer over a hub that never moved, prove or merge, or hold).
 *
 * A record is the LATEST settled shadow or nothing: an older shadow is not a safe stand-in (a
 * controller that returned to that older text would let the buffer be published over its
 * revert). A record must name exactly the document it is read for and its text must still hash to
 * the recorded content hash; anything else (missing, torn, corrupt, another version) is no
 * record, which the registration rules treat as no settled ancestor: the fail-closed hold, never
 * an adoption.
 *
 * `#shadowstorecost`: [save] runs on the replica worker each time the shadow advances
 * (`projectSettledVisibleState`), so it does no I/O and no hashing there. It records the text as
 * this document's pending shadow and schedules two background steps:
 * - an immediate INVALIDATION: once the pending text differs from the record on disk, that record
 *   is deleted (one unlink, independent of document size). A crash before the next write then
 *   leaves no record (the hold) instead of an older one (the unsafe stand-in).
 * - a debounced WRITE of whatever is pending when it runs, delayed in proportion to the document
 *   size ([writeDelayMs]), so a burst of settles costs one write and a large document is written
 *   at a bounded byte rate.
 * Every step reads the pending text under [ioLock] at the moment it writes, so a write always
 * carries the newest settle it can see: an older settle is never persisted over a newer one,
 * whatever order the scheduled steps run in. [flush] writes everything pending on the caller's
 * thread; the replica manager calls it on dispose (project close, plugin unload) and the plugin
 * generation closes the store ([close]) on unload and IDE exit, so a graceful exit never leaves
 * the latest shadow unwritten. [load] answers a pending shadow before the disk.
 *
 * Records are garbage-collected in the background ([collectGarbage], at most once per
 * [gcIntervalMs], first [GC_INITIAL_DELAY_MS] after the store is first used): a record whose
 * document no longer exists, a record neither written nor read for [retentionMs] (reading touches
 * its modification time, so it means "not opened"), an unreadable record, and a temp file a crash
 * left behind. A document this IDE session saved or loaded is never collected.
 *
 * Format: one JSON header line (`version`, `path`, `content_hash`), `\n`, then the exact UTF-8
 * text, so a multi-megabyte document is written as bytes rather than escaped into JSON. Records
 * live in the IDE system directory, not the project: a settled shadow is this editor
 * installation's own proof, never another editor's. Writes are atomic (temp file + rename in the
 * same directory) so a crash leaves the previous record or the new one, never a torn file.
 */
internal class SettledShadowStore(
    private val directory: () -> Path?,
    private val scheduler: (delayMs: Long, task: Runnable) -> Unit = { delayMs, task ->
        GenerationWork.schedule(task, delayMs, TimeUnit.MILLISECONDS)
    },
    private val clock: () -> Long = System::currentTimeMillis,
    private val retentionMs: Long = RETENTION_MS,
    private val gcIntervalMs: Long = GC_INTERVAL_MS,
    private val documentExists: (String) -> Boolean = { path ->
        runCatching { Files.exists(Paths.get(path)) }.getOrDefault(true)
    },
) {
    private val log = Logger.getInstance(SettledShadowStore::class.java)
    private val gson = Gson()

    /** Per-document state; guarded by its own monitor for the hot-path fields. */
    private class DocState {
        /** The newest settled shadow not yet on disk, or null when the record is current. */
        var pending: String? = null
        var pendingSeq: Long = 0L
        var invalidationQueued: Boolean = false
        var writeQueued: Boolean = false

        /** Content hash of the record on disk (written or verified here); null when none is known. */
        @Volatile var diskHash: String? = null
    }

    private val states = ConcurrentHashMap<String, DocState>()

    /** Serializes every record write, delete, and collection, on any thread. */
    private val ioLock = Any()
    private val warnedWriteFailure = AtomicBoolean(false)
    private val gcQueued = AtomicBoolean(false)

    @Volatile private var lastGcMs: Long = Long.MIN_VALUE

    @Volatile private var closed = false

    private data class Header(
        val version: Int,
        val path: String,
        @SerializedName("content_hash") val contentHash: String,
    )

    /** The record file for [filePath], or null when no directory is available. */
    internal fun recordPath(filePath: String): Path? =
        directory()?.resolve(contentHashUtil(filePath) + RECORD_SUFFIX)

    /**
     * Record [text] as the settled shadow of [filePath]; the hot path. No I/O or hashing here: the
     * record is invalidated and rewritten in the background (see the class comment). After [close],
     * nothing is scheduled any more, so the record is written on the calling thread instead.
     */
    fun save(filePath: String, text: String) {
        val state = states.computeIfAbsent(filePath) { DocState() }
        if (closed) {
            synchronized(state) {
                state.pending = text
                state.pendingSeq++
            }
            persist(filePath, state, write = true)
            return
        }
        var invalidate = false
        var write = false
        synchronized(state) {
            if (state.pending === text) return
            state.pending = text
            state.pendingSeq++
            if (!state.invalidationQueued) {
                state.invalidationQueued = true
                invalidate = true
            }
            if (!state.writeQueued) {
                state.writeQueued = true
                write = true
            }
        }
        if (invalidate) {
            schedule(0L) {
                synchronized(state) { state.invalidationQueued = false }
                persist(filePath, state, write = false)
            }
        }
        if (write) {
            schedule(writeDelayMs(text.length)) {
                // Cleared before reading the pending text, so a settle that lands during the write
                // schedules its own write instead of being dropped.
                synchronized(state) { state.writeQueued = false }
                persist(filePath, state, write = true)
            }
        }
        maybeScheduleGc()
    }

    /** Write every pending shadow now, on the calling thread. True when nothing is left unwritten. */
    fun flush(): Boolean {
        var ok = true
        for ((filePath, state) in states) {
            if (synchronized(state) { state.pending } != null) {
                ok = persist(filePath, state, write = true) && ok
            }
        }
        return ok
    }

    /** Plugin unload or IDE exit: write everything pending and stop scheduling background work. */
    fun close() {
        closed = true
        flush()
    }

    /** The settled shadow of [filePath]: a pending one, else a valid record, else null. */
    fun load(filePath: String): String? {
        val state = states.computeIfAbsent(filePath) { DocState() }
        val loaded =
            synchronized(ioLock) {
                // A pending shadow is newer than any record (and the record may be invalidated).
                synchronized(state) { state.pending }?.let { return@synchronized it }
                readRecord(filePath)?.also { (_, hash) -> state.diskHash = hash }?.first
            }
        maybeScheduleGc()
        return loaded
    }

    /**
     * Bring the record of [filePath] in line with its pending shadow, under [ioLock]. With [write],
     * the pending text is written; without it, a record that no longer matches is only deleted.
     * False when a needed write or delete failed.
     */
    private fun persist(filePath: String, state: DocState, write: Boolean): Boolean =
        synchronized(ioLock) {
            val (text, seq) = synchronized(state) { (state.pending ?: return true) to state.pendingSeq }
            val hash = contentHashUtil(text)
            if (hash == state.diskHash) {
                clearPending(state, seq)
                return true
            }
            val target = runCatching { recordPath(filePath) }.getOrNull()
            if (target == null) {
                clearPending(state, seq)
                return false
            }
            if (!write) {
                return try {
                    Files.deleteIfExists(target)
                    state.diskHash = null
                    true
                } catch (t: Throwable) {
                    warnWriteFailure(removed = false, t)
                    false
                }
            }
            try {
                writeRecord(target, filePath, hash, text)
                state.diskHash = hash
                clearPending(state, seq)
                true
            } catch (t: Throwable) {
                state.diskHash = null
                // The record on disk (if any) is now older than the settled shadow; never let a
                // later restart trust it. The next settle retries the write.
                val removed = runCatching { Files.deleteIfExists(target); true }.getOrDefault(false)
                clearPending(state, seq)
                warnWriteFailure(removed, t)
                false
            }
        }

    /** Drop [state]'s pending text unless a newer settle replaced it after [seq] was read. */
    private fun clearPending(state: DocState, seq: Long) {
        synchronized(state) {
            if (state.pendingSeq == seq) state.pending = null
        }
    }

    private fun writeRecord(target: Path, filePath: String, hash: String, text: String) {
        val parent = target.parent
        Files.createDirectories(parent)
        val header = gson.toJson(Header(RECORD_VERSION, filePath, hash)).toByteArray(StandardCharsets.UTF_8)
        val body = text.toByteArray(StandardCharsets.UTF_8)
        val temp = Files.createTempFile(parent, target.fileName.toString(), TEMP_SUFFIX)
        try {
            Files.newOutputStream(temp).use { out ->
                out.write(header)
                out.write('\n'.code)
                out.write(body)
            }
            try {
                Files.move(temp, target, StandardCopyOption.ATOMIC_MOVE, StandardCopyOption.REPLACE_EXISTING)
            } catch (_: AtomicMoveNotSupportedException) {
                Files.move(temp, target, StandardCopyOption.REPLACE_EXISTING)
            }
        } finally {
            Files.deleteIfExists(temp)
        }
    }

    private fun warnWriteFailure(removed: Boolean, t: Throwable) {
        if (warnedWriteFailure.compareAndSet(false, true)) {
            log.warn(
                "[crdt-replica] cannot persist the settled shadow (further failures are not logged); " +
                    "stale_record_removed=$removed; a restart of this IDE holds instead of proving " +
                    "registration (#restartadoptscanonical): ${t.message}",
            )
        }
    }

    /**
     * The verified record of [filePath] as (text, content hash), touching it as recently opened;
     * null when invalid. Under [ioLock].
     */
    private fun readRecord(filePath: String): Pair<String, String>? =
        try {
            val target = recordPath(filePath)
            if (target == null || !Files.isRegularFile(target)) {
                null
            } else {
                val bytes = Files.readAllBytes(target)
                val newline = bytes.indexOf('\n'.code.toByte())
                val root = if (newline < 0) null else parseHeader(String(bytes, 0, newline, StandardCharsets.UTF_8))
                val version = root?.version
                val path = root?.path
                val hash = root?.contentHash
                val text =
                    if (root == null) null else String(bytes, newline + 1, bytes.size - newline - 1, StandardCharsets.UTF_8)
                if (version != RECORD_VERSION || path != filePath || text == null || hash != contentHashUtil(text)) {
                    log.warn(
                        "[crdt-replica] ignoring an invalid persisted settled shadow for ${Paths.get(filePath).fileName}; " +
                            "version=$version path_matches=${path == filePath} hash_matches=" +
                            "${text != null && hash == contentHashUtil(text)} (#restartadoptscanonical)",
                    )
                    null
                } else {
                    // `#shadowstorecost`: reading is "opened"; the retention clock restarts.
                    runCatching { Files.setLastModifiedTime(target, FileTime.fromMillis(clock())) }
                    text to checkNotNull(hash)
                }
            }
        } catch (t: Throwable) {
            log.warn("[crdt-replica] cannot read the persisted settled shadow for $filePath: ${t.message}")
            null
        }

    private fun parseHeader(line: String): Header? =
        runCatching {
            val root = JsonParser.parseString(line).asJsonObject
            Header(
                version = root.get("version")?.takeIf { it.isJsonPrimitive }?.asInt ?: return null,
                path = root.get("path")?.takeIf { it.isJsonPrimitive }?.asString ?: return null,
                contentHash = root.get("content_hash")?.takeIf { it.isJsonPrimitive }?.asString ?: return null,
            )
        }.getOrNull()

    private fun schedule(delayMs: Long, task: () -> Unit) {
        val guarded = Runnable {
            // After close the store writes synchronously; a late background step must not touch a
            // directory the next plugin generation may already own.
            if (closed) return@Runnable
            try {
                task()
            } catch (t: Throwable) {
                log.warn("[crdt-replica] settled shadow background step failed: ${t.message}")
            }
        }
        try {
            scheduler(delayMs, guarded)
        } catch (t: Throwable) {
            // No scheduler (an unloading generation): do the step now rather than lose it.
            guarded.run()
        }
    }

    private fun maybeScheduleGc() {
        if (closed) return
        val last = lastGcMs
        if (last != Long.MIN_VALUE && clock() - last < gcIntervalMs) return
        if (!gcQueued.compareAndSet(false, true)) return
        schedule(GC_INITIAL_DELAY_MS) {
            try {
                collectGarbage()
            } finally {
                gcQueued.set(false)
            }
        }
    }

    /**
     * Delete the records nothing will read again (see the class comment). Returns the number of
     * files removed. Each file is examined and removed under [ioLock], so it never races a write.
     */
    internal fun collectGarbage(): Int {
        val now = clock()
        lastGcMs = now
        val dir = runCatching { directory() }.getOrNull() ?: return 0
        if (!Files.isDirectory(dir)) return 0
        val files =
            runCatching { Files.list(dir).use { listing -> listing.toList() } }.getOrNull() ?: return 0
        var removed = 0
        for (file in files) {
            val name = file.fileName.toString()
            val deleted =
                synchronized(ioLock) {
                    val modified =
                        runCatching { Files.getLastModifiedTime(file).toMillis() }.getOrNull() ?: return@synchronized false
                    val unreachable =
                        when {
                            name.endsWith(TEMP_SUFFIX) -> now - modified > TEMP_GRACE_MS
                            name.endsWith(RECORD_SUFFIX) -> {
                                val path = readHeaderPath(file)
                                when {
                                    path == null -> true
                                    // Saved or loaded this session: live, whatever its age.
                                    states.containsKey(path) -> false
                                    now - modified > retentionMs -> true
                                    else -> !documentExists(path)
                                }
                            }
                            else -> false
                        }
                    unreachable && runCatching { Files.deleteIfExists(file) }.getOrDefault(false)
                }
            if (deleted) removed++
        }
        if (removed > 0) {
            log.info("[crdt-replica] collected $removed settled shadow record(s) nothing will read again (#shadowstorecost)")
        }
        return removed
    }

    /** The document path a record's header names, read without loading its text; null when invalid. */
    private fun readHeaderPath(file: Path): String? =
        runCatching {
            val line = ByteArrayOutputStream()
            Files.newInputStream(file).buffered().use { input ->
                while (true) {
                    val b = input.read()
                    if (b < 0) return@runCatching null
                    if (b == '\n'.code) break
                    if (line.size() >= MAX_HEADER_BYTES) return@runCatching null
                    line.write(b)
                }
            }
            parseHeader(line.toString(StandardCharsets.UTF_8))
                ?.takeIf { it.version == RECORD_VERSION && file.fileName.toString() == contentHashUtil(it.path) + RECORD_SUFFIX }
                ?.path
        }.getOrNull()

    internal companion object {
        const val RECORD_VERSION = 1
        private const val RECORD_SUFFIX = ".settled-shadow"
        private const val TEMP_SUFFIX = ".tmp"
        private const val MAX_HEADER_BYTES = 64 * 1024

        /** A settle burst coalesces into one write at most this soon after its first settle. */
        const val MIN_WRITE_DELAY_MS = 250L

        /** Bound on how long a settle may wait for its write, however large the document. */
        const val MAX_WRITE_DELAY_MS = 5_000L

        /** Steady-state write budget per document: a write waits [size / budget] seconds. */
        const val WRITE_BYTES_PER_SECOND = 1_000_000L

        /** A record neither written nor read for this long is collected. */
        val RETENTION_MS: Long = TimeUnit.DAYS.toMillis(30)
        val GC_INTERVAL_MS: Long = TimeUnit.DAYS.toMillis(1)
        val GC_INITIAL_DELAY_MS: Long = TimeUnit.MINUTES.toMillis(1)

        /** A temp file older than this is a crash leftover, not a write in progress. */
        val TEMP_GRACE_MS: Long = TimeUnit.HOURS.toMillis(1)

        /** The debounce for a document of [chars] characters, proportional to its size and bounded. */
        fun writeDelayMs(chars: Int): Long =
            (chars.toLong() * 1_000L / WRITE_BYTES_PER_SECOND).coerceIn(MIN_WRITE_DELAY_MS, MAX_WRITE_DELAY_MS)

        /** The IDE-wide store under the IDE system directory (`<system>/agent-doc/settled-shadows`). */
        val Default: SettledShadowStore by lazy {
            SettledShadowStore({
                runCatching {
                    Paths.get(com.intellij.openapi.application.PathManager.getSystemPath(), "agent-doc", "settled-shadows")
                }.getOrNull()
            }).also { store ->
                // Plugin unload and IDE exit: write what is pending and stop background steps.
                PluginGeneration.registerResource("settled-shadow-store") { store.close() }
            }
        }
    }
}
