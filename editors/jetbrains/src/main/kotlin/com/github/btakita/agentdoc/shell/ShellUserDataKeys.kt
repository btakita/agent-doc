package com.github.btakita.agentdoc.shell

import com.intellij.openapi.util.Key
import java.util.concurrent.ConcurrentHashMap

/**
 * GH #339 R1, per-platform-object state (editors/jetbrains/SPEC.md "Who owns IntelliJ
 * registrations and callbacks"): the `Key`s implementation generations store user data under.
 * The shell creates each key once per IDE run, so every generation reads and writes the same key
 * on a platform object, and the shell knows every name it handed out (for the retirement sweep a
 * swap needs).
 */
object ShellUserDataKeys {
    private val keys = ConcurrentHashMap<String, Key<*>>()

    @Suppress("UNCHECKED_CAST")
    fun <T> key(name: String): Key<T> = keys.computeIfAbsent(name) { Key.create<T>(it) } as Key<T>

    /** Every key name handed out so far. */
    fun names(): Set<String> = keys.keys.toSortedSet()
}
