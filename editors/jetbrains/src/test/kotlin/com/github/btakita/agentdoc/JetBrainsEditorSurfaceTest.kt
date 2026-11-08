package com.github.btakita.agentdoc

import java.lang.invoke.MethodHandles
import java.util.LinkedHashMap
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Test

class JetBrainsEditorSurfaceTest {
    @Test
    fun `mount receipt names exactly one detached surface`() {
        val decision = parseTerminalSurfaceDecision(
            """{"terminal_decision":{"kind":"mount","surface_id":"dock-window-7","document":"/repo/tasks/plan.md","previous":{"surface_id":"main","document":"/repo/tasks/plan.md"}}}""",
        )

        assertEquals(TerminalSurfaceDecisionKind.MOUNT, decision?.kind)
        assertEquals("dock-window-7", decision?.surfaceId)
        assertEquals("/repo/tasks/plan.md", decision?.document)
    }

    @Test
    fun `stash receipt has no accidental mount target`() {
        val decision = parseTerminalSurfaceDecision(
            """{"terminal_decision":{"kind":"stash","previous":{"surface_id":"dock-window-7","document":"/repo/tasks/plan.md"}}}""",
        )

        assertEquals(TerminalSurfaceDecisionKind.STASH, decision?.kind)
        assertNull(decision?.surfaceId)
    }

    @Test
    fun `old controller receipt is ignored`() {
        assertNull(parseTerminalSurfaceDecision("""{"intent":{"kind":"idle"},"idle":true}"""))
    }

    @Test
    fun `current detached surface augments lagging window enumeration`() {
        assertEquals(
            setOf("main-frame", "frame-detached"),
            JetBrainsEditorSurfaces.authoritativeLiveSurfaceIds(
                setOf("main-frame"),
                "frame-detached",
            ),
        )
    }

    @Test
    fun `missing backend windows remain unknown despite a current surface hint`() {
        assertNull(
            JetBrainsEditorSurfaces.authoritativeLiveSurfaceIds(
                null,
                "frame-detached",
            ),
        )
    }

    @Test
    fun `pane enumeration keeps the list overload ahead of the map accessor`() {
        val fixture =
            MethodHandles.lookup().defineClass(
                toolWindowPanesOverloadClassBytes(
                    "com/github/btakita/agentdoc/ToolWindowPanesOverloadFixture",
                ),
            )
        val declared =
            fixture.declaredMethods.filter { method ->
                method.name.startsWith("getToolWindowPanes") && method.parameterCount == 0
            }
        assertEquals(LinkedHashMap::class.java, declared[0].returnType)
        assertEquals(List::class.java, declared[1].returnType)
        val selected = MarketplacePlatformBridge.selectToolWindowPanesMethod(fixture.methods)
        assertEquals(List::class.java, selected?.returnType)
        assertNotEquals(declared[0], selected)
    }
}

private fun toolWindowPanesOverloadClassBytes(internalName: String): ByteArray {
    val pool = ClassConstantPool()
    val name = pool.utf8(internalName)
    val thisClass = pool.classRef(name)
    val objectName = pool.utf8("java/lang/Object")
    val superClass = pool.classRef(objectName)
    val methodName = pool.utf8("getToolWindowPanes\$intellij_platform_ide_impl")
    val mapDescriptor = pool.utf8("()Ljava/util/LinkedHashMap;")
    val listDescriptor = pool.utf8("()Ljava/util/List;")
    val out = java.io.ByteArrayOutputStream()
    out.write(byteArrayOf(0xCA.toByte(), 0xFE.toByte(), 0xBA.toByte(), 0xBE.toByte()))
    out.writeShort(0)
    out.writeShort(65)
    pool.write(out)
    out.writeShort(0x0421)
    out.writeShort(thisClass)
    out.writeShort(superClass)
    out.writeShort(0)
    out.writeShort(0)
    out.writeShort(2)
    out.writeShort(0x0401)
    out.writeShort(methodName)
    out.writeShort(mapDescriptor)
    out.writeShort(0)
    out.writeShort(0x0401)
    out.writeShort(methodName)
    out.writeShort(listDescriptor)
    out.writeShort(0)
    out.writeShort(0)
    return out.toByteArray()
}

private class ClassConstantPool {
    private val entries = mutableListOf<ByteArray>()

    fun utf8(value: String): Int {
        val bytes = value.toByteArray(Charsets.UTF_8)
        val body = ByteArray(3 + bytes.size)
        body[0] = 1
        body[1] = (bytes.size ushr 8).toByte()
        body[2] = bytes.size.toByte()
        bytes.copyInto(body, 3)
        entries += body
        return entries.size
    }

    fun classRef(nameIndex: Int): Int {
        entries += byteArrayOf(7, (nameIndex ushr 8).toByte(), nameIndex.toByte())
        return entries.size
    }

    fun write(out: java.io.ByteArrayOutputStream) {
        out.writeShort(entries.size + 1)
        entries.forEach(out::write)
    }
}

private fun java.io.ByteArrayOutputStream.writeShort(value: Int) {
    write(value ushr 8)
    write(value)
}
