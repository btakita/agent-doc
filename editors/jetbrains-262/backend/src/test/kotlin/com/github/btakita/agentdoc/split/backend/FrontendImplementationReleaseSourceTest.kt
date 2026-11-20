package com.github.btakita.agentdoc.split.backend

import java.nio.file.Files
import kotlin.io.path.writeBytes
import kotlin.io.path.writeText
import kotlin.test.assertContentEquals
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import org.junit.Test

class FrontendImplementationReleaseSourceTest {
    @Test
    fun `the backend handoff carries signed release bytes without client trust material`() {
        val directory = Files.createTempDirectory("agent-doc-frontend-release-test")
        val packageBytes = byteArrayOf(1, 2, 3)
        val signatureBytes = byteArrayOf(4, 5)
        directory.resolve("plugin.zip").writeBytes(packageBytes)
        directory.resolve("plugin.zip.sig").writeBytes(signatureBytes)
        val manifest = directory.resolve("release.properties")
        manifest.writeText(
            """
            release_version=1.2.3
            package_url=https://example.invalid/plugin.zip
            signature_url=https://example.invalid/plugin.zip.sig
            impl_entry=impl/frontend.jar
            impl_sha256=${"a".repeat(64)}
            shell_digest=${"b".repeat(64)}
            spi_version=1
            epoch=42
            package_file=plugin.zip
            signature_file=plugin.zip.sig
            """.trimIndent(),
        )

        val release = FrontendImplementationReleaseSource.read(manifest)

        assertEquals("1.2.3", release.releaseVersion)
        assertEquals(42, release.epoch)
        assertContentEquals(packageBytes, release.packageBytes)
        assertContentEquals(signatureBytes, release.signatureBytes)
    }

    @Test
    fun `the handoff cannot read package bytes outside its directory`() {
        val directory = Files.createTempDirectory("agent-doc-frontend-release-traversal-test")
        val manifest = directory.resolve("release.properties")
        manifest.writeText(
            """
            release_version=1.2.3
            package_url=https://example.invalid/plugin.zip
            signature_url=https://example.invalid/plugin.zip.sig
            impl_entry=impl/frontend.jar
            impl_sha256=${"a".repeat(64)}
            shell_digest=${"b".repeat(64)}
            spi_version=1
            epoch=42
            package_file=../plugin.zip
            signature_file=plugin.zip.sig
            """.trimIndent(),
        )

        assertFailsWith<IllegalArgumentException> {
            FrontendImplementationReleaseSource.read(manifest)
        }
    }
}
