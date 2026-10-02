package com.github.btakita.agentdoc;

import org.junit.Test;

import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertTrue;

/**
 * The bootstrap loads the upgrade action child-first from the new agent jar. Its
 * nested classes must come from the same loader: loading only the outer class
 * child-first left {@code JetBrainsPluginUpgradeAction$1} to the live plugin's
 * loader, and the cross-loader package-private access threw
 * {@code IllegalAccessError}, so every restart-free upgrade staged instead.
 */
public class JetBrainsPluginUpgradeBootstrapLoaderTest {
    private static final String ACTION = "com.github.btakita.agentdoc.JetBrainsPluginUpgradeAction";

    @Test
    public void theActionAndEveryNestedClassLoadChildFirst() {
        assertTrue(JetBrainsPluginUpgradeBootstrap.isActionClass(ACTION));
        assertTrue(JetBrainsPluginUpgradeBootstrap.isActionClass(ACTION + "$1"));
        assertTrue(JetBrainsPluginUpgradeBootstrap.isActionClass(ACTION + "$UnloadCall"));
    }

    @Test
    public void unrelatedClassesStayParentFirst() {
        assertFalse(JetBrainsPluginUpgradeBootstrap.isActionClass(ACTION + "Helper"));
        assertFalse(JetBrainsPluginUpgradeBootstrap.isActionClass("com.github.btakita.agentdoc.CrdtReplicaManager"));
        assertFalse(JetBrainsPluginUpgradeBootstrap.isActionClass("java.lang.String"));
    }
}
