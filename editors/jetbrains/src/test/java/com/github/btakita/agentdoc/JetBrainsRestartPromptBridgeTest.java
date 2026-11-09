package com.github.btakita.agentdoc;

import org.junit.Test;

import java.util.concurrent.atomic.AtomicBoolean;
import java.util.concurrent.atomic.AtomicReference;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertSame;
import static org.junit.Assert.assertTrue;

public class JetBrainsRestartPromptBridgeTest {
    @Test
    public void bridgeReturnsReceiptAfterTheIdeOwnsTheScheduledDialog() {
        AtomicReference<Runnable> scheduled = new AtomicReference<>();
        AtomicBoolean ran = new AtomicBoolean(false);
        Runnable dialog = () -> ran.set(true);

        String receipt = JetBrainsPluginUpgradeAction.scheduleRestartPrompt(
            "0.2.966",
            scheduled::set,
            dialog
        );

        assertEquals("ok:restart-prompt:0.2.966", receipt);
        assertSame(dialog, scheduled.get());
        assertFalse(ran.get());
        scheduled.get().run();
        assertTrue(ran.get());
    }
}
