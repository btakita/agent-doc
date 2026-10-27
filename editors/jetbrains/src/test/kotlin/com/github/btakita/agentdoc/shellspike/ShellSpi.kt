package com.github.btakita.agentdoc.shellspike

import java.util.function.Consumer

/*
 * GH #339 shell spike (test-only, never shipped). See `SPEC.md`
 * "Restart-free implementation generations: shell plus child loader (GH #339 design)".
 *
 * This file is the frozen shell/implementation boundary. Only JDK types and these interfaces
 * cross it. The interfaces are loaded once, by the shell's classloader; every implementation
 * generation sees the same `Class` objects through parent delegation, so a cast to
 * [ImplGeneration] works for every generation while no implementation class ever becomes
 * visible to the shell.
 */

/** Bumped only by a shell release. A jar built against another SPI version is refused. */
const val SHELL_SPI_VERSION: Int = 1

/** Manifest attributes an implementation jar must carry. */
const val MANIFEST_IMPL_CLASS = "Agent-Doc-Impl-Class"
const val MANIFEST_IMPL_VERSION = "Agent-Doc-Impl-Version"
const val MANIFEST_SPI_VERSION = "Agent-Doc-Shell-Spi-Version"

/** Package prefix that only an implementation jar may define, and that the shell never defines. */
const val IMPL_PACKAGE_PREFIX = "com.github.btakita.agentdoc.shellspike.impl."

/**
 * One implementation generation. The shell calls these methods; the implementation never
 * registers anything with the platform itself.
 */
interface ImplGeneration {
    /**
     * Build the generation and stage its handlers through [context]. [importedState] is the
     * previous generation's [quiesce] result, or null on a cold start or an unknown schema.
     * Must not touch the platform: activation is the shell's decision.
     */
    fun prepare(context: GenerationContext, importedState: String?)

    /** Answer one shell-forwarded request. Payloads are version-neutral strings. */
    fun handle(kind: String, payload: String): String

    /** Stop work and return version-neutral state for the next generation. */
    fun quiesce(): String

    /** Undo [quiesce] when the replacement generation failed before activation. */
    fun resume()
}

/** What the shell lends a generation. Every registration it accepts is owned by the shell. */
interface GenerationContext {
    val generation: Long
    val implVersion: String

    /** Stage a handler for a platform topic; the shell's single forwarder delivers to it. */
    fun subscribe(topic: String, handler: Consumer<String>)

    /** Queue work on the platform's UI thread through a generation-fenced shell forwarder. */
    fun invokeLater(task: Runnable)
}
