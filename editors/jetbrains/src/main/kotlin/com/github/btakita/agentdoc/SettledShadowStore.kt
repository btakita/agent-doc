package com.github.btakita.agentdoc

import com.google.gson.Gson
import com.google.gson.JsonParser
import com.google.gson.annotations.SerializedName
import com.intellij.openapi.diagnostic.Logger
import java.nio.charset.StandardCharsets
import java.nio.file.AtomicMoveNotSupportedException
import java.nio.file.Files
import java.nio.file.Path
import java.nio.file.Paths
import java.nio.file.StandardCopyOption
import java.util.concurrent.ConcurrentHashMap
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
 * One record per document, written synchronously on the replica worker each time the shadow
 * advances (`projectSettledVisibleState`) and read once at registration when memory holds none.
 * A record is the LATEST settled shadow or nothing: an older shadow is not a safe stand-in (a
 * controller that returned to that older text would let the buffer be published over its
 * revert), so a write that fails removes the record. A record must name exactly the document it
 * is read for and its text must still hash to the recorded content hash; anything else
 * (missing, torn, corrupt, another version) is no record, which the registration rules treat as
 * no settled ancestor: the fail-closed hold, never an adoption.
 *
 * Format: one JSON header line (`version`, `path`, `content_hash`), `\n`, then the exact UTF-8
 * text, so a multi-megabyte document is written as bytes rather than escaped into JSON. Records
 * live in the IDE system directory, not the project: a settled shadow is this editor
 * installation's own proof, never another editor's. Writes are atomic (temp file + rename in the
 * same directory) so a crash leaves the previous record or the new one, never a torn file.
 */
internal class SettledShadowStore(private val directory: () -> Path?) {
    private val log = Logger.getInstance(SettledShadowStore::class.java)
    private val gson = Gson()

    /** Content hash last written per document, so an unchanged shadow is not rewritten. */
    private val writtenHashes = ConcurrentHashMap<String, String>()
    private val warnedWriteFailure = AtomicBoolean(false)

    private data class Header(
        val version: Int,
        val path: String,
        @SerializedName("content_hash") val contentHash: String,
    )

    /** The record file for [filePath], or null when no directory is available. */
    internal fun recordPath(filePath: String): Path? =
        directory()?.resolve(contentHashUtil(filePath) + RECORD_SUFFIX)

    /** Persist [text] as the settled shadow of [filePath]. False when it could not be written. */
    fun save(filePath: String, text: String): Boolean {
        val hash = contentHashUtil(text)
        if (writtenHashes[filePath] == hash) return true
        val target = runCatching { recordPath(filePath) }.getOrNull() ?: return false
        return try {
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
            writtenHashes[filePath] = hash
            true
        } catch (t: Throwable) {
            writtenHashes.remove(filePath)
            // The record on disk (if any) is now older than the settled shadow; never let a later
            // restart trust it.
            val removed = runCatching { Files.deleteIfExists(target); true }.getOrDefault(false)
            if (warnedWriteFailure.compareAndSet(false, true)) {
                log.warn(
                    "[crdt-replica] cannot persist the settled shadow (further failures are not logged); " +
                        "stale_record_removed=$removed; a restart of this IDE holds instead of proving " +
                        "registration (#restartadoptscanonical): ${t.message}",
                )
            }
            false
        }
    }

    /** The persisted settled shadow of [filePath], or null when there is no valid record. */
    fun load(filePath: String): String? =
        try {
            val target = recordPath(filePath)
            if (target == null || !Files.isRegularFile(target)) {
                null
            } else {
                val bytes = Files.readAllBytes(target)
                val newline = bytes.indexOf('\n'.code.toByte())
                val root =
                    if (newline < 0) {
                        null
                    } else {
                        runCatching {
                            JsonParser.parseString(String(bytes, 0, newline, StandardCharsets.UTF_8)).asJsonObject
                        }.getOrNull()
                    }
                val version = root?.get("version")?.takeIf { it.isJsonPrimitive }?.asInt
                val path = root?.get("path")?.takeIf { it.isJsonPrimitive }?.asString
                val hash = root?.get("content_hash")?.takeIf { it.isJsonPrimitive }?.asString
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
                    writtenHashes.putIfAbsent(filePath, hash)
                    text
                }
            }
        } catch (t: Throwable) {
            log.warn("[crdt-replica] cannot read the persisted settled shadow for $filePath: ${t.message}")
            null
        }

    internal companion object {
        const val RECORD_VERSION = 1
        private const val RECORD_SUFFIX = ".settled-shadow"
        private const val TEMP_SUFFIX = ".tmp"

        /** The IDE-wide store under the IDE system directory (`<system>/agent-doc/settled-shadows`). */
        val Default: SettledShadowStore by lazy {
            SettledShadowStore {
                runCatching {
                    Paths.get(com.intellij.openapi.application.PathManager.getSystemPath(), "agent-doc", "settled-shadows")
                }.getOrNull()
            }
        }
    }
}
