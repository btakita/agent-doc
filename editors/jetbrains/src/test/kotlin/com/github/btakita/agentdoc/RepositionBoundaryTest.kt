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
}
