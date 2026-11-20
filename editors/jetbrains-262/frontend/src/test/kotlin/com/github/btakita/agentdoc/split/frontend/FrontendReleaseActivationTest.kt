package com.github.btakita.agentdoc.split.frontend

import com.github.btakita.agentdoc.impl.frontend.TestFrontendImplementation
import com.github.btakita.agentdoc.shell.receipt.ActivationReceipt
import com.github.btakita.agentdoc.shell.receipt.SHELL_SPI_VERSION
import com.github.btakita.agentdoc.shell.receipt.ShellIdentity
import java.io.ByteArrayOutputStream
import java.net.URI
import java.security.KeyPairGenerator
import java.security.MessageDigest
import java.security.Signature
import java.util.jar.Attributes
import java.util.jar.JarEntry
import java.util.jar.JarOutputStream
import java.util.jar.Manifest
import java.util.zip.ZipEntry
import java.util.zip.ZipOutputStream
import kotlin.test.assertEquals
import kotlin.test.assertIs
import kotlin.test.assertTrue
import org.junit.Test

class FrontendReleaseActivationTest {
    @Test
    fun `the client authenticates the published ZIP before activating its exact implementation entry`() {
        val release = releasePackage()
        val host = FrontendReleaseActivationHost(FrontendReleaseTrust.of(release.publicKey))

        val outcome = host.activate(release.request, release.zip, release.signature)

        val activated = assertIs<FrontendActivationOutcome.Activated>(outcome)
        assertEquals(ActivationReceipt.ROLE_FRONTEND, activated.receipt.role)
        assertEquals(release.request.implSha256, activated.receipt.implSha256)
        assertEquals(release.request.epoch, activated.receipt.epoch)
        assertEquals(1L, activated.receipt.generation)
        assertEquals(activated.receipt, host.receipt())
    }

    @Test
    fun `backend metadata cannot add a signer to the client trust boundary`() {
        val release = releasePackage()
        val untrusted = KeyPairGenerator.getInstance("Ed25519").generateKeyPair()
        val host = FrontendReleaseActivationHost(FrontendReleaseTrust.of(untrusted.public))

        val outcome = host.activate(release.request, release.zip, release.signature)

        val refused = assertIs<FrontendActivationOutcome.Refused>(outcome)
        assertTrue(refused.reason.contains("client-trusted release key"))
        assertEquals(null, host.receipt().implSha256)
        assertEquals(0L, host.receipt().epoch)
    }

    @Test
    fun `a signed package with the wrong implementation digest is refused without replacing the active generation`() {
        val release = releasePackage()
        val host = FrontendReleaseActivationHost(FrontendReleaseTrust.of(release.publicKey))
        assertIs<FrontendActivationOutcome.Activated>(host.activate(release.request, release.zip, release.signature))
        val active = host.receipt()

        val outcome = host.activate(
            release.request.copy(implSha256 = "0".repeat(64), epoch = 8),
            release.zip,
            release.signature,
        )

        val refused = assertIs<FrontendActivationOutcome.Refused>(outcome)
        assertTrue(refused.reason.contains("sha256 mismatch"))
        assertEquals(active, host.receipt())
    }

    @Test
    fun `the platform-neutral host retains retired loaders only weakly`() {
        val release = releasePackage()
        val host = FrontendReleaseActivationHost(FrontendReleaseTrust.of(release.publicKey))
        assertIs<FrontendActivationOutcome.Activated>(host.activate(release.request, release.zip, release.signature))
        assertIs<FrontendActivationOutcome.Activated>(
            host.activate(release.request.copy(epoch = 8), release.zip, release.signature),
        )

        val retired = host.retiredLoaderReferences().single()
        repeat(40) {
            if (retired.get() == null) return@repeat
            System.gc()
            Thread.sleep(10)
        }

        assertEquals(null, retired.get(), "the retired child loader remained strongly reachable")
        assertIs<FrontendActivationOutcome.Activated>(
            host.activate(release.request.copy(epoch = 9), release.zip, release.signature),
        )
    }

    private data class ReleasePackage(
        val request: FrontendReleaseRequest,
        val zip: ByteArray,
        val signature: ByteArray,
        val publicKey: java.security.PublicKey,
    )

    private fun releasePackage(): ReleasePackage {
        val entryClass = TestFrontendImplementation::class.java.name
        val classEntry = entryClass.replace('.', '/') + ".class"
        val classBytes = requireNotNull(javaClass.classLoader.getResourceAsStream(classEntry)).use { it.readBytes() }
        val manifest = Manifest().apply {
            mainAttributes[Attributes.Name.MANIFEST_VERSION] = "1.0"
            mainAttributes.putValue("Agent-Doc-Impl-Class", entryClass)
            mainAttributes.putValue("Agent-Doc-Impl-Version", "1.0.0")
            mainAttributes.putValue("Agent-Doc-Shell-Spi-Version", SHELL_SPI_VERSION.toString())
        }
        val implJar = ByteArrayOutputStream().also { output ->
            JarOutputStream(output, manifest).use { jar ->
                jar.putNextEntry(JarEntry(classEntry))
                jar.write(classBytes)
                jar.closeEntry()
            }
        }.toByteArray()
        val implEntry = "impl/agent-doc-frontend-impl-1.0.0.jar"
        val zip = ByteArrayOutputStream().also { output ->
            ZipOutputStream(output).use { archive ->
                archive.putNextEntry(ZipEntry(implEntry))
                archive.write(implJar)
                archive.closeEntry()
            }
        }.toByteArray()
        val keys = KeyPairGenerator.getInstance("Ed25519").generateKeyPair()
        val signature = Signature.getInstance("Ed25519").run {
            initSign(keys.private)
            update(zip)
            sign()
        }
        val digest = MessageDigest.getInstance("SHA-256").digest(implJar)
            .joinToString("") { "%02x".format(it) }
        return ReleasePackage(
            request = FrontendReleaseRequest(
                releaseVersion = "1.0.0",
                packageUrl = URI("https://github.com/btakita/agent-doc/releases/download/v1.0.0/plugin.zip"),
                signatureUrl = URI("https://github.com/btakita/agent-doc/releases/download/v1.0.0/plugin.zip.sig"),
                implEntry = implEntry,
                implSha256 = digest,
                shellDigest = ShellIdentity.shellDigest,
                spiVersion = SHELL_SPI_VERSION,
                epoch = 7,
            ),
            zip = zip,
            signature = signature,
            publicKey = keys.public,
        )
    }
}
