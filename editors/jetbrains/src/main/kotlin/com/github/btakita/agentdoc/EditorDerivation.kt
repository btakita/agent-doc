package com.github.btakita.agentdoc

/**
 * GH #326 (the #295 "path 2" lever): the plugin reports evidence, the hot-reloaded native library
 * derives what it means.
 *
 * On the exact-263 modular package a plugin generation is replaced only by an IDE restart, while
 * `libagent_doc` hot-reloads under the running IDE. A rule that lives behind this channel therefore
 * changes without a restart; a rule written in Kotlin does not. Callers capture plain evidence
 * (on the EDT when it is IntelliJ state), ask one named derivation, and fall back to a frozen
 * Kotlin copy only when no native generation answers. That fallback is a compatibility floor: it
 * is never extended, and new rules land in the native derivation alone.
 */
internal fun interface EditorDerivation {
    /** The `output` JSON of [derivation] for [inputJson], or `null` when no native library answers. */
    fun derive(derivation: String, inputJson: String): String?

    companion object {
        /** The loaded native generation. Never call it on the EDT. */
        val Native = EditorDerivation { derivation, inputJson ->
            NativeAdminControls.deriveEditor(derivation, inputJson)
        }

        /** No native answer: callers take their frozen fallback. */
        val Unavailable = EditorDerivation { _, _ -> null }
    }
}
