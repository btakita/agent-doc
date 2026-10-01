package com.github.btakita.agentdoc;

import org.junit.Test;

import java.io.InputStream;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertSame;
import static org.junit.Assert.assertTrue;
import static org.junit.Assert.fail;

/**
 * `#jbunloadoptsloader` (GH #80): the update-mode {@code UnloadPluginOptions} is built through the
 * loader that defined {@code DynamicPlugins}, not the bootstrap's system-classloader parent.
 * On IU-262.9437.185 the system loader resolved {@code DynamicPlugins} but not its nested class,
 * so a compile-time {@code new} died with {@code NoClassDefFoundError} before anything unloaded.
 */
public class JetBrainsPluginUpgradeOptionsLoaderTest {
    /** The platform shape: a nested options builder on the owner. */
    public static final class FakeDynamicPlugins {
        public static final class UnloadPluginOptions {
            public boolean disable = true;
            public boolean isUpdate = false;

            public UnloadPluginOptions() {}

            public UnloadPluginOptions withDisable(boolean value) {
                disable = value;
                return this;
            }

            public UnloadPluginOptions withUpdate(boolean value) {
                isUpdate = value;
                return this;
            }
        }
    }

    /** An owner whose nested options class exists nowhere. */
    public static final class OwnerWithoutOptions {}

    @Test
    public void optionsResolveFromTheOwnersDefiningLoaderInUpdateMode() throws Exception {
        // Define the owner and its nested class in a loader the test's own loader cannot see,
        // mirroring the IDE: the action's loader is not the one that defined DynamicPlugins.
        ClassLoader isolated = new IsolatedLoader(
            FakeDynamicPlugins.class.getName(),
            FakeDynamicPlugins.UnloadPluginOptions.class.getName()
        );
        Class<?> owner = Class.forName(FakeDynamicPlugins.class.getName(), true, isolated);
        assertSame(isolated, owner.getClassLoader());

        Object options = JetBrainsPluginUpgradeAction.updateUnloadOptions(owner);

        assertSame(isolated, options.getClass().getClassLoader());
        assertEquals(Boolean.FALSE, options.getClass().getField("disable").get(options));
        assertEquals(Boolean.TRUE, options.getClass().getField("isUpdate").get(options));
    }

    @Test
    public void anUnresolvableOptionsClassIsAnUpgraderFailureNotARefusal() {
        try {
            JetBrainsPluginUpgradeAction.updateUnloadOptions(
                OwnerWithoutOptions.class, ClassLoader.getSystemClassLoader()
            );
            fail("a missing options class must fail closed");
        } catch (IllegalStateException missing) {
            String message = missing.getMessage();
            assertTrue(message, message.startsWith(JetBrainsPluginUpgradeAction.UPGRADER_FAILED));
            assertTrue(message, !message.contains(JetBrainsPluginUpgradeAction.DYNAMIC_UNLOAD_REFUSED));
            assertTrue(message, message.contains("OwnerWithoutOptions$UnloadPluginOptions"));
        }
    }

    @Test
    public void aLinkageFailureKeepsItsClassName() {
        String described = JetBrainsPluginUpgradeAction.describeFailure(
            new NoClassDefFoundError("com/intellij/ide/plugins/DynamicPlugins$UnloadPluginOptions")
        );
        assertTrue(described, described.startsWith(JetBrainsPluginUpgradeAction.UPGRADER_FAILED));
        assertTrue(described, described.contains("java.lang.NoClassDefFoundError: com/intellij"));
        assertEquals(
            "plugin cannot unload dynamically: x",
            JetBrainsPluginUpgradeAction.describeFailure(
                new IllegalStateException("plugin cannot unload dynamically: x")
            )
        );
    }

    /** Defines the named classes itself from the test classpath bytes; delegates the rest. */
    private static final class IsolatedLoader extends ClassLoader {
        private final java.util.Set<String> owned;

        IsolatedLoader(String... owned) {
            super(JetBrainsPluginUpgradeOptionsLoaderTest.class.getClassLoader());
            this.owned = java.util.Set.of(owned);
        }

        @Override
        protected Class<?> loadClass(String name, boolean resolve) throws ClassNotFoundException {
            if (!owned.contains(name)) {
                return super.loadClass(name, resolve);
            }
            synchronized (getClassLoadingLock(name)) {
                Class<?> loaded = findLoadedClass(name);
                if (loaded == null) {
                    String resource = name.replace('.', '/') + ".class";
                    try (InputStream in = getParent().getResourceAsStream(resource)) {
                        byte[] bytes = in.readAllBytes();
                        loaded = defineClass(name, bytes, 0, bytes.length);
                    } catch (Exception failure) {
                        throw new ClassNotFoundException(name, failure);
                    }
                }
                if (resolve) {
                    resolveClass(loaded);
                }
                return loaded;
            }
        }
    }
}
