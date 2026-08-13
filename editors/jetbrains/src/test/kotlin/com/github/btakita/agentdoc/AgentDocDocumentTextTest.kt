package com.github.btakita.agentdoc

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * `#replicarefusalstorm` (editor half) — the plugin registered every `.md` it
 * opened, including files the controller refuses terminally, and retried each
 * refusal on an 8s backoff forever.
 *
 * These cases are the four real files measured retrying on 2026-08-12 plus the
 * marker shapes that must keep registering.
 */
class AgentDocDocumentTextTest {
    @Test
    fun `frontmatter and component markers both identify a session document`() {
        assertTrue(isAgentDocDocumentTextUtil("---\nagent_doc_format: template\n---\n"))
        assertTrue(isAgentDocDocumentTextUtil("---\nagent_doc_write: crdt\n---\n"))
        assertTrue(isAgentDocDocumentTextUtil("---\nagent_doc_session: x\n---\n"))
        assertTrue(isAgentDocDocumentTextUtil("# doc\n\n<!-- agent:exchange -->\n"))
    }

    @Test
    fun `plain markdown is not a session document`() {
        // README, CONTRIBUTING, a market-research note and a plan doc: the exact
        // shapes that were retrying.
        assertFalse(isAgentDocDocumentTextUtil("# README\n\nInstall with cargo.\n"))
        assertFalse(isAgentDocDocumentTextUtil("# Contributing\n\nOpen a PR.\n"))
        assertFalse(isAgentDocDocumentTextUtil("# SDK market research\n\nNotes.\n"))
        assertFalse(isAgentDocDocumentTextUtil("# Contract source of truth\n\n## Plan\n"))
    }

    @Test
    fun `an empty document is not a session document`() {
        assertFalse(isAgentDocDocumentTextUtil(""))
    }

    @Test
    fun `a plain file that gains markers is recognized immediately`() {
        // The test runs on current text with no memoization, so the transition
        // needs nothing invalidated. This is what makes skipping registration
        // safe rather than sticky.
        val before = "# notes\n\njust prose\n"
        assertFalse(isAgentDocDocumentTextUtil(before))
        assertTrue(isAgentDocDocumentTextUtil(before + "\n<!-- agent:exchange -->\n"))
    }
}
