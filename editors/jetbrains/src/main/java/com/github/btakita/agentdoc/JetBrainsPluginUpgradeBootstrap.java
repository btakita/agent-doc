package com.github.btakita.agentdoc;

import com.sun.tools.attach.VirtualMachine;

import java.lang.instrument.Instrumentation;
import java.lang.reflect.InvocationTargetException;
import java.net.URI;
import java.net.URL;
import java.net.URLClassLoader;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.util.Base64;

/**
 * Stable system-classloader bootstrap for restart-free local plugin upgrades.
 *
 * <p>A JVM retains an attached agent class in its system classloader. The
 * bootstrap therefore loads the package-changing action child-first from the
 * newly supplied agent Jar on every invocation. Future installer releases can
 * update that action without requiring an IDE restart or retaining the old
 * plugin classloader.</p>
 */
public final class JetBrainsPluginUpgradeBootstrap {
    private static final String ACTION_CLASS =
        "com.github.btakita.agentdoc.JetBrainsPluginUpgradeAction";

    private JetBrainsPluginUpgradeBootstrap() {}

    public static void main(String[] args) throws Exception {
        if (args.length != 4) {
            throw new IllegalArgumentException(
                "usage: JetBrainsPluginUpgradeBootstrap <pid> <plugin-zip> <plugins-dir> <expected-version>"
            );
        }
        Path self = Path.of(new URI(
            JetBrainsPluginUpgradeBootstrap.class.getProtectionDomain().getCodeSource().getLocation().toString()
        )).toAbsolutePath().normalize();
        Path status = Files.createTempFile("agent-doc-jb-upgrade-", ".status");
        try {
            String options = String.join(
                ".",
                encode(args[1]),
                encode(args[2]),
                encode(args[3]),
                encode(status.toString()),
                encode(self.toString())
            );
            VirtualMachine vm = VirtualMachine.attach(args[0]);
            try {
                vm.loadAgent(self.toString(), options);
            } finally {
                vm.detach();
            }
            String result = Files.readString(status, StandardCharsets.UTF_8).trim();
            if (result.startsWith("ok:") || result.startsWith("skip:")) {
                System.out.println(result);
                return;
            }
            throw new IllegalStateException(result.isEmpty() ? "IDE returned no upgrade result" : result);
        } finally {
            Files.deleteIfExists(status);
        }
    }

    public static void agentmain(String options, Instrumentation instrumentation) {
        String[] fields = options.split("\\.", -1);
        Path status = fields.length == 5 ? Path.of(decode(fields[3])) : null;
        try {
            if (fields.length != 5) {
                throw new IllegalArgumentException("invalid updater agent options");
            }
            Path agentJar = Path.of(decode(fields[4])).toAbsolutePath().normalize();
            try (ActionClassLoader loader = new ActionClassLoader(agentJar.toUri().toURL())) {
                Class<?> action = Class.forName(ACTION_CLASS, true, loader);
                String result = (String) action
                    .getMethod("run", String.class, String.class, String.class)
                    .invoke(null, decode(fields[0]), decode(fields[1]), decode(fields[2]));
                writeStatus(status, result);
            }
        } catch (Throwable failure) {
            Throwable cause = failure instanceof InvocationTargetException invocation && invocation.getCause() != null
                ? invocation.getCause()
                : failure;
            if (status != null) {
                writeStatus(status, "error:" + cause.getClass().getName() + ":" + String.valueOf(cause.getMessage()));
            }
        }
    }

    private static String encode(String value) {
        return Base64.getUrlEncoder().withoutPadding().encodeToString(value.getBytes(StandardCharsets.UTF_8));
    }

    private static String decode(String value) {
        return new String(Base64.getUrlDecoder().decode(value), StandardCharsets.UTF_8);
    }

    private static void writeStatus(Path status, String value) {
        try {
            Path staged = status.resolveSibling(status.getFileName() + ".tmp");
            Files.writeString(staged, value, StandardCharsets.UTF_8);
            Files.move(staged, status, StandardCopyOption.REPLACE_EXISTING, StandardCopyOption.ATOMIC_MOVE);
        } catch (Exception ignored) {
            // The launcher reports an empty status as a hard failure.
        }
    }

    private static final class ActionClassLoader extends URLClassLoader {
        private ActionClassLoader(URL agentJar) {
            super(new URL[] {agentJar}, ClassLoader.getSystemClassLoader());
        }

        @Override
        protected Class<?> loadClass(String name, boolean resolve) throws ClassNotFoundException {
            if (!ACTION_CLASS.equals(name)) {
                return super.loadClass(name, resolve);
            }
            synchronized (getClassLoadingLock(name)) {
                Class<?> loaded = findLoadedClass(name);
                if (loaded == null) {
                    loaded = findClass(name);
                }
                if (resolve) {
                    resolveClass(loaded);
                }
                return loaded;
            }
        }
    }
}
