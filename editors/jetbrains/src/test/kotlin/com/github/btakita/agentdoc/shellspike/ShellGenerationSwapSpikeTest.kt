package com.github.btakita.agentdoc.shellspike

import java.io.ByteArrayOutputStream
import java.nio.file.Files
import java.nio.file.Path
import java.nio.file.Paths
import java.util.jar.Attributes
import java.util.jar.JarEntry
import java.util.jar.JarOutputStream
import java.util.jar.Manifest
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder

/**
 * GH #339 proof-of-concept spike for the restart-free shell design (`SPEC.md`, "Restart-free
 * implementation generations: shell plus child loader (GH #339 design)").
 *
 * Each implementation generation is packed from its own `impl.<name>` package into a versioned
 * jar on disk and loaded by an [ImplClassLoader]. Nothing here is wired into the shipped plugin.
 *
 * Every generation-touching step runs in a helper that returns only strings, so no stack slot of
 * a test method keeps an old generation reachable while its loader is checked.
 */
class ShellGenerationSwapSpikeTest {
    @get:Rule
    val temp = TemporaryFolder()

    private lateinit var platform: FakePlatform
    private lateinit var host: GenerationHost

    @Before
    fun setUp() {
        platform = FakePlatform()
        FakePlatform.current = platform
        host = GenerationHost(platform, role = "backend", shellDigest = SHELL_DIGEST)
    }

    @After
    fun tearDown() {
        FakePlatform.current = null
    }

    @Test
    fun `a versioned implementation jar loads in an agent-doc-owned child loader`() {
        val v1 = packImpl("v1", "CounterImplV1", "1.0.0")
        assertTrue(v1.path.fileName.toString() == "agent-doc-impl-1.0.0.jar")

        val outcome = host.installFromPath(v1.path, v1.sha256, epoch = 1)

        assertEquals(SwapOutcome.Activated(1, "1.0.0", v1.sha256, importedState = false), outcome)
        assertEquals("agent-doc-impl-g1-1.0.0", host.activeLoaderName())
        assertEquals("v1:0", host.request("count"))
        platform.publish("document-changed", "a.md")
        assertEquals("v1:1", host.request("count"))
    }

    @Test
    fun `a swap moves every registration to the new generation and carries state as data`() {
        val v1 = packImpl("v1", "CounterImplV1", "1.0.0")
        val v2 = packImpl("v2", "CounterImplV2", "2.0.0")
        host.installFromPath(v1.path, v1.sha256, epoch = 1)
        repeat(3) { platform.publish("document-changed", "a.md") }

        val outcome = host.installFromPath(v2.path, v2.sha256, epoch = 2)

        assertEquals(SwapOutcome.Activated(2, "2.0.0", v2.sha256, importedState = true), outcome)
        assertEquals("v2:3", host.request("count"))
        assertEquals("true", host.request("imported"))
        // The platform holds exactly one subscriber, the shell's forwarder, across both generations.
        assertEquals(1, platform.subscriberCount("document-changed"))
        platform.publish("document-changed", "b.md")
        assertEquals("v2:4", host.request("count"))
        assertEquals("agent-doc-impl-g2-2.0.0", host.activeLoaderName())
    }

    @Test
    fun `the retired generation's loader is collected while the platform still retains what the shell gave it`() {
        val failure = runV1ThatSchedulesAndFails()
        val v2 = packImpl("v2", "CounterImplV2", "2.0.0")
        host.installFromPath(v2.path, v2.sha256, epoch = 2)

        // The platform still holds the shell forwarder, v1's queued (never run) task, and v1's
        // reported failure. None of them may reach v1's loader.
        assertEquals(1, platform.subscriberCount("document-changed"))
        assertEquals(1, platform.queuedEvents())
        assertEquals(1, platform.errorPool.size)
        assertTrue(failure.contains("V1Failure: v1 refused payload"))

        assertEquals(emptyList<RetiredGeneration>(), host.checkRetiredCollected())
        assertTrue(host.hotSwapAllowed)
        assertEquals(null, host.retiredGenerations().single().loader.get())

        // A stale queued task from the retired generation is inert, not a resurrection.
        platform.runQueuedRetaining()
        assertEquals("v2:0", host.request("count"))
    }

    @Test
    fun `negative control - a generation that registers itself directly pins its loader and disables hot swap`() {
        val leaky = packImpl("leaky", "LeakyImpl", "0.9.0")
        val v2 = packImpl("v2", "CounterImplV2", "2.0.0")
        host.installFromPath(leaky.path, leaky.sha256, epoch = 1)
        host.installFromPath(v2.path, v2.sha256, epoch = 2)

        val survivors = host.checkRetiredCollected()

        assertEquals(listOf(1L), survivors.map { it.generation })
        assertFalse(host.hotSwapAllowed)
        val next = host.installFromPath(v2.path, v2.sha256, epoch = 3)
        assertTrue(next is SwapOutcome.RestartRequired)
        assertTrue((next as SwapOutcome.RestartRequired).reason.contains("still reachable"))
        // The leak is the platform's direct reference, not the shell's.
        assertEquals(2, platform.subscriberCount("document-changed"))
    }

    @Test
    fun `a jar whose bytes do not match the expected digest is refused and the active generation keeps running`() {
        val v1 = packImpl("v1", "CounterImplV1", "1.0.0")
        val v2 = packImpl("v2", "CounterImplV2", "2.0.0")
        host.installFromPath(v1.path, v1.sha256, epoch = 1)

        val outcome = host.installFromPath(v2.path, v1.sha256, epoch = 2)

        assertTrue(outcome is SwapOutcome.Refused)
        assertTrue((outcome as SwapOutcome.Refused).reason.startsWith("sha256 mismatch"))
        platform.publish("document-changed", "a.md")
        assertEquals("v1:1", host.request("count"))
        assertEquals(1L, host.receipt().epoch)
    }

    @Test
    fun `a replacement that fails to prepare rolls back to the quiesced generation`() {
        val v1 = packImpl("v1", "CounterImplV1", "1.0.0")
        val broken = packImpl("broken", "BrokenPrepareImpl", "1.1.0")
        host.installFromPath(v1.path, v1.sha256, epoch = 1)
        platform.publish("document-changed", "a.md")

        val outcome = host.installFromPath(broken.path, broken.sha256, epoch = 2)

        assertTrue(outcome is SwapOutcome.RolledBack)
        assertTrue((outcome as SwapOutcome.RolledBack).reason.contains("cannot import events=1"))
        platform.publish("document-changed", "b.md")
        assertEquals("v1:2", host.request("count"))
        assertEquals("1.0.0", host.receipt().implVersion)
        assertEquals(1L, host.receipt().epoch)
        assertEquals(emptyList<RetiredGeneration>(), host.checkRetiredCollected())
    }

    @Test
    fun `a jar built for another shell SPI needs a restart and changes nothing`() {
        val v1 = packImpl("v1", "CounterImplV1", "1.0.0")
        val future = packImpl("v2", "CounterImplV2", "3.0.0", spiVersion = SHELL_SPI_VERSION + 1)
        host.installFromPath(v1.path, v1.sha256, epoch = 1)

        val outcome = host.installFromPath(future.path, future.sha256, epoch = 2)

        assertTrue(outcome is SwapOutcome.RestartRequired)
        assertTrue((outcome as SwapOutcome.RestartRequired).reason.contains("shell SPI v${SHELL_SPI_VERSION + 1}"))
        assertEquals("v1:0", host.request("count"))
    }

    @Test
    fun `a jar that defines a class outside the implementation package is refused`() {
        val smuggled = packImpl(
            "v1",
            "CounterImplV1",
            "1.0.1",
            extraClasses = listOf("com.github.btakita.agentdoc.shellspike.FakePlatform"),
        )

        val outcome = host.installFromPath(smuggled.path, smuggled.sha256, epoch = 1)

        assertTrue(outcome is SwapOutcome.Refused)
        assertTrue((outcome as SwapOutcome.Refused).reason.contains("outside $IMPL_PACKAGE_PREFIX"))
        assertEquals(null, host.activeLoaderName())
    }

    @Test
    fun `implementation classes are child-only and never resolve from the shell loader`() {
        val v1 = packImpl("v1", "CounterImplV1", "1.0.0")
        val jar = VerifiedImplJar.verify(Files.readAllBytes(v1.path), v1.sha256)
        val loader = ImplClassLoader(jar, javaClass.classLoader, generation = 7)

        val loaded = loader.loadClass("$IMPL_PACKAGE_PREFIX" + "v1.CounterImplV1")
        assertTrue(loaded.classLoader === loader)
        // The shell SPI resolves parent-first, so every generation shares one ImplGeneration type.
        assertTrue(loader.loadClass(ImplGeneration::class.java.name) === ImplGeneration::class.java)
        try {
            loader.loadClass("$IMPL_PACKAGE_PREFIX" + "v2.CounterImplV2")
            fail("a class absent from this generation's jar must not fall back to the parent")
        } catch (_: ClassNotFoundException) {
        }
    }

    @Test
    fun `activation receipts round-trip as one version-neutral line`() {
        val v1 = packImpl("v1", "CounterImplV1", "1.0.0")
        host.installFromPath(v1.path, v1.sha256, epoch = 5)
        val receipt = host.receipt()

        val line = receipt.encode()

        assertTrue(line.startsWith(ActivationReceipt.PREFIX))
        assertEquals(receipt, ActivationReceipt.decode(line))
        assertEquals(5L, receipt.epoch)
        assertEquals(v1.sha256, receipt.implSha256)
        val frontendShellOnly = GenerationHost(FakePlatform(), role = "frontend", shellDigest = SHELL_DIGEST).receipt()
        assertEquals(frontendShellOnly, ActivationReceipt.decode(frontendShellOnly.encode()))
        assertEquals(null, frontendShellOnly.implSha256)
    }

    @Test
    fun `the activation fence admits only a backend and JetBrains Client on the same verified generation`() {
        val v1 = packImpl("v1", "CounterImplV1", "1.0.0")
        host.installFromPath(v1.path, v1.sha256, epoch = 4)
        val backend = host.receipt()
        val expected = ExpectedGeneration(SHELL_DIGEST, v1.sha256, epoch = 4)
        val clientShellOnly = GenerationHost(FakePlatform(), "frontend", SHELL_DIGEST).receipt()
        val clientOldShell = GenerationHost(FakePlatform(), "frontend", "older-shell").receipt()

        assertEquals(
            FenceDecision.Accepted,
            ActivationFence.decide(expected, backend, frontend = null, remoteFrontendConnected = false),
        )
        assertEquals(
            FenceDecision.Accepted,
            ActivationFence.decide(expected, backend, clientShellOnly, remoteFrontendConnected = true),
        )
        assertHold(ActivationFence.decide(expected, backend, frontend = null, remoteFrontendConnected = true), "has not reported")
        assertHold(ActivationFence.decide(expected, backend, clientOldShell, remoteFrontendConnected = true), "restart both sides")
        assertHold(ActivationFence.decide(expected, backend, backend, remoteFrontendConnected = true), "backend JVM")
        assertHold(
            ActivationFence.decide(expected.copy(epoch = 5), backend, clientShellOnly, remoteFrontendConnected = true),
            "expected",
        )
        assertHold(
            ActivationFence.decide(expected.copy(shellDigest = "newer-shell"), backend, clientShellOnly, true),
            "restart required",
        )

        // SAME_IMPL (an operator choice): the client must echo the same jar at the same epoch.
        val client = GenerationHost(FakePlatform(), "frontend", SHELL_DIGEST)
        assertHold(
            ActivationFence.decide(expected, backend, client.receipt(), true, FrontendPolicy.SAME_IMPL),
            "JetBrains Client runs impl null",
        )
        val clientPlatform = FakePlatform()
        val clientHost = GenerationHost(clientPlatform, "frontend", SHELL_DIGEST)
        clientHost.installFromPath(v1.path, v1.sha256, epoch = 4)
        assertEquals(
            FenceDecision.Accepted,
            ActivationFence.decide(expected, backend, clientHost.receipt(), true, FrontendPolicy.SAME_IMPL),
        )
        assertHold(
            ActivationFence.decide(expected, backend, clientHost.receipt(), true, FrontendPolicy.SHELL_ONLY),
            "shell-only frontend policy",
        )
    }

    @Test
    fun `the spike is not wired into the shipped plugin`() {
        val mainRoot = listOf(Paths.get("src/main"), Paths.get("editors/jetbrains/src/main")).first(Files::exists)
        val references = Files.walk(mainRoot).use { paths ->
            paths.filter(Files::isRegularFile)
                .filter { Files.readString(it).contains("shellspike") }
                .map { mainRoot.relativize(it).toString() }
                .toList()
        }
        assertEquals(emptyList<String>(), references)

        val spec = Files.readString(listOf(Paths.get("SPEC.md"), Paths.get("editors/jetbrains/SPEC.md")).first(Files::exists))
        assertTrue(spec.contains("Restart-free implementation generations: shell plus child loader (GH #339 design)"))
        assertTrue(spec.contains("The restart-required prompt remains the fallback"))
    }

    private fun assertHold(decision: FenceDecision, reasonFragment: String) {
        assertTrue("expected a hold, got $decision", decision is FenceDecision.Hold)
        val reason = (decision as FenceDecision.Hold).reason
        assertTrue("hold reason `$reason` does not mention `$reasonFragment`", reason.contains(reasonFragment))
    }

    /** Activate v1, queue one of its tasks without running it, and report one of its failures. */
    private fun runV1ThatSchedulesAndFails(): String {
        val v1 = packImpl("v1", "CounterImplV1", "1.0.0")
        host.installFromPath(v1.path, v1.sha256, epoch = 1)
        assertEquals("scheduled", host.request("schedule"))
        val failure = try {
            host.request("boom", "payload")
            fail("v1 must throw")
            ""
        } catch (error: ImplFailure) {
            platform.reportError(error)
            error.message.orEmpty()
        }
        return failure
    }

    private data class PackedJar(val path: Path, val sha256: String)

    /**
     * Pack every class file under `shellspike/impl/<subpackage>/` from the compiled test output
     * into `agent-doc-impl-<version>.jar`. The classes are copied as bytes; this test never loads
     * them through its own classloader.
     */
    private fun packImpl(
        subpackage: String,
        simpleName: String,
        version: String,
        spiVersion: Int = SHELL_SPI_VERSION,
        extraClasses: List<String> = emptyList(),
    ): PackedJar {
        val packagePath = (IMPL_PACKAGE_PREFIX + subpackage).replace('.', '/')
        val directory = javaClass.classLoader.getResource(packagePath)
            ?: error("compiled spike generation $packagePath is not on the test classpath")
        check(directory.protocol == "file") { "spike generations must be packed from a class directory, got $directory" }
        val classDir = Paths.get(directory.toURI())
        val manifest = Manifest().apply {
            mainAttributes[Attributes.Name.MANIFEST_VERSION] = "1.0"
            mainAttributes.putValue(MANIFEST_IMPL_CLASS, "$IMPL_PACKAGE_PREFIX$subpackage.$simpleName")
            mainAttributes.putValue(MANIFEST_IMPL_VERSION, version)
            mainAttributes.putValue(MANIFEST_SPI_VERSION, spiVersion.toString())
        }
        val bytes = ByteArrayOutputStream().also { buffer ->
            JarOutputStream(buffer, manifest).use { jar ->
                Files.list(classDir).use { files ->
                    files.filter { it.fileName.toString().endsWith(".class") }.sorted().forEach { file ->
                        jar.putNextEntry(JarEntry("$packagePath/${file.fileName}"))
                        jar.write(Files.readAllBytes(file))
                        jar.closeEntry()
                    }
                }
                extraClasses.forEach { className ->
                    val entry = className.replace('.', '/') + ".class"
                    val classBytes = javaClass.classLoader.getResourceAsStream(entry)!!.use { it.readBytes() }
                    jar.putNextEntry(JarEntry(entry))
                    jar.write(classBytes)
                    jar.closeEntry()
                }
            }
        }.toByteArray()
        val target = temp.root.toPath().resolve("$subpackage-$version").also(Files::createDirectories)
            .resolve("agent-doc-impl-$version.jar")
        Files.write(target, bytes)
        return PackedJar(target, VerifiedImplJar.sha256Hex(bytes))
    }

    private companion object {
        const val SHELL_DIGEST = "shell-digest-1"
    }
}
