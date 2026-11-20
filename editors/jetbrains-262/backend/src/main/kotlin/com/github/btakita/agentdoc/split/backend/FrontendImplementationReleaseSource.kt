package com.github.btakita.agentdoc.split.backend

import com.github.btakita.agentdoc.split.FrontendImplementationRelease
import com.intellij.openapi.application.PathManager
import java.nio.file.Files
import java.nio.file.Path
import java.util.Properties

/**
 * Reads the backend-owned release handoff written by agent-doc's installer/upgrader.
 *
 * The handoff may select bytes and an epoch, but it deliberately carries no client trust key.
 * The frontend verifies the detached signature against trust provisioned on its own host.
 */
internal object FrontendImplementationReleaseSource {
    const val MANIFEST_PROPERTY = "agent.doc.frontend.release.manifest"
    private const val MAX_PACKAGE_BYTES = 128L * 1024 * 1024
    private const val MAX_SIGNATURE_BYTES = 16L * 1024

    fun current(): FrontendImplementationRelease? {
        val configured = System.getProperty(MANIFEST_PROPERTY)?.trim()?.takeIf(String::isNotEmpty)
        val manifest = configured?.let(Path::of)
            ?: Path.of(PathManager.getPluginsPath()).resolve(HANDOFF_DIRECTORY).resolve(HANDOFF_MANIFEST)
        return manifest.takeIf(Files::isRegularFile)?.let(::read)
    }

    internal fun read(manifest: Path): FrontendImplementationRelease {
        val manifestPath = manifest.toAbsolutePath().normalize()
        val values = Properties().apply {
            Files.newInputStream(manifestPath).use(::load)
        }
        fun required(name: String): String =
            values.getProperty(name)?.trim()?.takeIf(String::isNotEmpty)
                ?: error("frontend release manifest is missing $name")
        fun sibling(name: String, limit: Long): ByteArray {
            val path = manifestPath.parent.resolve(required(name)).normalize()
            require(path.parent == manifestPath.parent) { "$name must name a manifest sibling" }
            val size = Files.size(path)
            require(size in 1..limit) { "$name has invalid size $size" }
            return Files.readAllBytes(path)
        }
        return FrontendImplementationRelease(
            releaseVersion = required("release_version"),
            packageUrl = required("package_url"),
            signatureUrl = required("signature_url"),
            implEntry = required("impl_entry"),
            implSha256 = required("impl_sha256"),
            shellDigest = required("shell_digest"),
            spiVersion = required("spi_version").toInt(),
            epoch = required("epoch").toLong(),
            packageBytes = sibling("package_file", MAX_PACKAGE_BYTES),
            signatureBytes = sibling("signature_file", MAX_SIGNATURE_BYTES),
        )
    }

    internal const val HANDOFF_DIRECTORY = ".agent-doc-frontend-release"
    internal const val HANDOFF_MANIFEST = "release.properties"
}
