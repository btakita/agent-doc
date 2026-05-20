package com.github.btakita.agentdoc

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
    fun `editor apply proof rejects content or generation drift`() {
        val proof = EditorApplyProof("before", 7L)

        assertTrue(editorApplyProofStillCurrentUtil(proof, "before", 7L))
        assertFalse(editorApplyProofStillCurrentUtil(proof, "after", 7L))
        assertFalse(editorApplyProofStillCurrentUtil(proof, "before", 8L))
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
