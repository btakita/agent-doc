package com.github.btakita.agentdoc

import com.google.gson.JsonParser
import io.github.lazily.IpcMessage
import io.github.lazily.NodeState
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #lzmsgpcp: the command-plane (`command-plane-v1`) rollout path for JetBrains
 * `Run Agent Doc`. Pins the `CommandSubmit` envelope shape and the terminal-only
 * projection resolution the controller's shadow endpoint relies on.
 */
class CpRouteClientCommandPlaneTest {
    private fun inlinePayload(submit: com.google.gson.JsonObject): com.google.gson.JsonObject {
        val bytes = submit.getAsJsonObject("payload").getAsJsonArray("Inline")
            .map { it.asInt.toByte() }
            .toByteArray()
        return JsonParser.parseString(String(bytes, Charsets.UTF_8)).asJsonObject
    }

    @Test
    fun `editorCommandSubmitRequest builds an agent-doc editor_route CommandSubmit`() {
        val request = CpRouteClient.editorCommandSubmitRequest(
            filePath = "/proj/plan.md",
            relativePath = "plan.md",
            layoutArgs = listOf("-h"),
            waitForReadySeconds = 120,
            attemptId = "attempt-7",
            routeKey = "root:plan.md:run",
            commandId = "cmd-fixed",
        )
        assertEquals("editor_command_submit", request.get("command").asString)
        assertEquals("/proj/plan.md", request.get("file").asString)

        val message = JsonParser.parseString(request.get("diagnostic_payload").asString).asJsonObject
        val submit = message.getAsJsonObject("CommandSubmit")
        assertEquals("agent-doc", submit.get("namespace").asString)
        assertEquals("editor_route", submit.get("name").asString)
        assertEquals("agent-doc.editor_route.v1", submit.get("payload_type").asString)
        assertEquals("cmd-fixed", submit.get("command_id").asString)
        assertEquals("attempt-7", submit.get("idempotency_key").asString)
        assertEquals("same_idempotency_key", submit.getAsJsonObject("policy").get("dedupe").asString)
        assertTrue(submit.get("payload_hash").asString.startsWith("sha256:"))

        // The inline payload round-trips to the editor_route payload the controller consumes.
        val payload = inlinePayload(submit)
        assertEquals("plan.md", payload.get("relative_path").asString)
        assertEquals(true, payload.get("dispatch_only").asBoolean)
        assertEquals("root:plan.md:run", payload.get("route_key").asString)
        assertEquals("attempt-7", payload.get("attempt_id").asString)
    }

    @Test
    fun `editor route payload preserves selected text and uses steering id for dedupe`() {
        val selected = "Keep this line\n  and these spaces  "
        val request = CpRouteClient.editorCommandSubmitRequest(
            filePath = "/proj/plan.md",
            relativePath = "plan.md",
            layoutArgs = emptyList(),
            waitForReadySeconds = 30,
            attemptId = "attempt-steer",
            routeKey = "root:plan.md:run",
            commandId = "cmd-steer",
            selectedText = selected,
            steeringId = "steering-exact-1",
        )

        val message = JsonParser.parseString(request.get("diagnostic_payload").asString).asJsonObject
        val submit = message.getAsJsonObject("CommandSubmit")
        val payload = inlinePayload(submit)
        assertEquals("steering-exact-1", submit.get("idempotency_key").asString)
        assertEquals(selected, payload.get("selected_text").asString)
        assertEquals("steering-exact-1", payload.get("steering_id").asString)
    }

    @Test
    fun `editorCommandSubmitRequest can target async submit endpoint`() {
        val request = CpRouteClient.editorCommandSubmitRequest(
            filePath = "/proj/plan.md",
            relativePath = "plan.md",
            layoutArgs = emptyList(),
            waitForReadySeconds = 30,
            attemptId = "attempt-async",
            routeKey = "root:plan.md:run",
            commandId = "cmd-editor-async",
            controllerCommand = "editor_command_submit_async",
        )

        assertEquals("editor_command_submit_async", request.get("command").asString)
        val message = JsonParser.parseString(request.get("diagnostic_payload").asString).asJsonObject
        assertEquals("editor_route", message.getAsJsonObject("CommandSubmit").get("name").asString)
    }

    @Test
    fun `editorCommandStatusRequest identifies the admitted command`() {
        val request = CpRouteClient.editorCommandStatusRequest("/proj/plan.md", "cmd-editor-async")

        assertEquals("editor_command_status", request.get("command").asString)
        assertEquals("/proj/plan.md", request.get("file").asString)
        val payload = JsonParser.parseString(request.get("diagnostic_payload").asString).asJsonObject
        assertEquals("cmd-editor-async", payload.get("command_id").asString)
    }

    @Test
    fun `syncTmuxLayoutCommandSubmitRequest builds sync_tmux_layout CommandSubmit`() {
        val request = CpRouteClient.syncTmuxLayoutCommandSubmitRequest(
            projectRoot = "/proj",
            columnsJson = """["/proj/tasks/one.md","/proj/tasks/two.md"]""",
            window = null,
            focus = "/proj/tasks/two.md",
            noAutostart = false,
            exactVisible = true,
            commandId = "cmd-sync",
        )
        assertEquals("editor_command_submit", request.get("command").asString)
        assertEquals("/proj/tasks/two.md", request.get("file").asString)

        val message = JsonParser.parseString(request.get("diagnostic_payload").asString).asJsonObject
        val submit = message.getAsJsonObject("CommandSubmit")
        assertEquals("agent-doc", submit.get("namespace").asString)
        assertEquals("sync_tmux_layout", submit.get("name").asString)
        assertEquals("agent-doc.sync_tmux_layout.v1", submit.get("payload_type").asString)
        assertEquals("/proj:sync", submit.get("idempotency_key").asString)
        assertEquals(true, submit.getAsJsonObject("policy").get("supersede").asBoolean)

        val payload = inlinePayload(submit)
        assertEquals("/proj", payload.get("project_root").asString)
        assertEquals("/proj/tasks/one.md", payload.getAsJsonArray("columns")[0].asString)
        assertEquals("/proj/tasks/two.md", payload.get("focus").asString)
        assertEquals(false, payload.get("no_autostart").asBoolean)
        assertEquals(true, payload.get("exact_visible").asBoolean)
        assertEquals("manual", payload.get("caller_kind").asString)
    }

    @Test
    fun `pane layout desired publication is a keyed Lazily snapshot`() {
        val request = CpRouteClient.paneLayoutDesiredStatePublishRequest(
            projectRoot = "/proj",
            columnsJson = """["/proj/tasks/one.md","/proj/tasks/two.md"]""",
            window = "agent-doc",
            focus = "/proj/tasks/two.md",
            noAutostart = false,
            exactVisible = true,
            producerId = "jetbrains-test",
            epoch = 42,
        )

        assertEquals("state_plane_publish", request.get("command").asString)
        val publication =
            JsonParser.parseString(request.get("diagnostic_payload").asString).asJsonObject
        assertEquals(
            "agent-doc/pane-layout/desired/v1",
            publication.get("channel").asString,
        )
        assertEquals("jetbrains-test", publication.get("producer_id").asString)

        val message = IpcMessage.decodeJson(publication.get("message_json").asString)
        val snapshot = (message as IpcMessage.SnapshotMessage).snapshot
        assertEquals(42, snapshot.epoch)
        val node = snapshot.nodes.single()
        assertEquals("agent-doc.pane-layout.desired.v1", node.typeTag)
        assertEquals("agent-doc/pane-layout/desired/v1", node.key?.path)
        val desired =
            JsonParser.parseString(
                String((node.state as NodeState.Payload).toByteArray(), Charsets.UTF_8),
            ).asJsonObject
        assertEquals("/proj/tasks/one.md", desired.getAsJsonArray("columns")[0].asString)
        assertEquals("/proj/tasks/two.md", desired.get("focus").asString)
        assertEquals("manual", desired.get("caller_kind").asString)
    }

    @Test
    fun `state plane subscription carries a monotonic resume cursor`() {
        val request = CpRouteClient.statePlaneSubscribeRequest(
            channel = "agent-doc/pane-layout/status/v1",
            afterVersion = 17,
            timeoutMs = 2_000,
        )

        assertEquals("state_plane_subscribe", request.get("command").asString)
        val subscription =
            JsonParser.parseString(request.get("diagnostic_payload").asString).asJsonObject
        assertEquals(
            "agent-doc/pane-layout/status/v1",
            subscription.get("channel").asString,
        )
        assertEquals(17, subscription.get("after_version").asLong)
        assertEquals(2_000, subscription.get("timeout_ms").asLong)
    }

    @Test
    fun `pane layout phase and reason tokens are typed and forward compatible`() {
        assertEquals(PaneLayoutPhase.Converged, PaneLayoutPhase.fromToken("converged"))
        assertEquals(
            PaneLayoutReasonCode.PaneOrderMismatch,
            PaneLayoutReasonCode.fromToken("pane_order_mismatch"),
        )
        assertNull(PaneLayoutPhase.fromToken("future_phase"))
        assertNull(PaneLayoutReasonCode.fromToken("future_reason"))
    }

    @Test
    fun `syncTmuxLayoutCommandSubmitRequest can target async submit endpoint`() {
        val request = CpRouteClient.syncTmuxLayoutCommandSubmitRequest(
            projectRoot = "/proj",
            columnsJson = """["/proj/tasks/one.md"]""",
            window = null,
            focus = "/proj/tasks/one.md",
            noAutostart = true,
            exactVisible = true,
            commandId = "cmd-sync-async",
            callerKind = "automatic",
            controllerCommand = "editor_command_submit_async",
        )

        assertEquals("editor_command_submit_async", request.get("command").asString)
        val message = JsonParser.parseString(request.get("diagnostic_payload").asString).asJsonObject
        val submit = message.getAsJsonObject("CommandSubmit")
        assertEquals("sync_tmux_layout", submit.get("name").asString)
        assertEquals(true, submit.getAsJsonObject("policy").get("supersede").asBoolean)
        val payload = inlinePayload(submit)
        assertEquals(true, payload.get("no_autostart").asBoolean)
        assertEquals("automatic", payload.get("caller_kind").asString)
    }

    @Test
    fun `only manual sync with autostart waits for terminal completion`() {
        assertTrue(CpRouteClient.shouldAwaitSyncCompletion("manual", noAutostart = false))
        assertFalse(CpRouteClient.shouldAwaitSyncCompletion("automatic", noAutostart = false))
        assertFalse(CpRouteClient.shouldAwaitSyncCompletion("manual", noAutostart = true))
    }

    @Test
    fun `tmuxLayoutSyncStateRequest builds read side model check request`() {
        val request = CpRouteClient.tmuxLayoutSyncStateRequest(
            columnsJson = """["/proj/tasks/one.md","/proj/tasks/two.md"]""",
            focus = "/proj/tasks/two.md",
        )

        assertEquals("tmux_layout_sync_state", request.get("command").asString)
        val payload = JsonParser.parseString(request.get("diagnostic_payload").asString).asJsonObject
        assertEquals("/proj/tasks/one.md", payload.getAsJsonArray("columns")[0].asString)
        assertEquals("/proj/tasks/two.md", payload.getAsJsonArray("columns")[1].asString)
        assertEquals("/proj/tasks/two.md", payload.get("focus").asString)
    }

    @Test
    fun `focusDocumentPaneCommandSubmitRequest builds focus_document_pane CommandSubmit`() {
        val request = CpRouteClient.focusDocumentPaneCommandSubmitRequest(
            projectRoot = "/proj",
            documentPath = "/proj/tasks/one.md",
            commandId = "cmd-focus",
        )
        assertEquals("editor_command_submit", request.get("command").asString)
        assertEquals("/proj/tasks/one.md", request.get("file").asString)

        val message = JsonParser.parseString(request.get("diagnostic_payload").asString).asJsonObject
        val submit = message.getAsJsonObject("CommandSubmit")
        assertEquals("focus_document_pane", submit.get("name").asString)
        assertEquals("agent-doc.focus_document_pane.v1", submit.get("payload_type").asString)
        assertEquals("/proj:selected-document-focus", submit.get("idempotency_key").asString)
        assertEquals(true, submit.getAsJsonObject("policy").get("supersede").asBoolean)

        val payload = inlinePayload(submit)
        assertEquals("/proj", payload.get("project_root").asString)
        assertEquals("/proj/tasks/one.md", payload.get("document_path").asString)
assertEquals(true, payload.get("no_promotion").asBoolean)
assertEquals(true, payload.get("active_window_guard").asBoolean)
assertEquals("observe_only", payload.get("missing_pane_policy").asString)
assertEquals(750L, submit.get("deadline_ms").asLong)
    }

    @Test
    fun `focusDocumentPaneCommandSubmitRequest can target async submit endpoint`() {
        val request = CpRouteClient.focusDocumentPaneCommandSubmitRequest(
            projectRoot = "/proj",
            documentPath = "/proj/tasks/one.md",
            commandId = "cmd-focus-async",
            controllerCommand = "editor_command_submit_async",
        )

        assertEquals("editor_command_submit_async", request.get("command").asString)
        val message = JsonParser.parseString(request.get("diagnostic_payload").asString).asJsonObject
        val submit = message.getAsJsonObject("CommandSubmit")
        assertEquals("focus_document_pane", submit.get("name").asString)
        assertEquals("agent-doc.focus_document_pane.v1", submit.get("payload_type").asString)
    }

    @Test
    fun `focus submissions for different tabs coalesce as one latest project intent`() {
        val first = CpRouteClient.focusDocumentPaneCommandSubmitRequest(
            projectRoot = "/proj",
            documentPath = "/proj/tasks/one.md",
            commandId = "cmd-focus-one",
        )
        val second = CpRouteClient.focusDocumentPaneCommandSubmitRequest(
            projectRoot = "/proj",
            documentPath = "/proj/tasks/two.md",
            commandId = "cmd-focus-two",
        )

        val firstSubmit = JsonParser.parseString(first.get("diagnostic_payload").asString)
            .asJsonObject.getAsJsonObject("CommandSubmit")
        val secondSubmit = JsonParser.parseString(second.get("diagnostic_payload").asString)
            .asJsonObject.getAsJsonObject("CommandSubmit")
        assertEquals(firstSubmit.get("idempotency_key"), secondSubmit.get("idempotency_key"))
        assertEquals(true, secondSubmit.getAsJsonObject("policy").get("supersede").asBoolean)
    }

    private fun projectionData(status: String, terminal: Boolean, output: String, reason: String? = null): com.google.gson.JsonObject {
        val entry = com.google.gson.JsonObject().apply {
            addProperty("command_id", "cmd-1")
            addProperty("status", status)
            addProperty("terminal", terminal)
            addProperty("generation", 0)
            if (reason == null) add("reason", com.google.gson.JsonNull.INSTANCE) else addProperty("reason", reason)
            add("terminal_receipt_id", com.google.gson.JsonNull.INSTANCE)
            add("last_event_id", com.google.gson.JsonNull.INSTANCE)
        }
        val commands = com.google.gson.JsonArray().apply { add(entry) }
        val projection = com.google.gson.JsonObject().apply {
            addProperty("generation", 0)
            add("commands", commands)
        }
        return com.google.gson.JsonObject().apply {
            addProperty("output", output)
            add("projection", projection)
        }
    }

    @Test
    fun `resolveCommandSubmitData returns output only on an applied terminal`() {
        val result = CpRouteClient.resolveCommandSubmitData(projectionData("applied", true, "routed ok"), "cmd-1")
        assertEquals(0, result.exitCode)
        assertEquals("routed ok", result.output)
    }

    @Test
    fun `resolveCommandSubmitData fails on a non-terminal projection`() {
        val result = CpRouteClient.resolveCommandSubmitData(projectionData("running", false, ""), "cmd-1")
        assertEquals(1, result.exitCode)
    }

    @Test
    fun `resolveCommandSubmitData fails on a rejected terminal`() {
        val result = CpRouteClient.resolveCommandSubmitData(
            projectionData("rejected", true, "boom", reason = "editor_route exit_code=1"),
            "cmd-1",
        )
        assertEquals(1, result.exitCode)
        assertTrue(result.output.contains("boom"))
    }

    @Test
    fun `resolveCommandSubmitTerminalData waits for a non terminal projection`() {
        val result = CpRouteClient.resolveCommandSubmitTerminalData(
            projectionData("running", false, "editor_route running"),
            "cmd-1",
        )

        assertEquals(null, result)
    }

    @Test
    fun `resolveCommandSubmitTerminalData returns an applied terminal`() {
        val result = CpRouteClient.resolveCommandSubmitTerminalData(
            projectionData("applied", true, "routed ok"),
            "cmd-1",
        )

        assertEquals(0, result?.exitCode)
        assertEquals("routed ok", result?.output)
    }

    @Test
    fun `resolveCommandSubmitTerminalData preserves typed turn steering acknowledgement`() {
        val data = projectionData("applied", true, "steered").apply {
            add("payload", com.google.gson.JsonObject().apply {
                addProperty("exit_code", 0)
                addProperty("output", "steered")
                add("steering", com.google.gson.JsonObject().apply {
                    addProperty("kind", "turn_steering_ack")
                    addProperty("steering_id", "steering-exact-1")
                    addProperty("outcome", "delivered")
                    addProperty("accepted_bytes", 34)
                })
            })
        }

        val result = CpRouteClient.resolveCommandSubmitTerminalData(data, "cmd-1")

        assertEquals("turn_steering_ack", result?.steering?.kind)
        assertEquals("steering-exact-1", result?.steering?.steeringId)
        assertEquals("delivered", result?.steering?.outcome)
        assertEquals(34, result?.steering?.acceptedBytes)
    }

    @Test
    fun `resolveCommandSubmitAcceptedData succeeds on accepted non terminal projection`() {
        val result = CpRouteClient.resolveCommandSubmitAcceptedData(
            projectionData("accepted", false, "sync_tmux_layout accepted"),
            "cmd-1",
            "sync_tmux_layout",
        )

        assertEquals(0, result.exitCode)
        assertEquals("sync_tmux_layout accepted", result.output)
    }

    @Test
    fun `resolveCommandSubmitAcceptedData fails on rejected terminal projection`() {
        val result = CpRouteClient.resolveCommandSubmitAcceptedData(
            projectionData("rejected", true, "sync failed", reason = "bad payload"),
            "cmd-1",
            "sync_tmux_layout",
        )

        assertEquals(1, result.exitCode)
        assertTrue(result.output.contains("sync failed"))
    }
}
