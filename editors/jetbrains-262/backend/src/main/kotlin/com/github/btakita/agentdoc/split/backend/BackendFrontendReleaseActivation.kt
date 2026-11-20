package com.github.btakita.agentdoc.split.backend

import com.github.btakita.agentdoc.shell.receipt.ActivationReceipt
import com.github.btakita.agentdoc.shell.receipt.SHELL_SPI_VERSION
import com.github.btakita.agentdoc.shell.receipt.ShellIdentity
import com.github.btakita.agentdoc.split.FrontendImplementation
import com.github.btakita.agentdoc.split.FrontendImplementationRelease
import com.intellij.openapi.diagnostic.Logger
import java.io.ByteArrayInputStream
import java.lang.ref.ReferenceQueue
import java.lang.ref.WeakReference
import java.net.URI
import java.security.MessageDigest
import java.util.jar.JarInputStream
import java.util.zip.ZipInputStream

/** Backend peer of the client host, used to prove both JVMs activated one digest and epoch. */
internal object BackendFrontendReleaseActivation {
    private data class Active(
        val implementation: FrontendImplementation,
        val loader: ClassLoader,
        val receipt: ActivationReceipt,
    )

    private val retiredQueue = ReferenceQueue<ClassLoader>()
    private val retired = mutableListOf<WeakReference<ClassLoader>>()
    private var active: Active? = null
    private var generation = 0L
    private var hotSwapAllowed = true

    @Synchronized
    fun activate(release: FrontendImplementationRelease): ActivationReceipt {
        active?.receipt?.takeIf {
            it.implSha256 == release.implSha256 && it.epoch == release.epoch
        }?.let { return it }
        check(hotSwapAllowed) { "a retired backend peer implementation loader leaked" }
        while (true) retired.remove(retiredQueue.poll() ?: break)
        retired.removeAll { it.get() == null }
        if (retired.any { it.get() != null }) {
            hotSwapAllowed = false
            error("a retired backend peer implementation loader is still reachable")
        }
        val verified = verify(release)
        val nextGeneration = generation + 1
        val loader = BackendFrontendImplClassLoader(verified.classes, nextGeneration)
        val next = loader.loadClass(verified.entryClass).getDeclaredConstructor().newInstance()
            as FrontendImplementation
        val previous = active
        val state = previous?.implementation?.quiesce()
        try {
            next.prepare(release.epoch, state)
        } catch (failure: Throwable) {
            previous?.implementation?.resume()
            throw failure
        }
        generation = nextGeneration
        val receipt = ActivationReceipt(
            role = ActivationReceipt.ROLE_BACKEND,
            jvmInstanceId = ShellIdentity.jvmInstanceId,
            shellDigest = release.shellDigest,
            spiVersion = release.spiVersion,
            implVersion = release.releaseVersion,
            implSha256 = release.implSha256,
            generation = generation,
            epoch = release.epoch,
        )
        active = Active(next, loader, receipt)
        previous?.let {
            it.implementation.retire()
            retired += WeakReference(it.loader, retiredQueue)
        }
        return receipt
    }

    @Synchronized
    fun receipt(): ActivationReceipt =
        active?.receipt ?: ActivationReceipt.shellOnly(ActivationReceipt.ROLE_BACKEND)

    private data class Verified(val entryClass: String, val classes: Map<String, ByteArray>)

    private fun verify(release: FrontendImplementationRelease): Verified {
        require(URI(release.packageUrl).scheme == "https" && URI(release.signatureUrl).scheme == "https") {
            "release URLs must use HTTPS"
        }
        require(release.shellDigest == ShellIdentity.shellDigest) { "release shell does not match backend shell" }
        require(release.spiVersion == SHELL_SPI_VERSION) { "release shell SPI does not match backend shell" }
        require(release.epoch > 0) { "release epoch must be positive" }
        var jarBytes: ByteArray? = null
        ZipInputStream(ByteArrayInputStream(release.packageBytes)).use { zip ->
            while (true) {
                val entry = zip.nextEntry ?: break
                if (entry.name == release.implEntry || entry.name.endsWith("/${release.implEntry}")) {
                    require(jarBytes == null) { "release repeats ${release.implEntry}" }
                    jarBytes = zip.readBytes()
                }
            }
        }
        val bytes = requireNotNull(jarBytes) { "release is missing ${release.implEntry}" }
        val digest = MessageDigest.getInstance("SHA-256").digest(bytes)
            .joinToString("") { "%02x".format(it) }
        require(digest == release.implSha256) { "implementation digest mismatch" }
        val classes = linkedMapOf<String, ByteArray>()
        var entryClass: String? = null
        JarInputStream(ByteArrayInputStream(bytes)).use { jar ->
            val attributes = requireNotNull(jar.manifest?.mainAttributes) { "implementation JAR has no manifest" }
            entryClass = attributes.getValue("Agent-Doc-Impl-Class")
            require(attributes.getValue("Agent-Doc-Impl-Version") == release.releaseVersion)
            require(attributes.getValue("Agent-Doc-Shell-Spi-Version") == release.spiVersion.toString())
            while (true) {
                val entry = jar.nextJarEntry ?: break
                if (!entry.isDirectory && entry.name.endsWith(".class")) {
                    val name = entry.name.removeSuffix(".class").replace('/', '.')
                    require(name.startsWith(IMPL_PREFIX)) { "implementation defines $name outside $IMPL_PREFIX" }
                    classes[name] = jar.readBytes()
                }
            }
        }
        val resolved = requireNotNull(entryClass) { "implementation manifest has no entry class" }
        require(resolved in classes) { "implementation entry class is absent" }
        return Verified(resolved, classes)
    }

    private const val IMPL_PREFIX = "com.github.btakita.agentdoc.impl."
}

private class BackendFrontendImplClassLoader(
    private val classes: Map<String, ByteArray>,
    generation: Long,
) : ClassLoader("agent-doc-backend-frontend-impl-g$generation", FrontendImplementation::class.java.classLoader) {
    override fun loadClass(name: String, resolve: Boolean): Class<*> {
        if (!name.startsWith("com.github.btakita.agentdoc.impl.")) return super.loadClass(name, resolve)
        synchronized(getClassLoadingLock(name)) {
            val loaded = findLoadedClass(name) ?: classes[name]?.let { defineClass(name, it, 0, it.size) }
                ?: throw ClassNotFoundException(name)
            if (resolve) resolveClass(loaded)
            return loaded
        }
    }
}

/** Implementation-owned bridge served through [FrontendReleaseBridge] to the Fleet RPC shell. */
internal class BackendFrontendReleaseBridge : FrontendReleaseBridge {
    override fun current(): FrontendImplementationRelease? =
        FrontendImplementationReleaseSource.current()?.takeIf { release ->
            runCatching { BackendFrontendReleaseActivation.activate(release) }
                .onFailure { LOG.warn("[frontend-impl] backend peer activation refused", it) }
                .isSuccess
        }

    private companion object {
        val LOG = Logger.getInstance(BackendFrontendReleaseBridge::class.java)
    }
}
