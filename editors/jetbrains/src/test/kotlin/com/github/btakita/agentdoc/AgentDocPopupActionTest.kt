package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.w3c.dom.Element
import java.io.File
import javax.xml.parsers.DocumentBuilderFactory

class AgentDocPopupActionTest {
    @Test
    fun `primary popup keeps clear session context numbered ninth`() {
        assertEquals(
            listOf(
                "AgentDoc.Submit",
                "AgentDoc.InitSession",
                "AgentDoc.FixDocument",
                "AgentDoc.Claim",
                "AgentDoc.CompactExchange",
                "AgentDoc.ShowSessionStatus",
                "AgentDoc.RestartSupervisorProcess",
                "AgentDoc.RestartAgent",
                "AgentDoc.ClearSessionContext",
                "AgentDoc.LintDocument",
                "AgentDoc.CancelTurn",
                "AgentDoc.CopySessionDiagnostics",
                "AgentDoc.SyncLayout",
                "AgentDoc.LoadTmuxWindow",
                "AgentDoc.RefreshEnvironment",
                "AgentDoc.Dashboard",
            ),
            AgentDocPopupAction.PRIMARY_ACTION_IDS,
        )
        assertEquals("AgentDoc.ClearSessionContext", AgentDocPopupAction.PRIMARY_ACTION_IDS[8])
        assertEquals(1, AgentDocPopupAction.PRIMARY_ACTION_IDS.count { it == "AgentDoc.ClearSessionContext" })
        assertFalse(AgentDocPopupAction.PRIMARY_ACTION_IDS.contains("AgentDoc.InterruptClearSessionContext"))
        assertFalse(AgentDocPopupAction.PRIMARY_ACTION_IDS.contains("AgentDoc.RunWithJunie"))
        assertFalse(AgentDocPopupAction.PRIMARY_ACTION_IDS.contains("AgentDoc.ForceClaim"))
    }

    @Test
    fun `overflow popup actions keep junie and force claim available`() {
        assertEquals(
            listOf(
                "AgentDoc.RunWithJunie",
                "AgentDoc.ForceClaim",
                "AgentDoc.InterruptClearSessionContext",
                // #plugin-cleanup-menu-command: operator session-hygiene commands
                // live in the overflow group (occasional, project-scoped cleanup).
                "AgentDoc.ResyncFixSessions",
                "AgentDoc.GcStaleSessions",
                "AgentDoc.StopAgent",
                "AgentDoc.KillSupervisor",
                "AgentDoc.About",
            ),
            AgentDocPopupAction.OVERFLOW_ACTION_IDS,
        )
    }

    private val pluginXml = File("src/main/resources/META-INF/plugin.xml")

    private fun actionElements(): List<Element> {
        val document = DocumentBuilderFactory.newInstance().newDocumentBuilder().parse(pluginXml)
        val nodes = document.getElementsByTagName("action")
        return (0 until nodes.length).map { nodes.item(it) as Element }
    }

    private fun Element.children(tag: String): List<Element> {
        val nodes = getElementsByTagName(tag)
        return (0 until nodes.length).map { nodes.item(it) as Element }
    }

    private fun Element.keystrokes(keymap: String): List<String> =
        children("keyboard-shortcut")
            .filter { it.getAttribute("keymap") == keymap }
            .map { it.getAttribute("first-keystroke").trim().lowercase().split(Regex("\\s+")).joinToString(" ") }

    private fun Element.defaultKeystrokes(): List<String> = keystrokes("\$default")

    @Test
    fun `every declared agent-doc action is reachable from the popup`() {
        // #gh116: StopAgent and KillSupervisor were declared but never listed.
        val declared = actionElements().map { it.getAttribute("id") }
            .filter { it.startsWith("AgentDoc.") && it != "AgentDoc.Popup" }
        assertTrue("plugin.xml should declare agent-doc actions", declared.size >= 20)
        val listed = AgentDocPopupAction.PRIMARY_ACTION_IDS + AgentDocPopupAction.OVERFLOW_ACTION_IDS
        assertEquals("popup ids must be unique", listed.size, listed.toSet().size)
        assertEquals("declared actions missing from the popup", emptyList<String>(), declared - listed.toSet())
        assertEquals("popup lists undeclared actions", emptyList<String>(), listed - declared.toSet())
    }

    @Test
    fun `no default shortcut uses a keystroke the OS consumes`() {
        // #gh116: Windows consumes alt SPACE (window system menu), alt F4 and
        // alt TAB before the IDE sees them, so such a default is unreachable.
        val reserved = setOf("alt space", "alt f4", "alt tab")
        val offenders = actionElements().flatMap { action ->
            action.defaultKeystrokes().filter { it in reserved }.map { "${action.getAttribute("id")}: $it" }
        }
        assertEquals(emptyList<String>(), offenders)
    }

    @Test
    fun `popup has a cross-platform default shortcut and a menu entry`() {
        val popup = actionElements().single { it.getAttribute("id") == "AgentDoc.Popup" }
        assertEquals(listOf("ctrl shift alt d"), popup.defaultKeystrokes())
        val groups = popup.children("add-to-group").map { it.getAttribute("group-id") }
        assertTrue("popup must be reachable without its shortcut: $groups", "ToolsMenu" in groups)
        assertTrue("popup must be in the editor context menu: $groups", "EditorPopupMenu" in groups)
        val otherDefaults = actionElements()
            .filter { it.getAttribute("id") != "AgentDoc.Popup" }
            .flatMap { it.defaultKeystrokes() }
        assertFalse("popup default collides with another agent-doc default", "ctrl shift alt d" in otherDefaults)
    }

    @Test
    fun `x window manager keymap keeps alt space for the popup`() {
        // `altshiftmenu`: #gh116 removed alt SPACE from every keymap. On Linux
        // under a bare X window manager (the operator runs i3 with a keymap
        // derived from "Default for XWin") Alt+Space reaches the IDE and was the
        // popup's working key, so the Windows fix silently took the menu away.
        val popup = actionElements().single { it.getAttribute("id") == "AgentDoc.Popup" }
        assertEquals(listOf("alt space"), popup.keystrokes("Default for XWin"))

        // The XWin shortcut is added on top of the parent's list only if the
        // `$default` binding was registered first (KeymapImpl copies the parent
        // shortcuts when an action gets its first own shortcut), so XWin keeps
        // Ctrl+Shift+Alt+D too.
        val keymapOrder = popup.children("keyboard-shortcut").map { it.getAttribute("keymap") }
        assertEquals(listOf("\$default", "Default for XWin"), keymapOrder)

        // Keymaps whose platforms consume Alt+Space never carry it:
        // Windows/macOS use $default or Mac keymaps, GNOME opens the window
        // menu, KDE opens KRunner.
        val consuming = listOf("\$default", "Default for GNOME", "Default for KDE", "Mac OS X 10.5+", "Mac OS X")
        val offenders = actionElements().flatMap { action ->
            consuming.flatMap { keymap ->
                action.keystrokes(keymap).filter { it == "alt space" }.map { "${action.getAttribute("id")} [$keymap]" }
            }
        }
        assertEquals(emptyList<String>(), offenders)

        // No other agent-doc action claims Alt+Space in any keymap.
        val others = actionElements()
            .filter { it.getAttribute("id") != "AgentDoc.Popup" }
            .flatMap { action -> action.children("keyboard-shortcut").map { it.getAttribute("first-keystroke").trim().lowercase() } }
        assertFalse("alt space collides with another agent-doc shortcut", others.any { it.split(Regex("\\s+")).joinToString(" ") == "alt space" })
    }
}
