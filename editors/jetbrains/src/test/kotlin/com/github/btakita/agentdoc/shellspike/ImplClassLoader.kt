package com.github.btakita.agentdoc.shellspike

import java.io.ByteArrayInputStream
import java.security.MessageDigest
import java.util.jar.JarInputStream
import java.util.jar.Manifest

/** A verified implementation jar, read once into memory. */
class VerifiedImplJar private constructor(
    val sha256: String,
    val implClass: String,
    val implVersion: String,
    val spiVersion: Int,
    internal val classes: Map<String, ByteArray>,
) {
    companion object {
        /**
         * Verify [bytes] against [expectedSha256] and the jar's own manifest. Classes are defined
         * from these same bytes, so nothing can change between verification and class loading
         * (no reopen of the path, no `JarFile` cache, no file lock on Windows).
         */
        fun verify(bytes: ByteArray, expectedSha256: String): VerifiedImplJar {
            val actual = sha256Hex(bytes)
            if (!actual.equals(expectedSha256, ignoreCase = true)) {
                throw ImplJarRefused("sha256 mismatch: expected $expectedSha256, jar is $actual")
            }
            val classes = LinkedHashMap<String, ByteArray>()
            var manifest: Manifest? = null
            JarInputStream(ByteArrayInputStream(bytes)).use { jar ->
                manifest = jar.manifest
                while (true) {
                    val entry = jar.nextJarEntry ?: break
                    if (entry.isDirectory) continue
                    val name = entry.name
                    if (!name.endsWith(".class")) continue
                    val className = name.removeSuffix(".class").replace('/', '.')
                    if (!className.startsWith(IMPL_PACKAGE_PREFIX)) {
                        // A jar that defines a shell or platform class could shadow the boundary
                        // types and split class identity across generations.
                        throw ImplJarRefused("jar defines $className outside $IMPL_PACKAGE_PREFIX")
                    }
                    classes[className] = jar.readBytes()
                }
            }
            val attributes = manifest?.mainAttributes ?: throw ImplJarRefused("jar has no manifest")
            val implClass = attributes.getValue(MANIFEST_IMPL_CLASS)
                ?: throw ImplJarRefused("manifest is missing $MANIFEST_IMPL_CLASS")
            val implVersion = attributes.getValue(MANIFEST_IMPL_VERSION)
                ?: throw ImplJarRefused("manifest is missing $MANIFEST_IMPL_VERSION")
            val spiVersion = attributes.getValue(MANIFEST_SPI_VERSION)?.toIntOrNull()
                ?: throw ImplJarRefused("manifest is missing $MANIFEST_SPI_VERSION")
            if (implClass !in classes) throw ImplJarRefused("entry class $implClass is not in the jar")
            return VerifiedImplJar(actual, implClass, implVersion, spiVersion, classes)
        }

        fun sha256Hex(bytes: ByteArray): String =
            MessageDigest.getInstance("SHA-256").digest(bytes).joinToString("") { "%02x".format(it) }
    }
}

class ImplJarRefused(message: String) : RuntimeException(message)

/**
 * The agent-doc-owned loader for one implementation generation.
 *
 * Delegation is parent-first for everything (JDK, Kotlin stdlib, the platform, the shell SPI)
 * except [IMPL_PACKAGE_PREFIX], which is child-only: an implementation class is defined from this
 * generation's jar or not at all, never resolved from the parent. That rule is what keeps two
 * generations' class identities apart and keeps the shell from ever holding an implementation
 * type.
 */
class ImplClassLoader(
    private val jar: VerifiedImplJar,
    parent: ClassLoader,
    generation: Long,
) : ClassLoader("agent-doc-impl-g$generation-${jar.implVersion}", parent) {
    override fun loadClass(name: String, resolve: Boolean): Class<*> {
        if (!name.startsWith(IMPL_PACKAGE_PREFIX)) return super.loadClass(name, resolve)
        synchronized(getClassLoadingLock(name)) {
            val loaded = findLoadedClass(name) ?: run {
                val bytes = jar.classes[name] ?: throw ClassNotFoundException(name)
                defineClass(name, bytes, 0, bytes.size)
            }
            if (resolve) resolveClass(loaded)
            return loaded
        }
    }

    override fun findClass(name: String): Class<*> = loadClass(name, false)
}
