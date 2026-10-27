package com.github.btakita.agentdoc.shell

import com.github.btakita.agentdoc.shell.receipt.ActivationReceipt
import com.github.btakita.agentdoc.shell.receipt.SHELL_SPI_VERSION
import com.github.btakita.agentdoc.shell.receipt.ShellIdentity
import com.intellij.openapi.Disposable
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.components.Service
import com.intellij.openapi.diagnostic.Logger

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

    private class ActiveGeneration(val impl: ImplGeneration, val receipt: ActivationReceipt)

    private class HostContext(override val generation: Long, override val epoch: Long) : GenerationContext

    /** Activate the implementation generation if it is not active yet; null once unloading. */
    fun activate(): ImplGeneration? = activeGeneration()?.impl

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

    override fun dispose() {
        val retired = synchronized(lock) {
            disposed = true
            active.also { active = null }
        } ?: return
        try {
            retired.impl.quiesce()
        } catch (failure: Throwable) {
            LOG.warn("[shell] implementation generation ${retired.receipt.generation} failed to quiesce", failure)
        }
    }

    companion object {
        private val LOG = Logger.getInstance(ShellGenerationHost::class.java)

        fun getInstance(): ShellGenerationHost =
            ApplicationManager.getApplication().getService(ShellGenerationHost::class.java)
    }
}
