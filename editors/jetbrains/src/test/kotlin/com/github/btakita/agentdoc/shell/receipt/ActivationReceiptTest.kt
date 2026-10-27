package com.github.btakita.agentdoc.shell.receipt

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** GH #339 R1: the `agent_doc_activation_receipt/v1` line and the build-time shell identity. */
class ActivationReceiptTest {
    private val backend = ActivationReceipt(
        role = ActivationReceipt.ROLE_BACKEND,
        jvmInstanceId = "11111111-2222-3333-4444-555555555555",
        shellDigest = "a".repeat(64),
        spiVersion = SHELL_SPI_VERSION,
        implVersion = "0.2.846",
        implSha256 = null,
        generation = 1,
        epoch = 0,
    )

    @Test
    fun `receipt encodes as the documented single line and round trips`() {
        assertEquals(
            "agent_doc_activation_receipt/v1 role=backend jvm=11111111-2222-3333-4444-555555555555 " +
                "shell=${"a".repeat(64)} spi=1 impl_version=0.2.846 impl_sha256=none generation=1 epoch=0",
            backend.encode(),
        )
        assertEquals(backend, ActivationReceipt.decode(backend.encode()))
    }

    @Test
    fun `a value with a separator cannot split the line`() {
        val odd = backend.copy(implVersion = "0.2 dev=1")
        val line = odd.encode()
        assertEquals(1, line.lines().size)
        assertEquals("0.2_dev_1", ActivationReceipt.decode(line).implVersion)
    }

    @Test
    fun `a peer line that is not a v1 receipt decodes to null`() {
        assertNull(ActivationReceipt.decodeOrNull(null))
        assertNull(ActivationReceipt.decodeOrNull("agent_doc_activation_receipt/v2 role=backend"))
        assertNull(ActivationReceipt.decodeOrNull("agent_doc_activation_receipt/v1 role=backend"))
    }

    @Test
    fun `a shell-only frontend reports no implementation and the built shell digest`() {
        val frontend = ActivationReceipt.shellOnly(ActivationReceipt.ROLE_FRONTEND)
        assertEquals("frontend", frontend.role)
        assertNull(frontend.implVersion)
        assertNull(frontend.implSha256)
        assertEquals(0L, frontend.generation)
        assertEquals(ShellIdentity.shellDigest, frontend.shellDigest)
        assertTrue(frontend.encode().contains(" impl_version=none impl_sha256=none "))
    }

    @Test
    fun `the build generated a shell identity with distinct shell and impl digests`() {
        val hex = Regex("[0-9a-f]{64}")
        assertTrue(ShellIdentity.shellDigest, hex.matches(ShellIdentity.shellDigest))
        assertTrue(ShellIdentity.implDigest, hex.matches(ShellIdentity.implDigest))
        assertNotEquals(ShellIdentity.shellDigest, ShellIdentity.implDigest)
    }

    @Test
    fun `the jvm instance id is one value per JVM, shared through system properties`() {
        val id = ShellIdentity.jvmInstanceId
        assertEquals(id, ShellIdentity.jvmInstanceId)
        assertEquals(id, System.getProperty("agent.doc.shell.jvm.instance"))
    }
}
