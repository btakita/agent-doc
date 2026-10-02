package com.github.btakita.agentdoc;

import org.junit.Test;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertNull;

/**
 * An attached agent jar is appended to the IDE's system class path once per JVM, so any nested
 * class of {@code JetBrainsPluginUpgradeAction} resolves from the FIRST jar ever attached. A
 * nested class from an older plugin then fails the newer action with IllegalAccessError and
 * every restart-free upgrade stages (observed upgrading 0.2.467 to 0.2.468 on IU-261). The
 * action must therefore compile to exactly one class file.
 */
public class JetBrainsPluginUpgradeActionShapeTest {
    @Test
    public void theUpgradeActionCompilesToASingleClassFile() {
        assertEquals(0, JetBrainsPluginUpgradeAction.class.getDeclaredClasses().length);
        for (int index = 1; index <= 9; index++) {
            assertNull(
                "anonymous class " + index + " would resolve from the first attached agent jar",
                JetBrainsPluginUpgradeAction.class.getResource("JetBrainsPluginUpgradeAction$" + index + ".class")
            );
        }
    }
}
