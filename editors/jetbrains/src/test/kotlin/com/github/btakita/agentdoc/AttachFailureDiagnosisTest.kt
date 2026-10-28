package com.github.btakita.agentdoc

import com.google.gson.Gson
import com.google.gson.JsonNull
import com.google.gson.JsonObject
import com.google.gson.JsonParser
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.nio.file.Files
import java.nio.file.Paths

/** GH #326 phase 5b: the CRDT replica attach-failure diagnosis behind the native `attach_failure_diagnosis` derivation. */
class AttachFailureDiagnosisTest {
    private val gson = Gson()

    private fun fixture(): JsonObject {
        val stream = javaClass.classLoader.getResourceAsStream("editor-derivations/attach_failure_diagnosis.json")
            ?: error("GH #326 attach-failure diagnosis parity fixture not on the test classpath")
        val raw = stream.bufferedReader(Charsets.UTF_8).use { it.readText() }
        return JsonParser.parseString(raw).asJsonObject
    }

    private fun source(name: String): String =
        Files.readString(Paths.get("src/main/kotlin/com/github/btakita/agentdoc/$name"))

    /** The answer in the fixture's wire shape. */
    private fun wire(diagnosis: AttachFailureDiagnosis.Diagnosis): JsonObject =
        JsonObject().apply {
            add("remedy", diagnosis.remedy?.let { gson.toJsonTree(it) } ?: JsonNull.INSTANCE)
            addProperty("status_token", diagnosis.statusToken)
            addProperty("needs_operator_surface", diagnosis.needsOperatorSurface)
            addProperty("needs_liveness_republish", diagnosis.needsLivenessRepublish)
            addProperty(
                "retry_disposition",
                when (diagnosis.retryDisposition) {
                    AttachFailureDiagnosis.RetryDisposition.Automatic -> "automatic"
                    AttachFailureDiagnosis.RetryDisposition.AwaitOperatorEvidence -> "await_operator_evidence"
                },
            )
            add("hold_message", diagnosis.holdMessage?.let { gson.toJsonTree(it) } ?: JsonNull.INSTANCE)
            addProperty(
                "hold_notification",
                when (diagnosis.holdNotification) {
                    AttachFailureDiagnosis.HoldNotification.Error -> "error"
                    AttachFailureDiagnosis.HoldNotification.LogOnly -> "log_only"
                },
            )
        }

    @Test
    fun `GH 326 frozen attach-failure floor answers every parity case and sends exactly its input`() {
        // The same file pins the native derivation in Rust
        // (gh326_attach_failure_diagnosis_answers_the_shared_parity_cases).
        val cases = fixture().getAsJsonArray("cases").map { it.asJsonObject }
        assertTrue("the parity file lost cases", cases.size >= 32)
        for (case in cases) {
            val name = case.get("name").asString
            val input = case.getAsJsonObject("input")
            // The plugin always knows its reason; the fixture's absent reason is the empty one.
            val reason = input.get("reason")?.asString ?: ""
            val fileName = input.get("file_name")?.asString
            val liveReplicaAttached = input.get("live_replica_attached")?.asBoolean ?: false
            if (input.has("reason")) {
                assertEquals(
                    name,
                    input,
                    JsonParser.parseString(
                        gson.toJson(AttachFailureDiagnosis.evidence(reason, fileName, liveReplicaAttached)),
                    ).asJsonObject,
                )
            }
            // A native rule that diverged on purpose keeps the frozen answer beside it.
            val frozenExpected = case.getAsJsonObject("frozen_expected") ?: case.getAsJsonObject("expected")
            assertEquals(
                name,
                frozenExpected,
                wire(AttachFailureDiagnosis.frozen(reason, fileName, liveReplicaAttached)),
            )
        }
    }

    @Test
    fun `the plugin sends the reason and executes the native answer`() {
        var asked: String? = null
        var sent: String? = null
        val diagnosis =
            AttachFailureDiagnosis.diagnose("controller_error: timeout", "plan.md") { derivation, input ->
                asked = derivation
                sent = input
                // A rule the frozen floor does not have: the native answer wins.
                """{"remedy":"Recycle the controller.","status_token":"controller_timeout",""" +
                    """"needs_operator_surface":true,"needs_liveness_republish":true,""" +
                    """"retry_disposition":"await_operator_evidence","hold_message":"newer text"}"""
            }
        assertEquals("attach_failure_diagnosis", asked)
        assertEquals(
            JsonParser.parseString("""{"reason":"controller_error: timeout","file_name":"plan.md"}"""),
            JsonParser.parseString(sent),
        )
        assertEquals(
            AttachFailureDiagnosis.Diagnosis(
                remedy = "Recycle the controller.",
                statusToken = "controller_timeout",
                needsOperatorSurface = true,
                needsLivenessRepublish = true,
                retryDisposition = AttachFailureDiagnosis.RetryDisposition.AwaitOperatorEvidence,
                holdMessage = "newer text",
            ),
            diagnosis,
        )
    }

    @Test
    fun `a native null remedy means no advice`() {
        val diagnosis =
            AttachFailureDiagnosis.diagnose("native-ffi-unavailable") { _, _ ->
                """{"remedy":null,"status_token":"native-ffi-unavailable",""" +
                    """"needs_operator_surface":false,"needs_liveness_republish":false,""" +
                    """"retry_disposition":"automatic","hold_message":null}"""
            }
        assertNull(diagnosis.remedy)
        assertNull("no file name was asked about", diagnosis.holdMessage)
    }

    @Test
    fun `no native answer or an unreadable answer takes the frozen floor, and a missing field takes the floor's field`() {
        val reason = "ambiguous-reregister-projection"
        val floor = AttachFailureDiagnosis.frozen(reason, "plan.md")
        assertTrue(floor.needsOperatorSurface)
        assertEquals(
            floor,
            AttachFailureDiagnosis.diagnose(reason, "plan.md", derivation = EditorDerivation.Unavailable),
        )
        assertEquals(floor, AttachFailureDiagnosis.diagnose(reason, "plan.md") { _, _ -> "not json" })
        assertEquals(floor, AttachFailureDiagnosis.diagnose(reason, "plan.md") { _, _ -> "[]" })
        // An older library that answers fewer fields: each missing one comes from the floor.
        assertEquals(floor, AttachFailureDiagnosis.diagnose(reason, "plan.md") { _, _ -> "{}" })
        val partial = AttachFailureDiagnosis.diagnose(reason, "plan.md") { _, _ -> """{"status_token":"held"}""" }
        assertEquals(floor.copy(statusToken = "held"), partial)
    }

    /**
     * Seen on agent-doc-bugs.md, 2026-10-28 12:57:53. A replacement registration
     * held `ambiguous-retained-projection` while the existing replica stayed attached
     * (`live_editors=1`), and the operator was told to close the tab and run `agent-doc repair`.
     */
    @Test
    fun `a hold beside a live replica sends the fact and executes a log-only answer`() {
        var sent: String? = null
        val diagnosis =
            AttachFailureDiagnosis.diagnose("ambiguous-retained-projection", "plan.md", liveReplicaAttached = true) {
                _, input ->
                sent = input
                """{"remedy":null,"status_token":"ambiguous-retained-projection",""" +
                    """"needs_operator_surface":true,"needs_liveness_republish":false,""" +
                    """"retry_disposition":"await_operator_evidence","hold_message":"kept",""" +
                    """"hold_notification":"log_only"}"""
            }
        assertEquals(
            JsonParser.parseString(
                """{"reason":"ambiguous-retained-projection","file_name":"plan.md","live_replica_attached":true}""",
            ),
            JsonParser.parseString(sent),
        )
        assertNull(diagnosis.remedy)
        assertEquals(AttachFailureDiagnosis.HoldNotification.LogOnly, diagnosis.holdNotification)
        assertEquals(AttachFailureDiagnosis.RetryDisposition.AwaitOperatorEvidence, diagnosis.retryDisposition)

        // Without an attached replica nothing new is sent, and an older library that does not
        // answer the field keeps the operator notification.
        AttachFailureDiagnosis.diagnose("ambiguous-retained-projection", "plan.md") { _, input ->
            sent = input
            "{}"
        }.also { assertEquals(AttachFailureDiagnosis.HoldNotification.Error, it.holdNotification) }
        assertFalse(sent!!.contains("live_replica_attached"))
        // A floor answer beside a live replica never notifies, so it cannot prescribe detaching it.
        assertEquals(
            AttachFailureDiagnosis.HoldNotification.LogOnly,
            AttachFailureDiagnosis.diagnose(
                "ambiguous-retained-projection",
                "plan.md",
                liveReplicaAttached = true,
                derivation = EditorDerivation.Unavailable,
            ).holdNotification,
        )
    }

    @Test
    fun `recordRegisterFailure checks the live replica before diagnosing and logs a log-only hold`() {
        val manager = source("CrdtReplicaManager.kt")
        val failure =
            manager.substringAfter("    private fun recordRegisterFailure(")
                .substringBefore("    /**")
        val live = failure.indexOf("val liveReplicaAttached = forwarders[filePath]?.attached == true")
        val diagnose = failure.indexOf("AttachFailureDiagnosis.diagnose(reason, File(filePath).name, liveReplicaAttached)")
        assertTrue(live in 0 until diagnose)
        assertTrue(failure.contains("surfacedAttachFailureKeyUtil(reason, liveReplicaAttached)"))

        val hold =
            manager.substringAfter("    private fun recordRetainedProjectionHold(")
                .substringBefore("    private fun clearRegisterFailure(")
        val event = hold.indexOf("lib.agent_doc_record_editor_surface_event(")
        val logOnly = hold.indexOf("AttachFailureDiagnosis.HoldNotification.LogOnly")
        val notify = hold.indexOf("TerminalUtil.notifyError(project, message)")
        // The ops-log event is still recorded; only the notification is skipped.
        assertTrue(event in 0 until logOnly)
        assertTrue(logOnly < notify)
        assertTrue(hold.substring(logOnly, notify).contains("return"))
    }

    @Test
    fun `a hold logged beside a live replica does not consume the notification owed without one`() {
        val reason = "ambiguous-retained-projection"
        val logged = surfacedAttachFailureKeyUtil(reason, liveReplicaAttached = true)
        assertEquals(reason, surfacedAttachFailureKeyUtil(reason, liveReplicaAttached = false))
        assertTrue(
            shouldSurfaceRegisterFailureUtil(
                failureCount = 1,
                needsOperatorSurface = true,
                surfacedReason = logged,
                reason = surfacedAttachFailureKeyUtil(reason, liveReplicaAttached = false),
            ),
        )
    }

    @Test
    fun `the hold message is formatted only for a file name`() {
        assertNull(AttachFailureDiagnosis.frozen("ambiguous-retained-projection", null).holdMessage)
        assertEquals(
            AttachFailureDiagnosis.frozenHoldMessage("plan.md", "ambiguous-retained-projection"),
            AttachFailureDiagnosis.frozen("ambiguous-retained-projection", "plan.md").holdMessage,
        )
    }

    @Test
    fun `an unchanged operator evidence hold surfaces only once after retry state resets`() {
        assertTrue(
            shouldSurfaceRegisterFailureUtil(
                failureCount = 1,
                needsOperatorSurface = true,
                surfacedReason = null,
                reason = "ambiguous-retained-projection",
            ),
        )
        assertFalse(
            shouldSurfaceRegisterFailureUtil(
                failureCount = 1,
                needsOperatorSurface = true,
                surfacedReason = "ambiguous-retained-projection",
                reason = "ambiguous-retained-projection",
            ),
        )
        assertTrue(
            shouldSurfaceRegisterFailureUtil(
                failureCount = 1,
                needsOperatorSurface = true,
                surfacedReason = "retained-reseed-missing-settled-shadow",
                reason = "ambiguous-retained-projection",
            ),
        )
        assertFalse(
            shouldSurfaceRegisterFailureUtil(
                failureCount = 2,
                needsOperatorSurface = true,
                surfacedReason = null,
                reason = "ambiguous-retained-projection",
            ),
        )
        assertFalse(
            shouldSurfaceRegisterFailureUtil(
                failureCount = 1,
                needsOperatorSurface = false,
                surfacedReason = null,
                reason = "controller-register",
            ),
        )
    }

    @Test
    fun `recordRegisterFailure asks the derivation once and executes the answer in the old order`() {
        val manager = source("CrdtReplicaManager.kt")
        val failure =
            manager.substringAfter("    private fun recordRegisterFailure(")
                .substringBefore("    private fun clearRegisterFailure(")
        val retired = failure.indexOf("if (PluginGeneration.retired) return false")
        val diagnose = failure.indexOf("AttachFailureDiagnosis.diagnose(reason, File(filePath).name, liveReplicaAttached)")
        val hold = failure.indexOf("recordRetainedProjectionHold(filePath, reason, diagnosis)")
        val republish = failure.indexOf("ReliableSyncLivenessListener.republishDocument(project, filePath)")
        assertTrue(retired in 0 until diagnose)
        assertTrue(diagnose < hold && hold < republish)
        assertTrue(failure.contains("diagnosis.needsOperatorSurface"))
        assertTrue(failure.contains("surfacedAttachFailureReasons.compute(filePath)"))
        assertTrue(failure.contains("shouldSurfaceRegisterFailureUtil("))
        assertTrue(failure.contains("diagnosis.needsLivenessRepublish"))
        assertTrue(failure.contains("diagnosis.retryDisposition"))
        assertTrue(failure.contains("retry=await_operator_evidence"))
        assertTrue(failure.contains("registerRetryTasks.remove(filePath)?.cancel(false)"))
        assertTrue(failure.contains("\"attach_refused_\${diagnosis.statusToken}\""))
        for (rule in listOf(
            "retainedProjectionHoldNeedsOperatorSurfaceUtil(",
            "registerFailureNeedsLivenessRepublishUtil(",
            "attachFailureStatusToken(",
            "attachFailureRemedy(",
            "retainedProjectionHoldMessageUtil(",
        )) {
            assertFalse("the rule $rule is asked through the derivation", failure.contains(rule))
        }
    }

    @Test
    fun `resolved or terminal registration episodes rearm operator hold surfacing`() {
        val manager = source("CrdtReplicaManager.kt")
        val terminal =
            manager.substringAfter("    private fun recordTerminalRegisterRefusal(")
                .substringBefore("    /** Persist and surface")
        val clear =
            manager.substringAfter("    private fun clearRegisterFailure(")
                .substringBefore("    /**")
        assertTrue(terminal.contains("surfacedAttachFailureReasons.remove(filePath)"))
        assertTrue(clear.contains("surfacedAttachFailureReasons.remove(filePath)"))
    }

    @Test
    fun `operator evidence holds do not arm the timed register retry`() {
        val manager = source("CrdtReplicaManager.kt")
        val failure =
            manager.substringAfter("    private fun recordRegisterFailure(")
                .substringBefore("    /**")
        val republish = failure.indexOf("diagnosis.needsLivenessRepublish")
        val pause = failure.indexOf("RetryDisposition.AwaitOperatorEvidence")
        val retry = failure.lastIndexOf("scheduleRegisterRetry(")
        assertTrue(pause >= 0)
        assertTrue(republish < pause && pause < retry)
        assertTrue(failure.substring(pause, retry).contains("return false"))

        val gate =
            manager.substringAfter("internal fun retainedProjectionHoldAllowsRefreshUtil(")
                .substringBefore("internal fun shouldApplyRemoteCrdtUpdateUtil(")
        assertTrue(gate.contains("explicitRecoveryRequest"))
        assertFalse(gate.contains("registrationAttemptDue"))
    }

    @Test
    fun `no register failure is recorded on the EDT`() {
        val manager = source("CrdtReplicaManager.kt")
        // The register retry timer reads the editor on the EDT and hands a failure to the worker.
        val retry =
            manager.substringAfter("    private fun scheduleRegisterRetry(")
                .substringBefore("    /**")
        val edtTurn = retry.substringAfter("runOnEdtNonBlocking {")
        assertFalse(edtTurn.contains("recordRegisterFailure("))
        assertTrue(edtTurn.contains("recordRegisterFailureOffEdt(filePath, \"editor-file-unavailable\")"))
        assertTrue(edtTurn.contains("recordRegisterFailureOffEdt(filePath, \"editor-document-unavailable\")"))
        val offEdt =
            manager.substringAfter("    private fun recordRegisterFailureOffEdt(")
                .substringBefore("    /** Record a text-terminal classification")
        assertTrue(offEdt.contains("executor.execute { recordRegisterFailure(filePath, reason) }"))

        // The native-reload target collection only reads editor state on the EDT; the failure is
        // recorded after it, before any registration is awaited.
        val restart =
            manager.substringAfter("internal fun restartAfterNativeReload(")
                .substringBefore("fun requestRemoteDrain(")
        val collection =
            restart.substringAfter("val collectTargets = {")
                .substringBefore("if (SwingUtilities.isEventDispatchThread())")
        assertFalse(collection.contains("recordRegisterFailure"))
        assertTrue(collection.contains("unavailableTargets.add(manager to filePath)"))
        val collected = restart.indexOf("ApplicationManager.getApplication().invokeAndWait(collectTargets)")
        val recorded = restart.indexOf("manager.recordRegisterFailureOffEdt(filePath, \"native-reload-document-unavailable\")")
        val awaited = restart.indexOf("manager.ensureOpenDocumentReplica(")
        assertTrue(collected in 0 until recorded)
        assertTrue(recorded < awaited)
    }

    @Test
    fun `Compact Exchange diagnoses and formats its refusal on the worker`() {
        val action = source("CompactExchangeAction.kt")
        val worker =
            action.substringAfter("executeOnPooledThread {")
                .substringBefore("GenerationWork.invokeLater {")
        assertTrue(worker.contains("AttachFailureDiagnosis.diagnose(reason)"))
        assertTrue(worker.contains("recordAttachRefusal(project.basePath, file.path, reason, diagnosis.statusToken)"))
        assertTrue(worker.contains("attachRefusalMessage(attachFailureReason, attachFailureDiagnosis?.remedy)"))
        val edt = action.substringAfter("GenerationWork.invokeLater {", "")
            .substringAfter("GenerationWork.invokeLater {")
            .substringBefore("override fun update(")
        assertTrue(edt.contains("TerminalUtil.notifyError(project, refusalMessage)"))
        assertFalse(edt.contains("attachFailureRemedy("))
        assertFalse(edt.contains("AttachFailureDiagnosis"))
        assertFalse(action.contains("attachFailureStatusToken("))
    }
}
