package com.github.btakita.agentdoc.split.frontend

import com.github.btakita.agentdoc.shell.receipt.ActivationReceipt
import com.github.btakita.agentdoc.shell.receipt.SHELL_SPI_VERSION
import com.github.btakita.agentdoc.shell.receipt.ShellIdentity
import com.github.btakita.agentdoc.split.FrontendImplementation
import com.intellij.openapi.application.PathManager
import java.io.ByteArrayInputStream
import java.lang.ref.ReferenceQueue
import java.lang.ref.WeakReference
import java.net.URI
import java.nio.file.Files
import java.nio.file.Path
import java.security.KeyFactory
import java.security.MessageDigest
import java.security.PublicKey
import java.security.Signature
import java.security.spec.X509EncodedKeySpec
import java.util.Base64
import java.util.jar.JarInputStream
import java.util.zip.ZipInputStream

/**
 * Client-visible metadata for one backend-selected implementation generation.
 *
 * None of these fields grants trust. The JetBrains Client authenticates [packageUrl]'s bytes with
 * a public key provisioned on the client host, then proves that the ZIP's [implEntry] has exactly
 * [implSha256]. Backend metadata can select a published generation, but cannot add a signer.
 */
data class FrontendReleaseRequest(
    val releaseVersion: String,
    val packageUrl: URI,
    val signatureUrl: URI,
    val implEntry: String,
    val implSha256: String,
    val shellDigest: String,
    val spiVersion: Int,
    val epoch: Long,
)

data class VerifiedFrontendImplementation(
    val entryClass: String,
    val implVersion: String,
    val implSha256: String,
    val classes: Map<String, ByteArray>,
)

sealed class FrontendActivationOutcome {
    data class Activated(val receipt: ActivationReceipt) : FrontendActivationOutcome()
    data class Refused(val reason: String) : FrontendActivationOutcome()
    data class RestartRequired(val reason: String) : FrontendActivationOutcome()
    data class RolledBack(val reason: String) : FrontendActivationOutcome()
}

/** Client-local release signer trust. Backend metadata never participates in this lookup. */
class FrontendReleaseTrust private constructor(val publicKeys: List<PublicKey>) {
    init {
        require(publicKeys.isNotEmpty()) { "at least one client-trusted release key is required" }
    }

    companion object {
        const val PUBLIC_KEYS_PROPERTY = "agent.doc.frontend.release.public-keys"
        const val PUBLIC_KEYS_FILE = ".agent-doc-frontend-release/trusted-ed25519-public-keys"

        fun fromSystemProperties(): FrontendReleaseTrust {
            val configured = System.getProperty(PUBLIC_KEYS_PROPERTY).orEmpty()
            val encoded = configured.takeIf(String::isNotBlank) ?: run {
                val trustFile = Path.of(PathManager.getPluginsPath()).resolve(PUBLIC_KEYS_FILE)
                if (Files.isRegularFile(trustFile)) Files.readString(trustFile) else ""
            }
            require(encoded.isNotBlank()) {
                "$PUBLIC_KEYS_PROPERTY and $PUBLIC_KEYS_FILE are not configured on the JetBrains Client host"
            }
            return fromBase64(encoded.split(',', '\n').map(String::trim).filter(String::isNotEmpty))
        }

        fun fromBase64(encodedKeys: List<String>): FrontendReleaseTrust {
            val factory = KeyFactory.getInstance("Ed25519")
            return FrontendReleaseTrust(encodedKeys.map { encoded ->
                factory.generatePublic(X509EncodedKeySpec(Base64.getDecoder().decode(encoded)))
            })
        }

        fun of(vararg keys: PublicKey): FrontendReleaseTrust = FrontendReleaseTrust(keys.toList())
    }
}

/** Verifies the published ZIP and extracts the implementation from the authenticated bytes. */
object FrontendReleaseVerifier {
    private const val IMPL_PREFIX = "com.github.btakita.agentdoc.impl."
    private const val IMPL_CLASS_ATTRIBUTE = "Agent-Doc-Impl-Class"
    private const val IMPL_VERSION_ATTRIBUTE = "Agent-Doc-Impl-Version"
    private const val SPI_VERSION_ATTRIBUTE = "Agent-Doc-Shell-Spi-Version"
    private val SHA_256 = Regex("[0-9a-f]{64}")

    fun verify(
        request: FrontendReleaseRequest,
        pluginZip: ByteArray,
        detachedSignature: ByteArray,
        trust: FrontendReleaseTrust,
    ): VerifiedFrontendImplementation {
        require(request.packageUrl.scheme == "https") { "release package URL must use HTTPS" }
        require(request.signatureUrl.scheme == "https") { "release signature URL must use HTTPS" }
        require(request.implEntry.startsWith("impl/") && request.implEntry.endsWith(".jar")) {
            "implementation entry must be one impl/*.jar path"
        }
        require(".." !in request.implEntry.split('/')) { "implementation entry must not traverse paths" }
        require(request.implSha256.matches(SHA_256)) { "implementation sha256 must be lowercase hex" }
        require(request.epoch > 0) { "implementation epoch must be positive" }
        require(request.shellDigest == ShellIdentity.shellDigest) {
            "release shell ${request.shellDigest} does not match this client shell ${ShellIdentity.shellDigest}"
        }
        require(request.spiVersion == SHELL_SPI_VERSION) {
            "release requires shell SPI ${request.spiVersion}, client has $SHELL_SPI_VERSION"
        }
        require(trust.publicKeys.any { verifies(it, pluginZip, detachedSignature) }) {
            "published plugin ZIP is not signed by a client-trusted release key"
        }

        var implJar: ByteArray? = null
        ZipInputStream(ByteArrayInputStream(pluginZip)).use { zip ->
            while (true) {
                val entry = zip.nextEntry ?: break
                if (entry.name == request.implEntry || entry.name.endsWith("/${request.implEntry}")) {
                    require(implJar == null) { "plugin ZIP repeats ${request.implEntry}" }
                    implJar = zip.readBytes()
                }
            }
        }
        val jarBytes = requireNotNull(implJar) { "published plugin ZIP is missing ${request.implEntry}" }
        val digest = sha256(jarBytes)
        require(digest == request.implSha256) {
            "implementation sha256 mismatch: expected ${request.implSha256}, ZIP contains $digest"
        }

        val classes = linkedMapOf<String, ByteArray>()
        var entryClass: String? = null
        var implVersion: String? = null
        var spiVersion: Int? = null
        JarInputStream(ByteArrayInputStream(jarBytes)).use { jar ->
            val attributes = requireNotNull(jar.manifest?.mainAttributes) { "implementation JAR has no manifest" }
            entryClass = attributes.getValue(IMPL_CLASS_ATTRIBUTE)
            implVersion = attributes.getValue(IMPL_VERSION_ATTRIBUTE)
            spiVersion = attributes.getValue(SPI_VERSION_ATTRIBUTE)?.toIntOrNull()
            while (true) {
                val entry = jar.nextJarEntry ?: break
                if (entry.isDirectory || !entry.name.endsWith(".class")) continue
                val className = entry.name.removeSuffix(".class").replace('/', '.')
                require(className.startsWith(IMPL_PREFIX)) {
                    "implementation JAR defines $className outside $IMPL_PREFIX"
                }
                require(classes.put(className, jar.readBytes()) == null) {
                    "implementation JAR repeats $className"
                }
            }
        }
        val resolvedEntry = requireNotNull(entryClass) { "manifest is missing $IMPL_CLASS_ATTRIBUTE" }
        val resolvedVersion = requireNotNull(implVersion) { "manifest is missing $IMPL_VERSION_ATTRIBUTE" }
        require(resolvedEntry in classes) { "implementation entry class $resolvedEntry is absent" }
        require(resolvedVersion == request.releaseVersion) {
            "implementation version $resolvedVersion is not release ${request.releaseVersion}"
        }
        require(spiVersion == request.spiVersion) {
            "implementation JAR uses shell SPI $spiVersion, expected ${request.spiVersion}"
        }
        return VerifiedFrontendImplementation(resolvedEntry, resolvedVersion, digest, classes.toMap())
    }

    private fun verifies(key: PublicKey, bytes: ByteArray, signature: ByteArray): Boolean =
        Signature.getInstance("Ed25519").run {
            initVerify(key)
            update(bytes)
            verify(signature)
        }

    internal fun sha256(bytes: ByteArray): String =
        MessageDigest.getInstance("SHA-256").digest(bytes).joinToString("") { "%02x".format(it) }
}

private class FrontendImplClassLoader(
    private val implementation: VerifiedFrontendImplementation,
    generation: Long,
    parent: ClassLoader,
) : ClassLoader("agent-doc-frontend-impl-g$generation-${implementation.implVersion}", parent) {
    override fun loadClass(name: String, resolve: Boolean): Class<*> {
        if (!name.startsWith("com.github.btakita.agentdoc.impl.")) return super.loadClass(name, resolve)
        synchronized(getClassLoadingLock(name)) {
            val loaded = findLoadedClass(name) ?: implementation.classes[name]?.let { bytes ->
                defineClass(name, bytes, 0, bytes.size)
            } ?: throw ClassNotFoundException(name)
            if (resolve) resolveClass(loaded)
            return loaded
        }
    }
}

/**
 * Atomic frontend generation activation. The caller supplies already-downloaded release bytes;
 * only this client-owned host consults the client trust store and creates the child loader.
 */
class FrontendReleaseActivationHost(
    private val trust: FrontendReleaseTrust,
    private val parent: ClassLoader = FrontendImplementation::class.java.classLoader,
) {
    private data class Active(
        val impl: FrontendImplementation,
        val loader: ClassLoader,
        val receipt: ActivationReceipt,
    )

    private val retiredQueue = ReferenceQueue<ClassLoader>()
    private val retired = mutableListOf<WeakReference<ClassLoader>>()
    private var generation = 0L
    private var active: Active? = null
    var hotSwapAllowed: Boolean = true
        private set

    @Synchronized
    fun activate(
        request: FrontendReleaseRequest,
        pluginZip: ByteArray,
        detachedSignature: ByteArray,
    ): FrontendActivationOutcome {
        if (!hotSwapAllowed) return FrontendActivationOutcome.RestartRequired("a retired implementation loader leaked")
        while (true) {
            val collected = retiredQueue.poll() ?: break
            retired.remove(collected)
        }
        retired.removeAll { it.get() == null }
        if (retired.any { it.get() != null }) {
            hotSwapAllowed = false
            return FrontendActivationOutcome.RestartRequired("a retired implementation loader is still reachable")
        }
        val verified = try {
            FrontendReleaseVerifier.verify(request, pluginZip, detachedSignature, trust)
        } catch (failure: Exception) {
            return FrontendActivationOutcome.Refused(failure.message.orEmpty())
        }
        val nextGeneration = generation + 1
        val loader = FrontendImplClassLoader(verified, nextGeneration, parent)
        val next = try {
            loader.loadClass(verified.entryClass).getDeclaredConstructor().newInstance() as FrontendImplementation
        } catch (failure: Throwable) {
            return FrontendActivationOutcome.Refused("implementation entry could not load: ${failure.message}")
        }
        val previous = active
        try {
            val previousState = previous?.impl?.quiesce()
            next.prepare(request.epoch, previousState)
        } catch (failure: Throwable) {
            previous?.impl?.resume()
            return FrontendActivationOutcome.RolledBack(failure.message ?: failure.javaClass.simpleName)
        }
        generation = nextGeneration
        val receipt = ActivationReceipt(
            role = ActivationReceipt.ROLE_FRONTEND,
            jvmInstanceId = ShellIdentity.jvmInstanceId,
            shellDigest = request.shellDigest,
            spiVersion = request.spiVersion,
            implVersion = verified.implVersion,
            implSha256 = verified.implSha256,
            generation = generation,
            epoch = request.epoch,
        )
        active = Active(next, loader, receipt)
        previous?.let { leaving ->
            leaving.impl.retire()
            retired += WeakReference(leaving.loader, retiredQueue)
        }
        return FrontendActivationOutcome.Activated(receipt)
    }

    @Synchronized
    fun receipt(): ActivationReceipt = active?.receipt ?: ActivationReceipt.shellOnly(ActivationReceipt.ROLE_FRONTEND)

    /** Weak-only evidence used by the production leak fence and its deterministic test. */
    @Synchronized
    internal fun retiredLoaderReferences(): List<WeakReference<ClassLoader>> = retired.toList()
}

/**
 * Process-wide frontend activation surface. Fleet transport hands release candidates to
 * [activate]; surface snapshots read [receipt] at capture time, so a successful swap and its
 * schema-5 proof become visible together.
 */
object FrontendReleaseActivation {
    @Volatile private var host: FrontendReleaseActivationHost? = null

    @Synchronized
    fun activate(
        request: FrontendReleaseRequest,
        pluginZip: ByteArray,
        detachedSignature: ByteArray,
    ): FrontendActivationOutcome {
        val current = host ?: runCatching {
            FrontendReleaseActivationHost(FrontendReleaseTrust.fromSystemProperties())
        }.getOrElse { failure ->
            return FrontendActivationOutcome.Refused(failure.message ?: failure.javaClass.simpleName)
        }.also { host = it }
        return current.activate(request, pluginZip, detachedSignature)
    }

    fun receipt(): ActivationReceipt =
        host?.receipt() ?: ActivationReceipt.shellOnly(ActivationReceipt.ROLE_FRONTEND)
}
