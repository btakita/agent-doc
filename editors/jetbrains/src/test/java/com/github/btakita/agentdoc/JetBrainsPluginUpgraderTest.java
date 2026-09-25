package com.github.btakita.agentdoc;

import org.junit.Test;

import java.nio.file.Path;

import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertTrue;

public class JetBrainsPluginUpgraderTest {
    @Test
    public void installRootComparisonNormalizesPathsWithoutCrossingIdeInstallations() {
        assertTrue(JetBrainsPluginUpgradeAction.sameInstallRoot(
            Path.of("/plugins/Idea/agent-doc-jetbrains/../agent-doc-jetbrains"),
            Path.of("/plugins/Idea/agent-doc-jetbrains")
        ));
        assertFalse(JetBrainsPluginUpgradeAction.sameInstallRoot(
            Path.of("/plugins/Idea/agent-doc-jetbrains"),
            Path.of("/plugins/RustRover/agent-doc-jetbrains")
        ));
    }
}
