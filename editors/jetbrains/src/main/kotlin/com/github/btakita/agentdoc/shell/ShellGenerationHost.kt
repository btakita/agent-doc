package com.github.btakita.agentdoc.shell

import com.github.btakita.agentdoc.ProjectPluginLifecycleService
import com.github.btakita.agentdoc.shell.receipt.ActivationReceipt
import com.github.btakita.agentdoc.shell.receipt.SHELL_SPI_VERSION
import com.github.btakita.agentdoc.shell.receipt.ShellIdentity
import com.intellij.openapi.Disposable
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.components.Service
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.editor.EditorFactory
import com.intellij.openapi.editor.event.DocumentListener
import com.intellij.openapi.editor.event.EditorFactoryListener
import com.intellij.openapi.editor.event.EditorMouseListener
import com.intellij.openapi.project.Project
import com.intellij.util.messages.Topic

/**
 * GH #339 R1: the shell's generation host, one per JVM. It instantiates the implementation
 * generation, serves the shell forwarders from it, and publishes this JVM's activation receipt.
 *
 * R1 runs exactly one generation per plugin load, defined by the plugin classloader itself (no
 * child loader, no swap), so behaviour is unchanged for users. The swap protocol (quiesce,
 * prepare with imported state, activate, retire, leak watch) arrives with R2/R3 on top of this
 * same host.
 */
@Service(Service.Level.APP)
class ShellGenerationHost : Disposable {
    private val lock = Any()

    @Volatile private var active: ActiveGeneration? = null

    /** Set by plugin unload; a late forwarder call then finds no generation instead of making one. */
    @Volatile private var disposed = false

    /** Set once the active generation has been retired; it then stages no more handlers. */
    @Volatile private var retired = false

    private class ActiveGeneration(val impl: ImplGeneration, val receipt: ActivationReceipt)

    /**
     * Application-level Tier A forwarders: the application message bus, the editor event
     * multicaster and the editor factory. Their platform registrations end with this host.
     */
    internal val applicationListeners =
        ShellListenerHub(this, "application") { ApplicationManager.getApplication().messageBus }

    private inner class HostContext(override val generation: Long, override val epoch: Long) : GenerationContext {
        private fun admits(what: String): Boolean {
            if (!retired) return true
            LOG.info("[shell] retired generation $generation staged $what after retirement; ignored")
            return false
        }

        override fun <L : Any> subscribe(topic: Topic<L>, handler: L, parent: Disposable) {
            if (admits(topic.displayName)) applicationListeners.subscribe(topic, handler, parent)
        }

        override fun <L : Any> subscribe(project: Project, topic: Topic<L>, handler: L, parent: Disposable) {
            if (!admits(topic.displayName)) return
            project.getService(ProjectPluginLifecycleService::class.java).listeners.subscribe(topic, handler, parent)
        }

        override fun addDocumentListener(listener: DocumentListener, parent: Disposable) {
            if (!admits(DOCUMENT_LISTENERS)) return
            applicationListeners.addListener(DOCUMENT_LISTENERS, DocumentListener::class.java, listener, parent) { forwarder, owner ->
                EditorFactory.getInstance().eventMulticaster.addDocumentListener(forwarder, owner)
            }
        }

        override fun addEditorMouseListener(listener: EditorMouseListener, parent: Disposable) {
            if (!admits(EDITOR_MOUSE_LISTENERS)) return
            applicationListeners.addListener(EDITOR_MOUSE_LISTENERS, EditorMouseListener::class.java, listener, parent) { forwarder, owner ->
                EditorFactory.getInstance().eventMulticaster.addEditorMouseListener(forwarder, owner)
            }
        }

        override fun addEditorFactoryListener(listener: EditorFactoryListener, parent: Disposable) {
            if (!admits(EDITOR_FACTORY_LISTENERS)) return
            applicationListeners.addListener(EDITOR_FACTORY_LISTENERS, EditorFactoryListener::class.java, listener, parent) { forwarder, owner ->
                EditorFactory.getInstance().addEditorFactoryListener(forwarder, owner)
            }
        }
    }

    /** Activate the implementation generation if it is not active yet; null once unloading. */
    fun activate(): ImplGeneration? = activeGeneration()?.impl

    /** The active generation's editor-extension handlers, activating it on first use. */
    fun editorExtensions(): ImplEditorExtensions? = activeGeneration()?.impl?.editorExtensions()

    /** The active generation's handler for [actionId], activating the generation on first use. */
    fun action(actionId: String): ImplAction? = activeGeneration()?.impl?.action(actionId)

    /** This JVM's current receipt; a host that has not activated reports generation 0. */
    fun receipt(): ActivationReceipt = active?.receipt ?: inactiveReceipt()

    private fun activeGeneration(): ActiveGeneration? {
        active?.let { return it }
        return synchronized(lock) {
            if (disposed) return null
            active ?: instantiate().also { activated ->
                active = activated
                LOG.info("[shell] ${activated.receipt.encode()}")
            }
        }
    }

    private fun instantiate(): ActiveGeneration {
        val generation = 1L
        val epoch = 0L
        // R1: the plugin classloader defines the implementation, so this is the same class the
        // plugin jar shipped. Instantiating it by name keeps the shell free of a compile-time
        // edge to the implementation, which R2 needs for the child loader.
        val loader = ShellGenerationHost::class.java.classLoader
        val impl = Class.forName(IMPL_GENERATION_CLASS, true, loader)
            .getDeclaredConstructor()
            .newInstance() as ImplGeneration
        impl.prepare(HostContext(generation, epoch), importedState = null)
        val receipt = ActivationReceipt(
            role = ActivationReceipt.ROLE_BACKEND,
            jvmInstanceId = ShellIdentity.jvmInstanceId,
            shellDigest = ShellIdentity.shellDigest,
            spiVersion = SHELL_SPI_VERSION,
            implVersion = impl.implVersion,
            // No separate implementation jar exists before R2; nothing was verified by sha256.
            implSha256 = null,
            generation = generation,
            epoch = epoch,
        )
        return ActiveGeneration(impl, receipt)
    }

    private fun inactiveReceipt(): ActivationReceipt =
        ActivationReceipt.shellOnly(ActivationReceipt.ROLE_BACKEND)

    /**
     * Retire the active generation now, once: it stops staging handlers, then [ImplGeneration.quiesce]
     * and [ImplGeneration.retire] run. Plugin unload calls this from `PluginUnloadCleanupService`
     * and from this host's own disposal, whichever IntelliJ disposes first; the second call finds
     * nothing to retire.
     */
    fun retireActiveGeneration() {
        val leaving = synchronized(lock) {
            disposed = true
            active.also { active = null }
        } ?: return
        retired = true
        try {
            leaving.impl.quiesce()
        } catch (failure: Throwable) {
            LOG.warn("[shell] implementation generation ${leaving.receipt.generation} failed to quiesce", failure)
        }
        try {
            leaving.impl.retire()
        } catch (failure: Throwable) {
            LOG.warn("[shell] implementation generation ${leaving.receipt.generation} failed to retire", failure)
        }
    }

    override fun dispose() {
        retireActiveGeneration()
    }

    companion object {
        private val LOG = Logger.getInstance(ShellGenerationHost::class.java)

        internal const val DOCUMENT_LISTENERS = "editor-multicaster-document"
        internal const val EDITOR_MOUSE_LISTENERS = "editor-multicaster-mouse"
        internal const val EDITOR_FACTORY_LISTENERS = "editor-factory"

        fun getInstance(): ShellGenerationHost =
            ApplicationManager.getApplication().getService(ShellGenerationHost::class.java)

        /** The host when IntelliJ has created it; teardown paths never create one. */
        fun getInstanceIfCreated(): ShellGenerationHost? =
            ApplicationManager.getApplication()?.getServiceIfCreated(ShellGenerationHost::class.java)
    }
}
