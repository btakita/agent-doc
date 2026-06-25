package com.github.btakita.agentdoc

import org.junit.Assert.*
import org.junit.Test

class RepositionBoundaryTest {

    @Test
    fun `repositions boundary from middle to end`() {
        val doc = """
<!-- agent:exchange patch=append -->
### Response — opus-4-6
Some content here.
<!-- agent:boundary:abc12345 -->
do something
<!-- /agent:exchange -->
""".trimStart()

        val result = repositionBoundaryToEndUtil(doc, "exchange")
        assertNotNull(result)
        // Boundary should be after "do something", before close tag
        assertTrue(result!!.contains("do something\n<!-- agent:boundary:"))
        assertTrue(result.endsWith("<!-- /agent:exchange -->\n"))
        // Old boundary should be removed
        assertFalse(result.contains("abc12345"))
    }

    @Test
    fun `returns null when no boundary exists`() {
        val doc = """
<!-- agent:exchange patch=append -->
Some content.
<!-- /agent:exchange -->
""".trimStart()

        val result = repositionBoundaryToEndUtil(doc, "exchange")
        assertNull(result)
    }

    @Test
    fun `returns null when boundary already at end`() {
        val doc = """
<!-- agent:exchange patch=append -->
Some content.
<!-- agent:boundary:abc12345 -->
<!-- /agent:exchange -->
""".trimStart()

        val result = repositionBoundaryToEndUtil(doc, "exchange")
        assertNull(result)
    }

    @Test
    fun `collapses multiple boundaries into one at end`() {
        val doc = """
<!-- agent:exchange patch=append -->
First response.
<!-- agent:boundary:aaa11111 -->
Second response.
<!-- agent:boundary:bbb22222 -->
User input here.
<!-- /agent:exchange -->
""".trimStart()

        val result = repositionBoundaryToEndUtil(doc, "exchange")
        assertNotNull(result)
        // Should have exactly one boundary marker
        val boundaryCount = Regex("""<!-- agent:boundary:[a-z0-9]+ -->""").findAll(result!!).count()
        assertEquals(1, boundaryCount)
        // Neither old boundary should remain
        assertFalse(result.contains("aaa11111"))
        assertFalse(result.contains("bbb22222"))
        // Content should be preserved
        assertTrue(result.contains("First response."))
        assertTrue(result.contains("Second response."))
        assertTrue(result.contains("User input here."))
    }

    @Test
    fun `skips component tags inside code blocks`() {
        val doc = """
```markdown
<!-- agent:exchange patch=append -->
fake content
<!-- /agent:exchange -->
```
<!-- agent:exchange patch=append -->
Real content.
<!-- agent:boundary:abc12345 -->
New input.
<!-- /agent:exchange -->
""".trimStart()

        val result = repositionBoundaryToEndUtil(doc, "exchange")
        assertNotNull(result)
        // Should reposition in the real component, not the code block one
        assertTrue(result!!.contains("New input.\n<!-- agent:boundary:"))
        // Code block content should be unchanged
        assertTrue(result.contains("```markdown\n<!-- agent:exchange patch=append -->"))
    }

    @Test
    fun `findCodeBlockRangesUtil detects fenced blocks`() {
        val doc = """
Some text.
```rust
fn main() {}
```
More text.
```
another block
```
""".trimStart()

        val ranges = findCodeBlockRangesUtil(doc)
        assertEquals(2, ranges.size)
        // First code block should contain "fn main"
        val firstBlock = doc.substring(ranges[0].first, ranges[0].second)
        assertTrue(firstBlock.contains("fn main()"))
        // Second code block should contain "another block"
        val secondBlock = doc.substring(ranges[1].first, ranges[1].second)
        assertTrue(secondBlock.contains("another block"))
    }

    // --- #npe1 / #codefencestrip: trailing code fence at the exchange boundary ---

    @Test
    fun `npe1 trailing fence in response survives reposition`() {
        // Response ends in a fenced code block; the boundary marker lands directly
        // under the closing fence, with an unresolved follow-up prompt below.
        val doc = """
<!-- agent:exchange patch=append -->
### Re: show me the snippet
Here is the code:
```rust
fn main() {}
```
<!-- agent:boundary:abc12345 -->
follow up question?
<!-- /agent:exchange -->
""".trimStart()

        val result = repositionBoundaryToEndUtil(doc, "exchange")
        assertNotNull(result)
        // The closing fence must survive.
        assertEquals(2, Regex("```").findAll(result!!).count())
        assertTrue("code body lost:\n$result", result.contains("fn main() {}"))
        assertTrue("closing fence lost:\n$result", result.contains("```rust\nfn main() {}\n```"))
        // Boundary repositioned after the follow-up prompt.
        assertTrue(result.contains("follow up question?\n<!-- agent:boundary:"))
    }

    @Test
    fun `npe1 odd prior fence does not desync trailing fence`() {
        // A prior user prompt references a single lone fence delimiter (odd count),
        // which desyncs the naive open/close pairing so the real trailing response
        // fence is mis-paired and the boundary close-tag detection drifts.
        val doc = """
<!-- agent:exchange patch=append -->
❯ here is one fence delimiter: ```
<!-- agent:boundary:old11111 -->
### Re: answer
done. here is code:
```rust
fn main() {}
```
<!-- /agent:exchange -->
""".trimStart()

        val result = repositionBoundaryToEndUtil(doc, "exchange")
        assertNotNull(result)
        assertTrue("code body lost:\n$result", result!!.contains("fn main() {}"))
        assertTrue("closing fence lost:\n$result", result.contains("```rust\nfn main() {}\n```"))
        assertTrue("close tag corrupted:\n$result", result.endsWith("<!-- /agent:exchange -->\n"))
        assertEquals(1, Regex("""<!-- agent:boundary:[a-z0-9-]+ -->""").findAll(result).count())
    }

    @Test
    fun `npe1 tilde fence in response survives reposition`() {
        val doc = """
<!-- agent:exchange patch=append -->
### Re: q
~~~rust
fn main() {}
~~~
<!-- agent:boundary:abc12345 -->
next?
<!-- /agent:exchange -->
""".trimStart()

        val result = repositionBoundaryToEndUtil(doc, "exchange")
        assertNotNull(result)
        assertTrue("tilde fence lost:\n$result", result!!.contains("~~~rust\nfn main() {}\n~~~"))
        assertTrue(result.contains("next?\n<!-- agent:boundary:"))
    }

    @Test
    fun `npe1 findCodeBlockRanges pairs tilde fences and backtick fences independently`() {
        val doc = """
~~~
backtick ``` inside tilde block stays content
~~~
after
```rust
fn main() {}
```
""".trimStart()
        val ranges = findCodeBlockRangesUtil(doc)
        // Exactly two real blocks: the ~~~ block and the ``` block.
        assertEquals(2, ranges.size)
        assertTrue(doc.substring(ranges[0].first, ranges[0].second).contains("inside tilde block"))
        assertTrue(doc.substring(ranges[1].first, ranges[1].second).contains("fn main()"))
    }

    @Test
    fun `npe1 findCodeBlockRanges treats unterminated fence as running to end`() {
        val doc = """
prose
```rust
fn main() {}
""".trimStart()
        val ranges = findCodeBlockRangesUtil(doc)
        assertEquals(1, ranges.size)
        assertEquals(doc.length, ranges[0].second)
    }

    @Test
    fun `npe1 tilde block before backtick response desyncs pairing`() {
        // A ~~~ block in the user region is invisible to the ```-only regex, so the
        // ``` pairing should still be even. But a backtick fence *inside* the ~~~
        // block (as content) is counted by the regex, making the ``` count ODD —
        // every subsequent ``` pairs wrong and the trailing response fence's close
        // is mis-detected.
        val doc = """
<!-- agent:exchange -->
❯ example:
~~~
here is a ``` inside a tilde block
~~~
<!-- agent:boundary:old11111 -->
### Re: answer
```rust
fn main() {}
```
<!-- /agent:exchange -->
""".trimStart()

        val result = repositionBoundaryToEndUtil(doc, "exchange")
        assertNotNull(result)
        assertTrue("rust body lost:\n$result", result!!.contains("fn main() {}"))
        assertTrue("trailing fence lost:\n$result", result.contains("```rust\nfn main() {}\n```"))
        assertTrue("close tag corrupted:\n$result", result.endsWith("<!-- /agent:exchange -->\n"))
    }

    @Test
    fun `npe1 findCodeBlockRanges handles fence with info string close`() {
        // Closing fence has no info string; an odd lone fence earlier must not
        // swallow the close tag region.
        val doc = """
prose ```
<!-- agent:exchange -->
```rust
fn main() {}
```
<!-- /agent:exchange -->
""".trimStart()
        val ranges = findCodeBlockRangesUtil(doc)
        // The real fenced rust block must be detected as a code range, and the
        // close tag must NOT fall inside any range.
        val closeIdx = doc.indexOf("<!-- /agent:exchange -->")
        assertTrue(
            "close tag swallowed by a code range: ranges=$ranges",
            ranges.none { closeIdx >= it.first && closeIdx < it.second },
        )
    }

    @Test
    fun `returns null for nonexistent component`() {
        val doc = """
<!-- agent:exchange patch=append -->
Content.
<!-- agent:boundary:abc12345 -->
<!-- /agent:exchange -->
""".trimStart()

        val result = repositionBoundaryToEndUtil(doc, "nonexistent")
        assertNull(result)
    }

    @Test
    fun `strips transient HEAD marker while collapsing stale boundaries`() {
        val doc = """
<!-- agent:exchange patch=append -->
### Re: test — opus-4-6 (HEAD)
Response.
<!-- agent:boundary:aaa11111 -->
User prompt.
<!-- agent:boundary:bbb22222 -->
<!-- /agent:exchange -->
""".trimStart()

        val result = repositionBoundaryToEndUtil(doc, "exchange")
        assertNotNull(result)
        assertTrue(result!!.contains("### Re: test — opus-4-6\n"))
        assertEquals(0, Regex("""\(HEAD\)""").findAll(result).count())
        assertEquals(1, Regex("""<!-- agent:boundary:[a-z0-9]+ -->""").findAll(result).count())
        assertTrue(result.contains("User prompt.\n<!-- agent:boundary:"))
        assertFalse(result.contains("aaa11111"))
        assertFalse(result.contains("bbb22222"))
    }

    @Test
    fun `reuses explicit boundary id when provided`() {
        val doc = """
<!-- agent:exchange patch=append -->
Response.
<!-- agent:boundary:aaa11111 -->
User prompt.
<!-- /agent:exchange -->
""".trimStart()

        val result = repositionBoundaryToEndUtil(doc, "exchange", "keep-this-id")
        assertNotNull(result)
        assertTrue(result!!.contains("<!-- agent:boundary:keep-this-id -->"))
        assertFalse(result.contains("aaa11111"))
        assertEquals(1, Regex("""<!-- agent:boundary:[a-z0-9-]+ -->""").findAll(result).count())
    }

    @Test
    fun `reposition preserves comment tail below exchange`() {
        val doc = """
<!-- agent:exchange patch=append -->
Response.
<!-- agent:boundary:aaa11111 -->
User prompt.
<!-- /agent:exchange -->
###

<!--
parked note
-->

<!-- agent:backlog -->
<!-- /agent:backlog -->
""".trimStart()

        val result = repositionBoundaryToEndUtil(doc, "exchange", "fresh1234")
        assertNotNull(result)
        assertTrue(result!!.contains("User prompt.\n<!-- agent:boundary:fresh1234 -->"))
        assertTrue(
            result.contains(
                """
<!-- /agent:exchange -->
###

<!--
parked note
-->

<!-- agent:backlog -->
<!-- /agent:backlog -->
""".trimStart()
            )
        )
    }

    @Test
    fun `extractBooleanField parses preserve_head from IPC JSON`() {
        val json = """{"type":"reposition","file":"/tmp/doc.md","boundary_id":"abc12345","preserve_head":true}"""
        assertTrue(extractBooleanField(json, "preserve_head"))

        val jsonFalse = """{"type":"reposition","file":"/tmp/doc.md","boundary_id":"abc12345","preserve_head":false}"""
        assertFalse(extractBooleanField(jsonFalse, "preserve_head"))

        val jsonMissing = """{"type":"reposition","file":"/tmp/doc.md","boundary_id":"abc12345"}"""
        assertFalse(extractBooleanField(jsonMissing, "preserve_head"))
    }

    @Test
    fun `clean reposition strips HEAD markers`() {
        val doc = """
<!-- agent:exchange patch=append -->
### Re: topic — opus-4-6 (HEAD)
Response content.
<!-- agent:boundary:aaa11111 -->
User prompt.
<!-- /agent:exchange -->
""".trimStart()

        val result = repositionBoundaryToEndUtil(doc, "exchange")
        assertNotNull(result)
        assertFalse(result!!.contains("(HEAD)"))
        assertTrue(result.contains("### Re: topic — opus-4-6\n"))
        assertTrue(result.contains("User prompt.\n<!-- agent:boundary:"))
    }

    @Test
    fun `preserve-head reposition keeps HEAD markers for editor-visible cleanup`() {
        val doc = """
<!-- agent:exchange patch=append -->
### Re: topic — opus-4-6 (HEAD)
Response content.
<!-- agent:boundary:aaa11111 -->
User prompt.
<!-- /agent:exchange -->
""".trimStart()

        val result = repositionBoundaryToEndUtil(doc, "exchange", "committed-id", preserveHead = true)

        assertNotNull(result)
        assertTrue(result!!.contains("### Re: topic — opus-4-6 (HEAD)\n"))
        assertTrue(result.contains("User prompt.\n<!-- agent:boundary:committed-id -->"))
        assertFalse(result.contains("aaa11111"))
    }

    @Test
    fun `parsePatchJson accepts preserve-head file IPC reposition patches`() {
        val json = """
            {
              "file": "/tmp/doc.md",
              "patches": [],
              "unmatched": "",
              "patch_id": "patch-1",
              "reposition_boundary": true,
              "reposition_boundary_id": "committed-id",
              "preserve_head": true
            }
        """.trimIndent()

        val patch = requireNotNull(parsePatchJson(json))

        assertTrue(patch.repositionBoundary)
        assertTrue(patch.preserveHead)
        assertEquals("committed-id", patch.repositionBoundaryId)
        assertTrue(patch.patches.isEmpty())
    }

    @Test
    fun `parsePatchJson preserves node-addressed component patch fields`() {
        val json = """
            {
              "file": "/tmp/doc.md",
              "patches": [
                {
                  "component": "exchange",
                  "content": "### Re: topic\n\nDone.",
                  "op": "append",
                  "boundary_id": "abc123:doc",
                  "node_id": "abc123:doc"
                }
              ],
              "unmatched": "",
              "patch_id": "patch-1"
            }
        """.trimIndent()

        val patch = requireNotNull(parsePatchJson(json))
        val componentPatch = patch.patches.single()

        assertEquals("exchange", componentPatch.component)
        assertEquals("append", componentPatch.op)
        assertEquals("abc123:doc", componentPatch.boundaryId)
        assertEquals("abc123:doc", componentPatch.nodeId)
    }

    @Test
    fun `parsePatchJson preserves narrow normalization repair payload fields`() {
        val json = """
            {
              "file": "/tmp/doc.md",
              "patches": [],
              "unmatched": "",
              "patch_id": "patch-1",
              "reposition_boundary": true,
              "reposition_boundary_id": "committed-id",
              "preserve_head": true,
              "normalize_prefix_lines": ["do #repaircleanup. spec-test-build-install-commit-push"],
              "expected_content_hash": "abc123",
              "expected_content_len": 42
            }
        """.trimIndent()

        val patch = requireNotNull(parsePatchJson(json))

        assertTrue(patch.repositionBoundary)
        assertEquals("committed-id", patch.repositionBoundaryId)
        assertTrue(patch.preserveHead)
        assertEquals(listOf("do #repaircleanup. spec-test-build-install-commit-push"), patch.normalizePrefixLines)
        assertEquals("patch-1", patch.patchId)
        assertEquals("abc123", patch.expectedContentHash)
        assertEquals(42, patch.expectedContentLen)
        assertTrue(patch.patches.isEmpty())
        assertEquals("", patch.unmatched)
        assertNull(patch.fullContent)
    }

    @Test
    fun `parsePatchJson preserves node-keyed IPC patch payloads`() {
        val json = """
{
  "file": "/tmp/doc.md",
  "patches": [],
  "node_patches": [
    {
      "component": "queue",
      "node_key": "queue:0:beta:0",
      "op": "strike",
      "content": "- ~~do [#beta]~~\n"
    },
    {
      "component": "queue",
      "node_key": "queue:0:gamma:0",
      "op": "insert",
      "content": "- do [#gamma]\n",
      "after": "queue:0:beta:0"
    }
  ],
  "unmatched": "",
  "patch_id": "patch-node-1"
}
""".trimIndent()

        val patch = requireNotNull(parsePatchJson(json))

        assertEquals("patch-node-1", patch.patchId)
        assertEquals(2, patch.nodePatches.size)
        assertEquals("queue:0:beta:0", patch.nodePatches[0].nodeKey)
        assertEquals("strike", patch.nodePatches[0].op)
        assertEquals("- ~~do [#beta]~~\n", patch.nodePatches[0].content)
        assertEquals("queue:0:beta:0", patch.nodePatches[1].after)
    }

    @Test
    fun `annotates newly patched response headings against baseline`() {
        val baseline = """
<!-- agent:exchange patch=append -->
### Re: earlier — gpt-5

Existing answer.
<!-- /agent:exchange -->
""".trimStart()
        val doc = """
<!-- agent:exchange patch=append -->
### Re: earlier — gpt-5

Existing answer.
### Re: latest — gpt-5

Fresh answer.
<!-- agent:boundary:abc12345 -->
<!-- /agent:exchange -->
""".trimStart()

        val result = annotateExchangeHeadingsAgainstBaselineUtil(doc, "exchange", baseline)
        assertNotNull(result)
        assertTrue(result!!.contains("### Re: earlier — gpt-5\n"))
        assertTrue(result.contains("### Re: latest — gpt-5 (HEAD)\n"))
        assertEquals(1, Regex("""\(HEAD\)""").findAll(result).count())
    }

    @Test
    fun `prefers committed disk content for response-heading attribution drift`() {
        val disk = """
<!-- agent:exchange patch=append -->
### Re: update Actual Dev Hrs with corky — gpt-5
Response content.
<!-- agent:boundary:committed123 -->
<!-- /agent:exchange -->
""".trimStart()
        val editor = """
<!-- agent:exchange patch=append -->
### Re: update Actual Dev Hrs with corky — codex (HEAD)
Response content.
<!-- agent:boundary:stale9999 -->
<!-- /agent:exchange -->
""".trimStart()

        assertTrue(shouldPreferCommittedDiskContentForRepositionUtil(editor, disk))
    }

    @Test
    fun `prefers committed disk content when only answered prompt prefix differs`() {
        val disk = """
<!-- agent:exchange patch=append -->
❯ commit + push all uncommitted src/agent-doc files
### Re: commit + push pending src/agent-doc work — gpt-5

Done.
<!-- agent:boundary:committed123 -->
<!-- /agent:exchange -->
""".trimStart()
        val editor = """
<!-- agent:exchange patch=append -->
commit + push all uncommitted src/agent-doc files
### Re: commit + push pending src/agent-doc work — codex (HEAD)

Done.
<!-- agent:boundary:stale9999 -->
<!-- /agent:exchange -->
""".trimStart()

        assertTrue(shouldPreferCommittedDiskContentForRepositionUtil(editor, disk))
    }

    @Test
    fun `does not prefer disk content when prompt prefix differs on unresolved follow up`() {
        val disk = """
<!-- agent:exchange patch=append -->
### Re: topic — gpt-5
Answer.
❯ do #qprx. spec-test-build-install-commit-push
<!-- agent:boundary:committed123 -->
<!-- /agent:exchange -->
""".trimStart()
        val editor = """
<!-- agent:exchange patch=append -->
### Re: topic — codex (HEAD)
Answer.
do #qprx. spec-test-build-install-commit-push
<!-- agent:boundary:stale9999 -->
<!-- /agent:exchange -->
""".trimStart()

        assertFalse(shouldPreferCommittedDiskContentForRepositionUtil(editor, disk))
    }

    @Test
    fun `does not prefer disk content when user follow-up differs`() {
        val disk = """
<!-- agent:exchange patch=append -->
### Re: topic — gpt-5
Answer.
<!-- agent:boundary:committed123 -->
<!-- /agent:exchange -->
""".trimStart()
        val editor = """
<!-- agent:exchange patch=append -->
### Re: topic — codex (HEAD)
Answer.
User follow-up.
<!-- agent:boundary:stale9999 -->
<!-- /agent:exchange -->
""".trimStart()

        assertFalse(shouldPreferCommittedDiskContentForRepositionUtil(editor, disk))
    }

    @Test
    fun `does not prefer disk content when comment tail differs outside exchange`() {
        val disk = """
<!-- agent:exchange patch=append -->
### Re: topic — gpt-5
Answer.
<!-- agent:boundary:committed123 -->
<!-- /agent:exchange -->
###

<!--
old parked note
-->
""".trimStart()
        val editor = """
<!-- agent:exchange patch=append -->
### Re: topic — codex (HEAD)
Answer.
<!-- agent:boundary:stale9999 -->
<!-- /agent:exchange -->
###

<!--
edited parked note
-->
""".trimStart()

        assertFalse(shouldPreferCommittedDiskContentForRepositionUtil(editor, disk))
    }

    @Test
    fun `does not prefer disk content when markdown tail differs outside exchange`() {
        val disk = """
<!-- agent:exchange patch=append -->
### Re: topic — gpt-5
Answer.
<!-- agent:boundary:committed123 -->
<!-- /agent:exchange -->

## Notes

old note
""".trimStart()
        val editor = """
<!-- agent:exchange patch=append -->
### Re: topic — codex (HEAD)
Answer.
<!-- agent:boundary:stale9999 -->
<!-- /agent:exchange -->

## Notes

edited note
""".trimStart()

        assertFalse(shouldPreferCommittedDiskContentForRepositionUtil(editor, disk))
    }
}
