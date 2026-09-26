package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.nio.file.Files
import java.nio.file.Paths

/**
 * `#jbupgradereattach`: an open-document reattach shortfall is a receipt, not an upgrade
 * verdict.
 *
 * Measured 2026-09-26: `make install` exited 2 with "replacement plugin did not reclaim open
 * documents: dynamic plugin load did not reattach src/boost-client/tasks/monsterrodholders.md"
 * while the immediate retry reported the package already byte-identical at 0.2.427 with no
 * restart required. The replacement bytes had converged and the new generation was live; only
 * the post-load assertion over one document from an unrelated open project had failed.
 */
class DynamicPluginUpgradeReattachTest {
    private fun source(relative: String): String =
        Files.readString(
            listOf(
                Paths.get("src/main/$relative"),
                Paths.get("editors/jetbrains/src/main/$relative"),
            ).first { Files.exists(it) },
        )

    @Test
    fun `a converged reattach receipt carries counts only`() {
        assertEquals(
            "documents=2/2",
            dynamicLoadReattachReceipt(
                nativeReloadReplicaRestartReport(
                    expectedPaths = listOf("/p/a.md", "/p/b.md"),
                    attachedPaths = listOf("/p/a.md", "/p/b.md"),
                ),
            ),
        )
        assertEquals(
            "documents=0/0",
            dynamicLoadReattachReceipt(
                nativeReloadReplicaRestartReport(emptyList(), emptyList()),
            ),
        )
    }

    @Test
    fun `a pending document is named in the receipt instead of raising`() {
        val receipt = dynamicLoadReattachReceipt(
            nativeReloadReplicaRestartReport(
                expectedPaths = listOf("/p/a.md", "/other/tasks/monsterrodholders.md"),
                attachedPaths = listOf("/p/a.md"),
            ),
        )

        assertEquals("documents=1/2:pending=/other/tasks/monsterrodholders.md", receipt)
    }

    /**
     * Pending paths are the receipt's last field, so the installer can take everything after
     * `pending=` verbatim and a path containing `:` survives intact.
     */
    @Test
    fun `pending paths stay last and keep the receipt on one line`() {
        val receipt = dynamicLoadReattachReceipt(
            nativeReloadReplicaRestartReport(
                expectedPaths = listOf("/p/od:d/a.md", "/p/b\nc.md"),
                attachedPaths = emptyList(),
            ),
        )

        assertTrue(receipt, receipt.startsWith("documents=0/2:pending="))
        assertFalse("a receipt must stay on one line", receipt.contains("\n"))
        assertTrue("a path's own colon must survive", receipt.contains("/p/od:d/a.md"))
        assertEquals(
            "the report sorts expected paths, so pending order is stable",
            listOf("/p/b c.md", "/p/od:d/a.md"),
            receipt.substringAfter(":pending=").split(","),
        )
    }

    @Test
    fun `the whole-IDE receipt merges every open project`() {
        val merged = mergeReplicaRestartReports(
            listOf(
                nativeReloadReplicaRestartReport(listOf("/a/x.md"), listOf("/a/x.md")),
                nativeReloadReplicaRestartReport(listOf("/b/y.md", "/b/z.md"), listOf("/b/z.md")),
            ),
        )

        assertEquals(3, merged.expected)
        assertEquals(2, merged.attached)
        assertEquals(listOf("/b/y.md"), merged.failedPaths)
        assertFalse(merged.converged)
        assertEquals("documents=2/3:pending=/b/y.md", dynamicLoadReattachReceipt(merged))
        assertTrue(mergeReplicaRestartReports(emptyList()).converged)
    }

    /**
     * The upgrade verdict belongs to the byte/descriptor checks. Nothing on the post-load
     * reattach path may raise it back into an install failure.
     */
    @Test
    fun `the post-load reattach path cannot fail the upgrade`() {
        val manager = source("kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt")
        val wait = manager
            .substringAfter("fun ensureOpenDocumentReplicasAndWait(")
            .substringBefore("fun ensureOpenDocumentReplica(")
        assertFalse(
            "a reattach shortfall must not be raised as a failure",
            wait.contains("did not reattach") || wait.contains("check(failed"),
        )
        assertTrue(
            "the wait must report a typed receipt",
            wait.contains("return nativeReloadReplicaRestartReport("),
        )

        val lifecycle = source("kotlin/com/github/btakita/agentdoc/PluginLifecycleListener.kt")
        assertTrue(
            "the attach bridge entry point must hand back a receipt, not a bare count",
            lifecycle.contains("fun initializeOpenProjectsAfterDynamicLoad(): String"),
        )
        assertTrue(
            "a shortfall must still be recorded for the operator",
            lifecycle.contains("awaiting replica re-registration"),
        )
        assertTrue(
            "one project that cannot produce a receipt must not erase the others'",
            lifecycle.contains("nativeReloadReplicaRestartReport(listOf(label), emptyList())"),
        )
        assertTrue(
            "nor may it read as a converged reattach",
            lifecycle.indexOf("} catch (failure: Exception) {") <
                lifecycle.indexOf("nativeReloadReplicaRestartReport(listOf(label), emptyList())"),
        )

        val action = source("java/com/github/btakita/agentdoc/JetBrainsPluginUpgradeAction.java")
        assertFalse(
            "the abort that failed make install after the upgrade landed must be gone",
            action.contains("did not reclaim open documents"),
        )
        val reattach = action.substringAfter("private static String reattachOpenDocuments(")
        assertFalse(
            "the reattach receipt must never throw: the replacement bytes are already live",
            reattach.substringBefore("private static String singleLine(").contains("throw "),
        )
        assertTrue(
            "an unreachable receipt is still a landed upgrade",
            reattach.contains("reattach_error="),
        )
        assertTrue(
            "the upgrade verdict stays with the byte and descriptor checks",
            action.contains("dynamic install returned "),
        )
    }
}
