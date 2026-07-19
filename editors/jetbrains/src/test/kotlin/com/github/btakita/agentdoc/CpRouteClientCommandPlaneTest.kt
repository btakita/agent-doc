package com.github.btakita.agentdoc

import com.google.gson.JsonParser
import org.junit.Assert.assertEquals
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
