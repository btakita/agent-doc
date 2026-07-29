package com.github.btakita.agentdoc

import io.github.lazily.ThreadSafeComputed
import io.github.lazily.ThreadSafeContext
import io.github.lazily.ThreadSafeSource
import java.util.concurrent.ConcurrentHashMap

/**
 * Revision-keyed template validation on the project-lifetime reactive plane.
 *
 * Callers publish editor or remote-candidate revision facts. Validation is a
 * Computed derived from that Source, so an unchanged revision is not normalized
 * repeatedly and a changed revision invalidates the projection automatically.
 * Delivery visibility is intentionally not represented here: validation gates
 * structure-dependent mutations, while the CRDT ACK frontier uses exact
 * editor/replica/target revision evidence independently.
 */
internal class TemplateValidationPlane(
    private val context: ThreadSafeContext,
    private val normalize: (String) -> String?,
    private val hash: (String) -> String,
) {
    internal enum class Lane {
        Editor,
        RemoteCandidate,
    }

    private data class Key(
        val filePath: String,
        val lane: Lane,
    )

    private data class Revision(
        val hash: String,
        val text: String,
    )

    internal data class Projection(
        val revisionHash: String,
        val state: TemplateStructureProjectionState,
    )

    private data class Nodes(
        val revision: ThreadSafeSource<Revision>,
        val projection: ThreadSafeComputed<Projection>,
    )

    private val nodes = ConcurrentHashMap<Key, Nodes>()

    fun publish(
        filePath: String,
        lane: Lane,
        text: String,
    ): Projection {
        val key = Key(filePath, lane)
        val revision = Revision(hash(text), text)
        val graph = nodes.computeIfAbsent(key) {
            val revisionSource = context.source(revision)
            val validation = context.computed {
                val current = get(revisionSource)
                Projection(
                    revisionHash = current.hash,
                    state =
                        templateStructureProjectionStateUtil(
                            current.text,
                            normalize(current.text),
                        ),
                )
            }
            Nodes(revisionSource, validation)
        }
        context.set(graph.revision, revision)
        return context.get(graph.projection)
    }

    fun remove(filePath: String) {
        nodes.entries.removeIf { (key, graph) ->
            if (key.filePath != filePath) {
                false
            } else {
                context.disposeSlot(graph.projection)
                context.disposeCell(graph.revision)
                true
            }
        }
    }

    fun clear() {
        nodes.keys.map { it.filePath }.distinct().forEach(::remove)
    }
}
