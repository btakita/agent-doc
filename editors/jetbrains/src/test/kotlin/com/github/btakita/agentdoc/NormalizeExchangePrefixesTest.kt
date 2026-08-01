package com.github.btakita.agentdoc

import java.nio.file.Files
import java.nio.file.Paths
import org.junit.Assert.*
import org.junit.Test

class NormalizeExchangePrefixesTest {

    @Test
    fun `adds prefix to matching line`() {
        val doc = """
<!-- agent:exchange patch=append -->
Some prompt text.
<!-- /agent:exchange -->
""".trimStart()

        val result = normalizeExchangePrefixesUtil(doc, listOf("Some prompt text."))
        assertTrue(result.contains("❯ Some prompt text.\n"))
    }

    @Test
    fun `is idempotent when line already has prefix`() {
        val doc = """
<!-- agent:exchange patch=append -->
❯ Some prompt text.
<!-- /agent:exchange -->
""".trimStart()

        val result = normalizeExchangePrefixesUtil(doc, listOf("Some prompt text."))
        // Should not double-prefix
        assertFalse(result.contains("❯ ❯ "))
        assertTrue(result.contains("❯ Some prompt text.\n"))
    }

    @Test
    fun `matches when target line has trailing whitespace that buffer stripped`() {
        // Binary sends line with trailing space; editor buffer has stripped version.
        val doc = """
<!-- agent:exchange patch=append -->
deterministic software.
<!-- /agent:exchange -->
""".trimStart()

        // The binary's normalize_prefix_lines contains the line WITH trailing space
        val result = normalizeExchangePrefixesUtil(doc, listOf("deterministic software. "))
        assertTrue("Should prefix despite trailing whitespace mismatch", result.contains("❯ deterministic software.\n"))
    }

    @Test
    fun `matches when buffer line has trailing whitespace`() {
        // Buffer line has trailing space; binary sends clean version.
        val doc = "<!-- agent:exchange patch=append -->\ndeterministic software.  \n<!-- /agent:exchange -->\n"

        val result = normalizeExchangePrefixesUtil(doc, listOf("deterministic software."))
        assertTrue("Should prefix buffer line with trailing whitespace", result.contains("❯ deterministic software.  \n"))
    }

    @Test
    fun `does not prefix line in agent region after boundary`() {
        val doc = """
<!-- agent:exchange patch=append -->
User prompt here.
<!-- agent:boundary:abc12345 -->
### Re: topic — opus-4-6

Agent response referencing: User prompt here.
<!-- /agent:exchange -->
""".trimStart()

        val result = normalizeExchangePrefixesUtil(doc, listOf("User prompt here."))
        // User region line gets prefix
        assertTrue(result.contains("❯ User prompt here.\n<!-- agent:boundary:abc12345 -->"))
        // Agent region line must NOT get prefix — count occurrences of the prefix
        val prefixCount = result.split("❯ User prompt here.").size - 1
        assertEquals("Only one prefix should be added", 1, prefixCount)
    }

    @Test
    fun `returns document unchanged when lines list is empty`() {
        val doc = """
<!-- agent:exchange patch=append -->
Some text.
<!-- /agent:exchange -->
""".trimStart()

        val result = normalizeExchangePrefixesUtil(doc, emptyList())
        assertEquals(doc, result)
    }

    @Test
    fun `returns document unchanged when no exchange component exists`() {
        val doc = "Just plain text without any components.\n"
        val result = normalizeExchangePrefixesUtil(doc, listOf("Just plain text without any components."))
        assertEquals(doc, result)
    }

    @Test
    fun `append patch dedupe detects replayed exchange response despite head marker`() {
        val doc = """
<!-- agent:exchange patch=append -->
### Re: Duplicate — gpt-5 (HEAD)

Already applied.
<!-- agent:boundary:abc12345 -->
<!-- /agent:exchange -->
""".trimStart()

        assertTrue(
            appendPatchAlreadyPresentUtil(
                doc,
                "exchange",
                "### Re: Duplicate — gpt-5\n\nAlready applied.\n"
            )
        )
    }

    @Test
    fun `append patch dedupe ignores boundary markers`() {
        val doc = """
<!-- agent:exchange patch=append -->
### Re: Duplicate — gpt-5

Already applied.
<!-- agent:boundary:abc12345 -->
<!-- /agent:exchange -->
""".trimStart()

        assertTrue(
            appendPatchAlreadyPresentUtil(
                doc,
                "exchange",
                "### Re: Duplicate — gpt-5\n\nAlready applied.\n<!-- agent:boundary:other -->\n"
            )
        )
    }

    @Test
    fun `patch replay dedupe detects response already present on disk with head marker`() {
        val disk = """
<!-- agent:exchange patch=append -->
### Re: #gsqlwrite — gpt-5 (HEAD)

Committed response.
<!-- agent:boundary:abc12345 -->
<!-- /agent:exchange -->
""".trimStart()
        val patch = IpcPatch(
            file = "/repo/tasks/software/tsift.md",
            patches = listOf(ComponentPatch("exchange", "### Re: #gsqlwrite — gpt-5\n\nCommitted response.\n")),
            unmatched = "",
            frontmatter = null,
            fullContent = null,
        )

        assertTrue(patchReplayAlreadyPresentUtil(patch, listOf(disk)))
    }

    @Test
    fun `patch replay dedupe requires all response payloads to be present`() {
        val disk = """
<!-- agent:exchange patch=append -->
### Re: first — gpt-5

One.
<!-- /agent:exchange -->
""".trimStart()
        val patch = IpcPatch(
            file = "/repo/doc.md",
            patches = listOf(
                ComponentPatch("exchange", "### Re: first — gpt-5\n\nOne.\n"),
                ComponentPatch("exchange", "### Re: second — gpt-5\n\nTwo.\n"),
            ),
            unmatched = "",
            frontmatter = null,
            fullContent = null,
        )

        assertFalse(patchReplayAlreadyPresentUtil(patch, listOf(disk)))
    }

    @Test
    fun `patch replay dedupe can use safe committed proof`() {
        val patch = IpcPatch(
            file = "/repo/doc.md",
            patches = listOf(ComponentPatch("exchange", "### Re: committed — gpt-5\n\nDone.\n")),
            unmatched = "",
            frontmatter = null,
            fullContent = null,
        )

        assertTrue(patchReplayAlreadyPresentUtil(patch, emptyList()) { payload ->
            payload.contains("committed")
        })
    }

    @Test
    fun `editor apply proof rejects content or generation drift`() {
        val proof = EditorApplyProof("before", 7L)

        assertTrue(editorApplyProofStillCurrentUtil(proof, "before", 7L))
        assertFalse(editorApplyProofStillCurrentUtil(proof, "after", 7L))
        assertFalse(editorApplyProofStillCurrentUtil(proof, "before", 8L))
    }

    @Test
    fun `minimal document edit narrows replacement to changed span`() {
        assertNull(minimalDocumentEditUtil("same", "same"))
        assertEquals(
            MinimalDocumentEdit(start = 6, end = 10, replacement = "BETA"),
            minimalDocumentEditUtil("alpha beta gamma", "alpha BETA gamma"),
        )
        assertEquals(
            MinimalDocumentEdit(start = 6, end = 6, replacement = "beta "),
            minimalDocumentEditUtil("alpha gamma", "alpha beta gamma"),
        )
        assertEquals(
            MinimalDocumentEdit(start = 6, end = 11, replacement = ""),
            minimalDocumentEditUtil("alpha beta gamma", "alpha gamma"),
        )
    }

    @Test
    fun `full content source buffer proof rejects live editor drift`() {
        val before = "before"
        val hash = sha256HexUtf8(before)
        val len = before.toByteArray(Charsets.UTF_8).size

        assertTrue(fullContentExpectedBufferMatchesUtil(before, hash, len))
        assertFalse(fullContentExpectedBufferMatchesUtil("before\nlive prompt", hash, len))
        assertFalse(fullContentExpectedBufferMatchesUtil(before, hash, len + 1))
    }

    @Test
    fun `component patch op override accepts known modes`() {
        assertEquals("replace", componentPatchModeOverrideUtil(" REPLACE "))
        assertEquals("append", componentPatchModeOverrideUtil("append"))
        assertEquals("prepend", componentPatchModeOverrideUtil("Prepend"))
        assertNull(componentPatchModeOverrideUtil(null))
        assertNull(componentPatchModeOverrideUtil("delete"))
    }

    @Test
    fun `component patch op override is wired into document apply path`() {
        val sourcePath = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt"),
        ).first { Files.exists(it) }
        val source = Files.readString(sourcePath)

        assertTrue(source.contains("componentPatchModeOverrideUtil(modeOverride) ?: extractComponentMode(doc, component)"))
        assertTrue(source.contains("applyComponentPatchNative(result, p.component, p.content, effectiveBoundaryId, p.op)"))
        assertTrue(source.contains("VFS whole-buffer patch apply is disabled"))
    }

    @Test
    fun `plugin rejects full content and reconnect repair paths`() {
        val sourcePath = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt"),
        ).first { Files.exists(it) }
        val source = Files.readString(sourcePath)
        assertTrue(source.contains("full-content IPC is disabled"))
        assertFalse(source.contains("EditorIntent.SaveDocument"))
        assertFalse(source.contains("save_document"))
        assertFalse(source.contains("document.setText(patch.fullContent)"))
        assertFalse(source.contains("setBinaryContent(patch.fullContent"))
        assertFalse(source.contains("setBinaryContent("))
        assertFalse(source.contains("saveDocument("))
        assertFalse(source.contains("applyReconnectReread("))
        assertFalse(source.contains("Agent Doc Reconnect Reread"))
        assertFalse(source.contains("re-read disk/HEAD into stale buffer"))
        assertFalse(source.contains("agent_doc_reconnect_buffer_decision"))
        assertFalse(source.contains("reread_disk repair is disabled"))
    }

    @Test
    fun `cycle 1779845677327 full content fixture is rejected before visible writes`() {
        val json = """
            {
              "file": "/repo/tasks/agent-doc/agent-doc-bugs2.md",
              "patches": [],
              "unmatched": "",
              "patch_id": "cycle-1779845677327",
              "fullContent": "<!-- agent:exchange -->\n❯ do [#liveipcrace]\n<!-- /agent:exchange -->\n\n###\n\n<!--\nThe duplicate content corrupting document and duplicate prompt issues happened yet again.\n#spec-test-build-install-commit-push\n---\ndispatch #spec-test-build-install-commit-push\n-->\n"
            }
        """.trimIndent()
        val patch = requireNotNull(parsePatchJson(json))
        val sourcePath = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt"),
        ).first { Files.exists(it) }
        val source = Files.readString(sourcePath)
        val socketFullContentGuard = source.indexOf("if (!patch.fullContent.isNullOrEmpty())")
        val socketApplyStart = source.indexOf(
            "ApplicationManager.getApplication().invokeAndWait",
            if (socketFullContentGuard >= 0) socketFullContentGuard else 0,
        )
        val fileFullContentGuard = source.indexOf("[patch-watcher] full-content IPC is disabled; deleting stale/foreign patch file")
        val fileApplyStart = source.indexOf(
            "ApplicationManager.getApplication().invokeLater",
            if (fileFullContentGuard >= 0) fileFullContentGuard else 0,
        )
        val fullContent = requireNotNull(patch.fullContent)

        assertEquals("cycle-1779845677327", patch.patchId)
        assertTrue(fullContent.contains("#spec-test-build-install-commit-push"))
        assertTrue(fullContent.contains("dispatch #spec-test-build-install-commit-push"))
        assertTrue(socketFullContentGuard >= 0 && socketApplyStart > socketFullContentGuard)
        assertTrue(fileFullContentGuard < 0)
        assertFalse(source.contains("awaitIdleBeforeDocumentMutation("))
        assertFalse(source.contains("document.setText(patch.fullContent)"))
        assertFalse(source.contains("setBinaryContent(patch.fullContent"))
        assertFalse(source.contains("setBinaryContent("))
    }

    @Test
    fun `plugin rejects file cache conflict writes before mutating document`() {
        val sourcePath = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt"),
        ).first { Files.exists(it) }
        val source = Files.readString(sourcePath)

        assertTrue(source.contains("hasPendingMemoryDiskConflict(targetFile)"))
        assertTrue(source.contains("lastApplyBlockedForFileCacheConflict = true"))
        assertTrue(source.contains("ui_outcome=real_component_conflict"))
        assertTrue(source.contains("File Cache Conflict pending for") && source.contains("rejecting patch without mutating document"))
        assertTrue(source.contains("recordFileCacheConflictOps("))
        assertTrue(source.contains("\"editor_ipc_convergence\""))
        assertTrue(source.contains("\"write_finalize_ipc\""))
        assertFalse(source.contains("memoryDiskConflictDeferredPatchIds"))
        assertFalse(source.contains("markPatchDeferredForMemoryDiskConflict"))
        assertFalse(source.contains("wasPatchDeferredForMemoryDiskConflict"))
        assertFalse(source.contains("clearPatchDeferredForMemoryDiskConflict"))
        assertFalse(source.contains("memoryDiskConflictCancelLikelyUtil"))
        assertFalse(source.contains("File Cache Conflict kept memory changes"))
        assertFalse(source.contains("File Cache Conflict pending\")"))
    }

    @Test
    fun `file cache conflict ops marker identifies action command and surface`() {
        val line = buildFileCacheConflictOpsLogLine(
            timestamp = "2026-06-24T12:34:56Z",
            relativePath = "tasks/agent-doc/agent-doc-bugs2.md",
            outcome = "pending",
            surface = "editor_ipc_convergence",
            action = "apply_patch",
            agentCommand = "write_finalize_ipc",
            proof = "conflict_key=patch-1 patch_id=patch-1 document_unsaved=true document_stamp=10 file_stamp=20",
        )

        assertEquals(
            "[2026-06-24T12:34:56Z] file_cache_conflict_detected source=jetbrains outcome=pending " +
                "surface=editor_ipc_convergence action=apply_patch agent_command=write_finalize_ipc " +
                "file=tasks/agent-doc/agent-doc-bugs2.md conflict_key=patch-1 patch_id=patch-1 " +
                "document_unsaved=true document_stamp=10 file_stamp=20 doc=agent-doc-bugs2 #cyh0",
            line,
        )
    }

    @Test
    fun `editor surface ops route through shared native ffi`() {
        val patchWatcherPath = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt"),
        ).first { Files.exists(it) }
        val nativeLibPath = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/NativeLib.kt"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/NativeLib.kt"),
        ).first { Files.exists(it) }
        val watcher = Files.readString(patchWatcherPath)
        val nativeLib = Files.readString(nativeLibPath)

        assertTrue(nativeLib.contains("fun agent_doc_record_editor_surface_event("))
        assertTrue(watcher.contains("lib.agent_doc_record_editor_surface_event("))
        assertTrue(watcher.contains("root, \"jetbrains\", filePath"))
        assertTrue(watcher.contains("root, \"jetbrains\", \".\""))
        assertFalse(watcher.contains("buildEditorSurfaceOpsLogLine("))
    }

    @Test
    fun `file cache conflict path refreshes visual highlighters`() {
        val patchWatcherPath = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt"),
        ).first { Files.exists(it) }
        val visualHighlighterPath = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/VisualHighlighterManager.kt"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/VisualHighlighterManager.kt"),
        ).first { Files.exists(it) }
        val patchWatcher = Files.readString(patchWatcherPath)
        val visualHighlighter = Files.readString(visualHighlighterPath)

        val conflict = patchWatcher.indexOf("hasPendingMemoryDiskConflict(targetFile)")
        val conflictRefresh = patchWatcher.indexOf("refreshVisualHighlightersAfterFileCacheConflict(targetFile, \"blocked\")")

        assertTrue(visualHighlighter.contains("fun refreshFile(file: VirtualFile)"))
        assertTrue(conflict >= 0 && conflictRefresh > conflict)
        assertTrue(patchWatcher.contains("VisualHighlighterManager.getInstance(project).refreshFile(targetFile)"))
    }

    @Test
    fun `minimal document edit only reports success after target text is present`() {
        val patchWatcherPath = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt"),
        ).first { Files.exists(it) }
        val patchWatcher = Files.readString(patchWatcherPath)
        val helperStart = patchWatcher.indexOf("internal fun applyMinimalDocumentEditUtil(")
        val helperEnd = patchWatcher.indexOf("/**", helperStart)
        val helper = patchWatcher.substring(helperStart, helperEnd)

        val replaceIdx = helper.indexOf("document.replaceString(edit.start, edit.end, edit.replacement)")
        val postApplyProofIdx = helper.indexOf("return document.text == after")

        assertTrue(replaceIdx >= 0)
        assertTrue(
            "minimal edit helper must verify the post-apply buffer before recording receipt",
            postApplyProofIdx > replaceIdx,
        )
    }

    @Test
    fun `cold opened markdown files refresh visual highlighters`() {
        val visualHighlighterPath = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/VisualHighlighterManager.kt"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/VisualHighlighterManager.kt"),
        ).first { Files.exists(it) }
        val visualHighlighter = Files.readString(visualHighlighterPath)

        assertTrue(visualHighlighter.contains("FileEditorManagerListener.FILE_EDITOR_MANAGER"))
        assertTrue(visualHighlighter.contains("override fun fileOpened(source: FileEditorManager, file: VirtualFile)"))
        assertTrue(visualHighlighter.contains("override fun selectionChanged(event: FileEditorManagerEvent)"))
        assertTrue(visualHighlighter.contains("private fun refreshMarkdownFileAfterEditorEvent(file: VirtualFile)"))
        assertTrue(visualHighlighter.contains("ApplicationManager.getApplication().invokeLater"))
        assertTrue(visualHighlighter.contains("refreshFile(file)"))
    }

    @Test
    fun `component body inherits editor foreground`() {
        val visualHighlighterPath = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/VisualHighlighterManager.kt"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/VisualHighlighterManager.kt"),
        ).first { Files.exists(it) }
        val visualHighlighter = Files.readString(visualHighlighterPath)
        val componentBodyBranch = visualHighlighter
            .substringAfter("\"component_body\" ->")
            .substringBefore("\"scratch_comment_body\"")

        assertTrue(componentBodyBranch.contains("baseAttrs(null).apply"))
        assertTrue(componentBodyBranch.contains("mutedBackground(editor, accent)"))
        assertFalse(componentBodyBranch.contains("foregroundColor ="))
        assertFalse(componentBodyBranch.contains("DefaultLanguageHighlighterColors.METADATA)?.foregroundColor,"))
    }

    @Test
    fun `file patch success requires content projection before deleting patch file`() {
        val patchWatcherPath = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt"),
        ).first { Files.exists(it) }
        val patchWatcher = Files.readString(patchWatcherPath)

        assertTrue(patchWatcher.contains("private fun writeEditorContentProjection(patchId: String?, content: String, filePath: String? = null): Boolean"))
        assertTrue(patchWatcher.contains("FFI unavailable, cannot write content projection"))
        assertTrue(patchWatcher.contains("if (!writeEditorContentProjection(patch.patchId, document.text, patch.file))"))
        assertTrue(patchWatcher.contains("if (!writeEditorContentProjection(patch.patchId, content, patch.file))"))
        assertTrue(patchWatcher.contains("applyMinimalDocumentEditUtil(document, content, result)"))
        assertFalse(patchWatcher.contains("document.setText(result)"))
        assertFalse(patchWatcher.contains("setBinaryContent("))

        assertTrue(patchWatcher.contains("private fun handleSocketMessageV2"))
        assertTrue(patchWatcher.contains("StateProjectionBridge.recordEditorPatchApplied"))
        assertFalse(patchWatcher.contains("processPatchFile"))
        assertFalse(patchWatcher.contains(".agent-doc/patches"))
    }

    @Test
    fun `already applied socket dedup publishes content projection before receipt`() {
        val patchWatcherPath = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt"),
        ).first { Files.exists(it) }
        val patchWatcher = Files.readString(patchWatcherPath)

        assertTrue(patchWatcher.contains("private fun writeAlreadyAppliedContentProjection(patch: IpcPatch, source: String): Boolean"))
        assertTrue(patchWatcher.contains("currentContentForProjection(patch.file)"))
        assertTrue(patchWatcher.contains("agent_doc_editor_content_applied_for_editor_v1"))
        assertTrue(patchWatcher.contains("editor_content_applied_for_editor_v1"))

        val socketPatch = patchWatcher
            .substringAfter("\"patch\" -> {")
            .substringBefore("\"reposition\" -> {")
        val socketPrecheck = socketPatch.indexOf("isAlreadyApplied(patch.patchId)")
        val socketProjection = socketPatch.indexOf("writeAlreadyAppliedContentProjection(patch, \"socket_precheck\")")
        val socketAlreadyApplied = socketPatch.indexOf("APPLY_ALREADY_APPLIED", socketProjection)
        assertTrue("socket dedup must publish content projection before already_applied", socketPrecheck >= 0 && socketPrecheck < socketProjection && socketProjection < socketAlreadyApplied)

        assertFalse(patchWatcher.contains("file_precheck"))
        assertFalse(patchWatcher.contains("patchFile.delete()"))
    }

    @Test
    fun `socket patch requires explicit Lazily editor targeting before receipt`() {
        val patchWatcherPath = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt"),
        ).first { Files.exists(it) }
        val patchWatcher = Files.readString(patchWatcherPath)
        val socketBranch = patchWatcher
            .substringAfter("\"patch\" -> {")
            .substringBefore("when {")

        val target = socketBranch.indexOf("if (!patch.targetsThisEditor())")
        val queued = socketBranch.indexOf("StateProjectionBridge.recordEditorPatchQueued")
        val apply = socketBranch.indexOf("applyPatch(patch)")
        val ack = socketBranch.indexOf("StateProjectionBridge.recordEditorPatchApplied")

        assertTrue("socket IPC must reject foreign/untargeted delivery before queueing", target >= 0 && target < queued)
        assertTrue("socket IPC must reject foreign/untargeted delivery before applying", target >= 0 && target < apply)
        assertTrue("socket IPC must reject foreign/untargeted delivery before recording receipt", target >= 0 && target < ack)
        assertFalse("plugin-owner compatibility must be absent", patchWatcher.contains("plugin_owner"))
    }

    @Test
    fun `socket reposition honors targeted editor identity before mutating`() {
        val patchWatcherPath = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt"),
        ).first { Files.exists(it) }
        val patchWatcher = Files.readString(patchWatcherPath)
        val repositionBranch = patchWatcher
            .substringAfter("\"reposition\" -> {")
            .substringBefore("\"refresh_content\" -> {")

        val file = repositionBranch.indexOf("val file = extractStringField(json, \"file\")")
        val editorId = repositionBranch.indexOf("val editorId = extractStringField(json, \"editor_id\")")
        val targetGuard = repositionBranch.indexOf("targetsThisEditorId(editorId)")
        val mutate = repositionBranch.indexOf("repositionBoundaryViaDocument(file, boundaryId, preserveHead)")

        assertTrue("socket reposition must parse the target file", file >= 0)
        assertTrue("socket reposition must parse editor_id", editorId > file)
        assertTrue("socket reposition must reject foreign editor_id before mutating", targetGuard > editorId && targetGuard < mutate)
    }

    @Test
    fun `handles line at very start of user region`() {
        // First line of the user region (no leading newline before it)
        val doc = "<!-- agent:exchange patch=append -->\nFirst line.\nSecond line.\n<!-- /agent:exchange -->\n"

        val result = normalizeExchangePrefixesUtil(doc, listOf("First line."))
        assertTrue(result.contains("❯ First line.\n"))
        // Second line should be unchanged
        assertTrue(result.contains("Second line.\n"))
        assertFalse(result.contains("❯ Second line."))
    }

    @Test
    fun `skips blank lines in target list`() {
        val doc = "<!-- agent:exchange patch=append -->\n\nSome text.\n<!-- /agent:exchange -->\n"
        // Blank lines in the target list must not cause a prefix on blank lines
        val result = normalizeExchangePrefixesUtil(doc, listOf("", "  ", "Some text."))
        assertFalse("Blank lines in doc must not gain prefix", result.contains("❯ \n"))
        assertTrue(result.contains("❯ Some text.\n"))
    }

    @Test
    fun `uses last boundary marker as user-region end`() {
        // Real-world shape: binary inserts fresh boundary at END of exchange (after user prompt).
        // A stale boundary from the prior cycle sits between the old response and the new prompt.
        // Using the LAST boundary puts the user prompt in userRegion → gets prefix.
        // If the FIRST boundary were used, the user prompt would be in agentRegion → no prefix.
        val doc = """
<!-- agent:exchange patch=append -->
### Re: earlier — opus-4-6

Response one.
<!-- agent:boundary:aaa11111 -->
New user prompt.
<!-- agent:boundary:bbb22222 -->
<!-- /agent:exchange -->
""".trimStart()

        val result = normalizeExchangePrefixesUtil(doc, listOf("New user prompt."))
        assertTrue(result.contains("❯ New user prompt.\n"))
        // Agent response before the first boundary must NOT be prefixed
        assertFalse(result.contains("❯ Response one."))
    }

    @Test
    fun `reposition before normalization repairs prompt typed after stale boundary`() {
        val doc = """
<!-- agent:exchange patch=append -->
### Re: earlier — gpt-5

Response one.
<!-- agent:boundary:aaa11111 -->
do [#sidecarfallbackstill]. spec-test-build-install-commit-push
<!-- /agent:exchange -->
""".trimStart()

        val normalizedBeforeReposition = normalizeExchangePrefixesUtil(
            doc,
            listOf("do [#sidecarfallbackstill]. spec-test-build-install-commit-push")
        )
        assertFalse(
            "Prompt after the stale boundary is not in the user region until boundary cleanup",
            normalizedBeforeReposition.contains("❯ do [#sidecarfallbackstill]")
        )

        val repositioned = repositionBoundaryToEndUtil(doc, "exchange", "bbb22222")
        assertNotNull(repositioned)
        val result = normalizeExchangePrefixesUtil(
            repositioned!!,
            listOf("do [#sidecarfallbackstill]. spec-test-build-install-commit-push")
        )

        assertTrue(result.contains("❯ do [#sidecarfallbackstill]. spec-test-build-install-commit-push\n"))
        assertTrue(result.contains("<!-- agent:boundary:bbb22222 -->"))
    }

    @Test
    fun `does not prefix assistant verification list before latest boundary`() {
        val doc = """
<!-- agent:exchange patch=append -->
### Re: previous — gpt-5

Implemented.

Verification:
- Passed focused tests:
  - `cargo test normalize_prefix`
- `cargo test` is still red on a pre-existing failure.
<!-- agent:boundary:aaa11111 -->
New user prompt.
<!-- agent:boundary:bbb22222 -->
<!-- /agent:exchange -->
""".trimStart()

        val result = normalizeExchangePrefixesUtil(
            doc,
            listOf(
                "- Passed focused tests:",
                "  - `cargo test normalize_prefix`",
                "- `cargo test` is still red on a pre-existing failure.",
                "New user prompt."
            )
        )

        assertTrue(result.contains("Verification:\n- Passed focused tests:\n  - `cargo test normalize_prefix`\n- `cargo test` is still red on a pre-existing failure."))
        assertTrue(result.contains("❯ New user prompt.\n"))
        assertFalse(result.contains("❯ - Passed focused tests:"))
        assertFalse(result.contains("❯   - `cargo test normalize_prefix`"))
        assertFalse(result.contains("❯ - `cargo test` is still red on a pre-existing failure."))
    }

    @Test
    fun `does not treat prefixed markdown response labels as fresh prompts`() {
        val doc = """
<!-- agent:exchange patch=append -->
### Re: previous — gpt-5

Implemented.

❯ **Verification:** Both redirects confirmed via `curl`.
❯ **Commit / push:**
- `abc1234` pushed.
do #next. spec-test-build-install-commit-push
<!-- agent:boundary:bbb22222 -->
<!-- /agent:exchange -->
""".trimStart()

        val result = normalizeExchangePrefixesUtil(
            doc,
            listOf(
                "**Verification:** Both redirects confirmed via `curl`.",
                "**Commit / push:**",
                "- `abc1234` pushed.",
                "do #next. spec-test-build-install-commit-push"
            )
        )

        assertTrue(result.contains("❯ **Verification:** Both redirects confirmed via `curl`.\n"))
        assertTrue(result.contains("❯ **Commit / push:**\n- `abc1234` pushed."))
        assertFalse(result.contains("❯ - `abc1234` pushed."))
        assertTrue(result.contains("❯ do #next. spec-test-build-install-commit-push\n"))
    }
}
