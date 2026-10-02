package com.github.btakita.agentdoc

import java.nio.file.Files
import java.nio.file.Paths
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * `#retainedprojectionclobbersoperatortext` — the 2026-08-12 lost prompt.
 *
 * Registration was refused for hours, the operator typed into a buffer that
 * reached nothing, and a compaction computed against disk before those
 * keystrokes was retained and then adopted over them on attach. The prompt
 * existed only in that buffer, so it survived nowhere: not git, not the CRDT,
 * not the compaction archive.
 */
class RetainedCanonicalOperatorTextTest {

    @Test
    fun `fresh controller reseed publishes a proven live buffer instead of temporary empty canonical`() {
        assertEquals(
            RetainedRegistrationProjectionAction.PublishOperatorBuffer,
            retainedRegistrationProjectionActionForAttachUtil(
                deferCanonicalProjectionForPendingLocal = false,
                canonicalProjectionRetained = false,
                retainedReplicaReseedPending = true,
                publishedShadow = "# Session\n\n<!-- agent:queue -->\n<!-- /agent:queue -->\n",
                bufferText = "# Session\n\n<!-- agent:queue -->\n- operator prompt\n<!-- /agent:queue -->\n",
                canonicalText = "",
            ),
        )
    }

    @Test
    fun `fresh controller reseed without a settled ancestor holds without mutation`() {
        assertEquals(
            RetainedRegistrationProjectionAction.HoldOperatorBuffer,
            retainedRegistrationProjectionActionForAttachUtil(
                deferCanonicalProjectionForPendingLocal = false,
                canonicalProjectionRetained = false,
                retainedReplicaReseedPending = true,
                publishedShadow = null,
                bufferText = "# Session\n\noperator text\n",
                canonicalText = "",
            ),
        )
    }

    @Test
    fun `fresh controller reseed transition table never adopts temporary canonical`() {
        val facts = listOf<String?>(null, "# settled\n")
        for (pendingLocal in listOf(false, true)) {
            for (publishedShadow in facts) {
                for (bufferText in facts) {
                    val action =
                        retainedRegistrationProjectionActionForAttachUtil(
                            deferCanonicalProjectionForPendingLocal = pendingLocal,
                            canonicalProjectionRetained = false,
                            retainedReplicaReseedPending = true,
                            publishedShadow = publishedShadow,
                            bufferText = bufferText,
                            canonicalText = "",
                        )
                    val expected =
                        when {
                            pendingLocal -> RetainedRegistrationProjectionAction.DeferCanonicalProjection
                            publishedShadow != null && bufferText != null ->
                                RetainedRegistrationProjectionAction.PublishOperatorBuffer
                            else -> RetainedRegistrationProjectionAction.HoldOperatorBuffer
                        }
                    assertEquals(expected, action)
                    assertFalse(action == RetainedRegistrationProjectionAction.ApplyCanonical)
                }
            }
        }
    }

    @Test
    fun `pending captured edits defer a fresh bootstrap even without retained delivery`() {
        assertEquals(
            RetainedRegistrationProjectionAction.DeferCanonicalProjection,
            retainedRegistrationProjectionActionForAttachUtil(
                deferCanonicalProjectionForPendingLocal = true,
                canonicalProjectionRetained = false,
                canonicalCoversRetainedFrontier = null,
                publishedShadow = "old response and queue",
                bufferText = "old response and edited queue",
                canonicalText = "new response and queue",
            ),
        )
    }
    @Test
    fun `first captured local delta defers retained canonical projection`() {
        assertEquals(
            RetainedRegistrationProjectionAction.DeferCanonicalProjection,
            retainedRegistrationProjectionActionForAttachUtil(
                deferCanonicalProjectionForPendingLocal = true,
                canonicalProjectionRetained = true,
                publishedShadow = null,
                bufferText = "# doc\n\noperator deleted the backlog item\n",
                canonicalText = "# doc\n\n- [ ] stale backlog item\n",
            ),
        )
    }

    @Test
    fun `a live buffer is published when retained canonical is its exact shadow`() {
        val shadow = "# doc\n\nlast published state\n"
        assertEquals(
            RetainedRegistrationProjectionAction.PublishOperatorBuffer,
            retainedRegistrationProjectionActionUtil(
                publishedShadow = shadow,
                bufferText = "$shadow\ndo the thing I just typed\n",
                canonicalText = shadow,
            ),
        )
    }

    @Test
    fun `three divergent generations hold without overwriting the operator`() {
        assertEquals(
            RetainedRegistrationProjectionAction.HoldOperatorBuffer,
            retainedRegistrationProjectionActionUtil(
                publishedShadow = "# doc\n\nlast published state\n",
                bufferText = "# doc\n\nlast published state\n\ndo the thing I just typed\n",
                canonicalText = "# doc\n\na different remote generation\n",
            ),
        )
    }

    @Test
    fun `a clean three-way merge carries the operator buffer forward instead of holding`() {
        // `#editorauth2`: api.md 2026-09-29 15:35 shape. The operator typed after the
        // settled shadow, a handoff moved canonical with an agent write, and the
        // hold had no exit. A clean reconcile publishes both sides; a conflicting
        // or unavailable reconcile still holds; no path adopts canonical over the buffer.
        val shadow = "# doc\n\nlast published state\n"
        val buffer = "${shadow}do the thing I just typed\n"
        val canonical = "# doc\nagent write\n\nlast published state\n"
        for ((clean, expected) in listOf(
            true to RetainedRegistrationProjectionAction.MergeForward,
            false to RetainedRegistrationProjectionAction.HoldOperatorBuffer,
            null to RetainedRegistrationProjectionAction.HoldOperatorBuffer,
        )) {
            for (retained in listOf(true, false)) {
                assertEquals(
                    "clean=$clean retained=$retained",
                    expected,
                    retainedRegistrationProjectionActionForAttachUtil(
                        deferCanonicalProjectionForPendingLocal = false,
                        canonicalProjectionRetained = retained,
                        publishedShadow = shadow,
                        bufferText = buffer,
                        canonicalText = canonical,
                        cleanMergeAvailable = clean,
                    ),
                )
            }
        }
    }

    @Test
    fun `a proven rebase still adopts canonical ahead of a merge`() {
        val shadow = "# doc\n\nFix api.md issue\n"
        val buffer = "${shadow}typed\n"
        val canonical = "$buffer\n### Re: replayed\n"
        assertEquals(
            RetainedRegistrationProjectionAction.ApplyCanonical,
            retainedRegistrationProjectionActionUtil(
                publishedShadow = shadow,
                bufferText = buffer,
                canonicalText = canonical,
                canonicalContainsOperatorEdits = true,
                cleanMergeAvailable = true,
            ),
        )
    }

    @Test
    fun `canonical proven to contain the operator edits ends the three-generation hold`() {
        // `#ambiguousholdforever`: the controller ingested the operator's paste,
        // then appended a response beside it. The hold had no exit and refused
        // registration every second for hours; the proof is its exit.
        val shadow = "# doc\n\nFix api.md issue\n"
        val buffer = "$shadow```\npasted log\n```\n"
        val canonical = "$buffer\n### Re: replayed\n"
        for ((proof, expected) in listOf(
            true to RetainedRegistrationProjectionAction.ApplyCanonical,
            false to RetainedRegistrationProjectionAction.HoldOperatorBuffer,
            null to RetainedRegistrationProjectionAction.HoldOperatorBuffer,
        )) {
            assertEquals(
                expected,
                retainedRegistrationProjectionActionForAttachUtil(
                    deferCanonicalProjectionForPendingLocal = false,
                    canonicalProjectionRetained = true,
                    publishedShadow = shadow,
                    bufferText = buffer,
                    canonicalText = canonical,
                    canonicalContainsOperatorEdits = proof,
                ),
            )
        }
    }

    @Test
    fun `single splice batch spans only the changed code points`() {
        val batch = singleSpliceBatchUtil("a🌍 fix bug\n", "a🌍 fix the bug\n")
        assertEquals(listOf(PreparedLocalEditorEdit(7, 0, "the ")), batch.edits)
        assertEquals("a🌍 fix the bug\n", batch.resultingText)
        assertEquals(emptyList<PreparedLocalEditorEdit>(), singleSpliceBatchUtil("same", "same").edits)
        assertEquals(
            listOf(PreparedLocalEditorEdit(1, 2, "")),
            singleSpliceBatchUtil("abcd", "ad").edits,
        )
    }

    private fun reregisterAction(
        shadow: String?,
        buffer: String?,
        canonical: String?,
        containsOperatorEdits: Boolean? = null,
    ) = retainedRegistrationProjectionActionForAttachUtil(
        deferCanonicalProjectionForPendingLocal = false,
        canonicalProjectionRetained = false,
        publishedShadow = shadow,
        bufferText = buffer,
        canonicalText = canonical,
        canonicalContainsOperatorEdits = containsOperatorEdits,
    )

    @Test
    fun `a library-reload re-register publishes text typed while detached instead of overwriting it`() {
        // `#reloadclobbersoperatortext`, tasks/api.md 2026-09-28 23:08:16: the
        // operator typed during the `make install` reload window; canonical was
        // still the settled shadow, and the re-register projected it over the buffer.
        val shadow = "<!-- agent:queue go -->\n- Does PR 591 have a contracts\n<!-- /agent:queue -->\n"
        val buffer = "<!-- agent:queue go -->\n- Does PR 591 have a contracts PR? Also check SDK.\n<!-- /agent:queue -->\n"
        assertEquals(
            RetainedRegistrationProjectionAction.PublishOperatorBuffer,
            reregisterAction(shadow = shadow, buffer = buffer, canonical = shadow),
        )
    }

    @Test
    fun `a library-reload re-register still adopts remote writes when the operator typed nothing`() {
        val shadow = "# doc\n\nbefore\n"
        assertEquals(
            RetainedRegistrationProjectionAction.ApplyCanonical,
            reregisterAction(shadow = shadow, buffer = shadow, canonical = "# doc\n\nagent response\n"),
        )
    }

    @Test
    fun `a library-reload re-register with three divergent generations holds unless canonical has the edits`() {
        val shadow = "# doc\n\nbase\n"
        val buffer = "# doc\n\nbase plus operator\n"
        val canonical = "# doc\n\nbase plus agent\n"
        assertEquals(
            RetainedRegistrationProjectionAction.HoldOperatorBuffer,
            reregisterAction(shadow = shadow, buffer = buffer, canonical = canonical),
        )
        assertEquals(
            RetainedRegistrationProjectionAction.ApplyCanonical,
            reregisterAction(shadow = shadow, buffer = buffer, canonical = canonical, containsOperatorEdits = true),
        )
    }

    @Test
    fun `a restarted IDE has no shadow, so adoption stays the recovery`() {
        // The shadow map is in-memory and does not survive a restart. Without it
        // the buffer is a stale reconstruction and canonical must win — the
        // behaviour this guard must not regress.
        assertEquals(
            RetainedRegistrationProjectionAction.ApplyCanonical,
            retainedRegistrationProjectionActionUtil(
                publishedShadow = null,
                bufferText = "# doc\n\nanything at all\n",
                canonicalText = "# doc\n\ncontroller state\n",
            ),
        )
    }

    @Test
    fun `an unacknowledged local shadow cannot authorize an older controller overwrite`() {
        val localText = "# doc\n\noperator edit accepted only by the retiring controller\n"
        assertEquals(
            RetainedRegistrationProjectionAction.HoldOperatorBuffer,
            retainedRegistrationProjectionActionUtil(
                publishedShadow = localText,
                bufferText = localText,
                canonicalText = "# doc\n\nolder replacement controller snapshot\n",
            ),
        )
    }

    @Test
    fun `a causally covered settled buffer accepts retained canonical recovery`() {
        val settledBuffer = "# doc\n\nlast controller-accepted editor projection\n"
        assertEquals(
            RetainedRegistrationProjectionAction.ApplyCanonical,
            retainedRegistrationProjectionActionUtil(
                canonicalCoversRetainedFrontier = true,
                publishedShadow = settledBuffer,
                bufferText = settledBuffer,
                canonicalText = "# doc\n\nnewer retained controller projection\n",
            ),
        )
    }

    @Test
    fun `an uncovered settled buffer keeps the ambiguity hold`() {
        val settledBuffer = "# doc\n\nprojection known only to the retiring controller\n"
        assertEquals(
            RetainedRegistrationProjectionAction.HoldOperatorBuffer,
            retainedRegistrationProjectionActionUtil(
                canonicalCoversRetainedFrontier = false,
                publishedShadow = settledBuffer,
                bufferText = settledBuffer,
                canonicalText = "# doc\n\nolder replacement controller projection\n",
            ),
        )
    }

    @Test
    fun `a replacement canonical that catches the live buffer ends the hold`() {
        val liveBuffer = "# doc\n\noperator edit\n"
        assertEquals(
            RetainedRegistrationProjectionAction.ApplyCanonical,
            retainedRegistrationProjectionActionUtil(
                publishedShadow = "# doc\n\nolder settled projection\n",
                bufferText = liveBuffer,
                canonicalText = liveBuffer,
            ),
        )
    }

    @Test
    fun `an ambiguous hold suppresses refresh until its retry is due`() {
        assertFalse(
            retainedProjectionHoldAllowsRefreshUtil(
                holdActive = true,
                registrationAttemptDue = false,
            ),
        )
        assertTrue(
            retainedProjectionHoldAllowsRefreshUtil(
                holdActive = true,
                registrationAttemptDue = true,
            ),
        )
        assertTrue(
            retainedProjectionHoldAllowsRefreshUtil(
                holdActive = false,
                registrationAttemptDue = false,
            ),
        )
    }

    @Test
    fun `provisional transport registration cannot reset projection retry state`() {
        val source =
            Files.readString(
                Paths.get("src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt"),
            )
        val registration =
            source.substringAfter("    private fun forwarderFor(")
                .substringBefore("    /** Complete the causal projection decision")
        val finalization =
            source.substringAfter("    private fun finalizeRegistrationProjection(")
                .substringBefore("    /**", missingDelimiterValue = source)

        assertFalse(registration.contains("clearRegisterFailure(filePath)"))
        assertTrue(finalization.contains("if (committed)"))
        assertTrue(finalization.contains("clearRegisterFailure(filePath)"))
    }

    @Test
    fun `unknown buffer text is not divergence`() {
        // A closed or unreadable document proves nothing, and guessing "diverged"
        // would strand every retained projection behind a document nobody has open.
        assertEquals(
            RetainedRegistrationProjectionAction.ApplyCanonical,
            retainedRegistrationProjectionActionUtil(
                publishedShadow = "# doc\n",
                bufferText = null,
                canonicalText = "# doc\n\ncontroller response\n",
            ),
        )
    }

    // lazily.md 2026-10-01 21:42:17: a reload_library generation handoff left the
    // operator's queue edits quarantined by the controller (projected=false, the
    // settled shadow stayed at the pre-edit canonical). The controller redelivered
    // that pre-edit canonical as a REPLACE; the plugin retained it while the buffer
    // was unsaved, and Run Agent Doc's save made the buffer "clean", so the REPLACE
    // was installed over the edited queue.
    private val settledQueue =
        "# lazily\n\n<!-- agent:queue -->\n- old item\n<!-- /agent:queue -->\n"
    private val editedQueue =
        "# lazily\n\n<!-- agent:queue -->\n- rewritten item\n- new item\n<!-- /agent:queue -->\n"

    @Test
    fun `replace delivery of the settled canonical never clobbers unaccepted operator queue edits`() {
        assertTrue(
            replaceDeliveryWouldClobberUnsettledOperatorTextUtil(
                settledShadow = settledQueue,
                bufferText = editedQueue,
                canonicalText = settledQueue,
            ),
        )
        // The re-register that replaces the REPLACE publishes the buffer: canonical
        // is exactly the settled shadow the operator edited from.
        assertEquals(
            RetainedRegistrationProjectionAction.PublishOperatorBuffer,
            retainedRegistrationProjectionActionForAttachUtil(
                deferCanonicalProjectionForPendingLocal = false,
                canonicalProjectionRetained = false,
                publishedShadow = settledQueue,
                bufferText = editedQueue,
                canonicalText = settledQueue,
            ),
        )
    }

    @Test
    fun `replace delivery still installs when the operator has no unaccepted text`() {
        val compacted = "# lazily\n\n<!-- agent:queue -->\n<!-- /agent:queue -->\n"
        // Buffer is the accepted projection: an out-of-band deletion may replace it.
        assertFalse(
            replaceDeliveryWouldClobberUnsettledOperatorTextUtil(settledQueue, settledQueue, compacted),
        )
        // Already converged.
        assertFalse(
            replaceDeliveryWouldClobberUnsettledOperatorTextUtil(settledQueue, editedQueue, editedQueue),
        )
        // Restarted IDE: no settled frontier, historical semantics stand.
        assertFalse(replaceDeliveryWouldClobberUnsettledOperatorTextUtil(null, editedQueue, settledQueue))
        assertFalse(replaceDeliveryWouldClobberUnsettledOperatorTextUtil(settledQueue, null, compacted))
        // Canonical advanced AND the operator typed: still refused; registration
        // decides (containment proof, merge forward, or hold), never this REPLACE.
        assertTrue(replaceDeliveryWouldClobberUnsettledOperatorTextUtil(settledQueue, editedQueue, compacted))
    }

    @Test
    fun `replace delivery checks the live buffer against the settled shadow before any save gate`() {
        val source =
            Files.readString(
                listOf(
                    Paths.get("src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt"),
                    Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt"),
                ).first { Files.exists(it) },
            )
        val replace =
            source.substringAfter("    private fun applyReplaceDelivery(")
                .substringBefore("    private fun queueRemoteTextApply(")
        val guard = replace.indexOf("replaceDeliveryWouldClobberUnsettledOperatorTextUtil(")
        assertTrue("REPLACE must consult the settled-shadow guard", guard >= 0)
        assertTrue(replace.substring(guard, guard + 200).contains("settledShadows[filePath]"))
        // Before the clean/unsaved gate: Run Agent Doc's save must not open a path around it.
        assertTrue(guard < replace.indexOf("refreshCleanDocumentBeforeRemoteApply("))
        assertTrue(guard < replace.indexOf("applyMinimalDocumentEditUtil("))
        // The refusal re-registers from the buffer and never retains the canonical
        // for a lazy projection over the operator text.
        val refusal =
            replace.substringAfter("unsettledOperatorBuffer?.let")
                .substringBefore("deferredEditorText?.let")
        assertTrue(refusal.contains("refreshReplicaAfterTransportLoss("))
        assertFalse(refusal.contains("retainedCanonicalProjectionPaths.add("))
        assertTrue(refusal.contains("return false"))
    }
}
