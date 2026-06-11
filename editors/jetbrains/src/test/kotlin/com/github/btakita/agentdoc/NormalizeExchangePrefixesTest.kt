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
    fun `full content source buffer proof rejects live editor drift`() {
        val before = "before"
        val hash = sha256HexUtf8(before)
        val len = before.toByteArray(Charsets.UTF_8).size

        assertTrue(fullContentExpectedBufferMatchesUtil(before, hash, len))
        assertFalse(fullContentExpectedBufferMatchesUtil("before\nlive prompt", hash, len))
        assertFalse(fullContentExpectedBufferMatchesUtil(before, hash, len + 1))
    }

    @Test
    fun `memory disk conflict cancel classifier only rejects deferred unsaved divergence`() {
        assertTrue(memoryDiskConflictCancelLikelyUtil(true, true, 11L, 12L))
        assertFalse(memoryDiskConflictCancelLikelyUtil(false, true, 11L, 12L))
        assertFalse(memoryDiskConflictCancelLikelyUtil(true, false, 11L, 12L))
        assertFalse(memoryDiskConflictCancelLikelyUtil(true, true, 12L, 12L))
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
    fun `component patch op override is wired into document and vfs apply paths`() {
        val sourcePath = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt"),
        ).first { Files.exists(it) }
        val source = Files.readString(sourcePath)

        assertTrue(source.contains("componentPatchModeOverrideUtil(modeOverride) ?: extractComponentMode(doc, component)"))
        assertTrue(source.contains("applyComponentPatchNative(result, p.component, p.content, caretOffset, effectiveBoundaryId, p.op)"))
        assertTrue(source.contains("applyComponentPatchNative(result, p.component, p.content, null, effectiveBoundaryId, p.op)"))
    }

    @Test
    fun `plugin rejects full content patch application paths`() {
        val sourcePath = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt"),
        ).first { Files.exists(it) }
        val source = Files.readString(sourcePath)

        assertTrue(source.contains("full-content IPC is disabled"))
        assertFalse(source.contains("document.setText(patch.fullContent)"))
        assertFalse(source.contains("setBinaryContent(patch.fullContent"))
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
        val socketTypingGuard = source.indexOf("awaitIdleBeforeDocumentMutation(patch.file, \"socket patch\")")
        val fileFullContentGuard = source.indexOf("[patch-watcher] full-content IPC is disabled; deleting stale/foreign patch file")
        val fileTypingGuard = source.indexOf("awaitIdleBeforeDocumentMutation(patch.file, \"file patch\")")
        val fullContent = requireNotNull(patch.fullContent)

        assertEquals("cycle-1779845677327", patch.patchId)
        assertTrue(fullContent.contains("#spec-test-build-install-commit-push"))
        assertTrue(fullContent.contains("dispatch #spec-test-build-install-commit-push"))
        assertTrue(socketFullContentGuard >= 0 && socketFullContentGuard < socketTypingGuard)
        assertTrue(fileFullContentGuard >= 0 && fileFullContentGuard < fileTypingGuard)
        assertFalse(source.contains("document.setText(patch.fullContent)"))
        assertFalse(source.contains("setBinaryContent(patch.fullContent"))
    }

    @Test
    fun `plugin defers file cache conflict writes before mutating document`() {
        val sourcePath = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt"),
        ).first { Files.exists(it) }
        val source = Files.readString(sourcePath)

        assertTrue(source.contains("memoryDiskConflictDeferredPatchIds"))
        assertTrue(source.contains("hasPendingMemoryDiskConflict(targetFile)"))
        assertTrue(source.contains("File Cache Conflict kept memory changes"))
        assertTrue(source.contains("schedulePatchRetry(patchFile, \"File Cache Conflict pending\")"))
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

        val pendingConflict = patchWatcher.indexOf("hasPendingMemoryDiskConflict(targetFile)")
        val pendingRefresh = patchWatcher.indexOf("refreshVisualHighlightersAfterFileCacheConflict(targetFile, \"pending\")")
        val cancelClassifier = patchWatcher.indexOf("memoryDiskConflictCancelLikelyUtil(")
        val cancelRefresh = patchWatcher.indexOf("refreshVisualHighlightersAfterFileCacheConflict(targetFile, \"cancel\")")
        val clearConflict = patchWatcher.indexOf("clearPatchDeferredForMemoryDiskConflict(patch)")
        val resolvedRefresh = patchWatcher.indexOf("refreshVisualHighlightersAfterFileCacheConflict(targetFile, \"resolved\")")
        val documentWrite = patchWatcher.indexOf("document.setText(result)")
        val appliedRefresh = patchWatcher.indexOf("refreshVisualHighlightersAfterFileCacheConflict(targetFile, \"applied\")")

        assertTrue(visualHighlighter.contains("fun refreshFile(file: VirtualFile)"))
        assertTrue(pendingConflict >= 0 && pendingRefresh > pendingConflict)
        assertTrue(cancelClassifier >= 0 && cancelRefresh > cancelClassifier)
        assertTrue(clearConflict >= 0 && resolvedRefresh > clearConflict)
        assertTrue(documentWrite >= 0 && appliedRefresh > documentWrite)
        assertTrue(patchWatcher.contains("VisualHighlighterManager.getInstance(project).refreshFile(targetFile)"))
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
