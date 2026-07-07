package com.github.btakita.agentdoc

/**
 * `#lzlosstree` Phase 4 plugin binding: a thin Kotlin wrapper over the lossless-tree
 * frame-exchange FFI (`agent_doc_lossless_tree_*`). Business logic lives in the CLI /
 * native library (FFI-first, Shared Foundation pattern); this object only marshals
 * strings and owns the free contract.
 *
 * A capable editor uses [render] to turn an incoming durable tree projection into
 * buffer text and [project] to snapshot its buffer into a frame, and [isCurrent] to
 * prove a projection still describes the visible buffer before trusting it. The
 * capability token ([CAPABILITY]) is what the plugin advertises so the binary knows it
 * can route a session's live merge authority through the tree.
 */
object LosslessTreeFrames {
    /** The advertised capability token, or null if the native library is unavailable. */
    val CAPABILITY: String? by lazy {
        // Borrowed static C string — must NOT be freed.
        AgentDocLib.get()?.agent_doc_lossless_tree_capability()?.getString(0)
    }

    /** Render a durable tree projection JSON back to document text, or null on failure. */
    fun render(projectionJson: String): String? {
        val lib = AgentDocLib.get() ?: return null
        val ptr = lib.agent_doc_lossless_tree_render(projectionJson) ?: return null
        return try {
            ptr.getString(0)
        } finally {
            lib.agent_doc_free_string(ptr)
        }
    }

    /** Project buffer text into a durable tree projection JSON, or null on failure. */
    fun project(docText: String): String? {
        val lib = AgentDocLib.get() ?: return null
        val ptr = lib.agent_doc_lossless_tree_project(docText) ?: return null
        return try {
            ptr.getString(0)
        } finally {
            lib.agent_doc_free_string(ptr)
        }
    }

    /**
     * Whether [projectionJson] still describes [visibleText] — the frontier/hash proof
     * required before a projection may overwrite the editor-visible buffer. Fails closed
     * (false) when the native library is unavailable or the projection can't be validated.
     */
    fun isCurrent(projectionJson: String, visibleText: String): Boolean {
        val lib = AgentDocLib.get() ?: return false
        return lib.agent_doc_lossless_tree_projection_current(projectionJson, visibleText) == 1
    }
}
