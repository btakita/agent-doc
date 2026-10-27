package com.github.btakita.agentdoc.shell

import com.intellij.openapi.actionSystem.AnActionEvent

/*
 * GH #339 R1: the shell/implementation boundary. See `editors/jetbrains/SPEC.md`
 * "Restart-free implementation generations: shell plus child loader (GH #339 design)".
 *
 * The shell owns every IntelliJ registration and lends one implementation *generation* the
 * handlers it may serve. In R1 the generation is compiled into the plugin jar and loaded by the
 * plugin classloader; R2 moves it into a child loader. These interfaces are the only types the
 * shell knows the implementation by, so the shell never names an implementation class except
 * through [IMPL_GENERATION_CLASS].
 */

/**
 * The implementation entry point the shell instantiates by name. R2 reads the same value from the
 * implementation jar's `Agent-Doc-Impl-Class` manifest attribute instead.
 */
const val IMPL_GENERATION_CLASS: String = "com.github.btakita.agentdoc.AgentDocImplGeneration"

/**
 * One implementation generation. The shell calls these methods; the implementation never
 * registers an action with the platform itself.
 */
interface ImplGeneration {
    /** What this generation reports as `impl_version` in its activation receipt. */
    val implVersion: String

    /**
     * Build the generation. [importedState] is the previous generation's [quiesce] result, or
     * null on a cold start or an unknown schema. Must not touch the platform.
     */
    fun prepare(context: GenerationContext, importedState: String?)

    /**
     * The handler for one shell-declared action id, or null when this generation serves no such
     * id (the forwarder then hides the action). Called on the update thread; must be cheap.
     */
    fun action(actionId: String): ImplAction?

    /** Stop work and return version-neutral state for the next generation. */
    fun quiesce(): String

    /** Undo [quiesce] when a replacement generation failed before activation. */
    fun resume()
}

/** What the shell lends a generation. R1 lends identity only; later classes add their hooks here. */
interface GenerationContext {
    /** Local, per-JVM generation counter; never compared across JVMs. */
    val generation: Long

    /** The backend-assigned swap epoch; 0 until a live swap exists (R3). */
    val epoch: Long
}

/**
 * The implementation half of one shell-declared action. The shell's forwarder is the `AnAction`
 * the platform holds; it owns the id, text, shortcuts, groups, `DumbAware` and the update thread
 * (always `BGT`), and calls these with the platform's own event.
 */
interface ImplAction {
    /** Same contract as `AnAction.update`; the default leaves the presentation as the platform built it. */
    fun update(e: AnActionEvent) {}

    /** Same contract as `AnAction.actionPerformed`. */
    fun actionPerformed(e: AnActionEvent)
}
