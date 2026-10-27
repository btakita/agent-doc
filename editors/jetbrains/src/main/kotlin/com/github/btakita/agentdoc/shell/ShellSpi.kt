package com.github.btakita.agentdoc.shell

import com.intellij.openapi.Disposable
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.editor.event.DocumentListener
import com.intellij.openapi.editor.event.EditorFactoryListener
import com.intellij.openapi.editor.event.EditorMouseListener
import com.intellij.openapi.project.Project
import com.intellij.util.messages.Topic

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

    /**
     * The shell's `ProjectManagerListener.projectOpened`, forwarded. Runs on every delivery,
     * including the attach bridge's replay after a dynamic load; the generation decides whether
     * the project still needs initializing.
     */
    fun projectOpened(project: Project)

    /**
     * Release this generation's resources for [project]: the shell's `projectClosed` and its
     * project lifecycle service's disposal both forward here.
     */
    fun disposeProject(project: Project)

    /**
     * The classic attach bridge's `initializeOpenProjectsAfterDynamicLoad` hook, forwarded from
     * the shell's static entry point. Returns the bridge's one-line reattach receipt.
     */
    fun initializeOpenProjectsAfterDynamicLoad(): String

    /** The classic attach bridge's `disposeOpenProjectsForDynamicUnload` hook; the projects released. */
    fun disposeOpenProjectsForDynamicUnload(): Int

    /** Stop work and return version-neutral state for the next generation. */
    fun quiesce(): String

    /** Undo [quiesce] when a replacement generation failed before activation. */
    fun resume()

    /**
     * Release everything this generation created, once, after [quiesce]: in R1 that is plugin
     * unload (or application exit), the only time a generation leaves.
     */
    fun retire()
}

/**
 * What the shell lends a generation: its identity, and the shell-owned forwarders it stages its
 * platform handlers through. Each `parent` is the disposable that released the handler's own
 * platform registration before R1; disposing it removes the handler from the forwarder.
 */
interface GenerationContext {
    /** Local, per-JVM generation counter; never compared across JVMs. */
    val generation: Long

    /** The backend-assigned swap epoch; 0 until a live swap exists (R3). */
    val epoch: Long

    /** Tier A: [topic] on the application message bus. */
    fun <L : Any> subscribe(topic: Topic<L>, handler: L, parent: Disposable)

    /** Tier A: [topic] on [project]'s message bus. */
    fun <L : Any> subscribe(project: Project, topic: Topic<L>, handler: L, parent: Disposable)

    /** Tier A: `EditorFactory.eventMulticaster.addDocumentListener`. */
    fun addDocumentListener(listener: DocumentListener, parent: Disposable)

    /** Tier A: `EditorFactory.eventMulticaster.addEditorMouseListener`. */
    fun addEditorMouseListener(listener: EditorMouseListener, parent: Disposable)

    /** Tier A: `EditorFactory.addEditorFactoryListener`. */
    fun addEditorFactoryListener(listener: EditorFactoryListener, parent: Disposable)
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
