package com.github.btakita.agentdoc;

import com.intellij.ide.plugins.DynamicPlugins;
import com.intellij.ide.plugins.IdeaPluginDescriptor;
import com.intellij.ide.plugins.IdeaPluginDescriptorImpl;
import com.intellij.ide.plugins.PluginInstaller;
import com.intellij.ide.plugins.PluginManagerCore;
import com.intellij.openapi.application.ApplicationInfo;
import com.intellij.openapi.application.ApplicationManager;
import com.intellij.openapi.extensions.PluginDescriptor;
import com.intellij.openapi.extensions.PluginId;
import com.intellij.openapi.project.Project;
import com.intellij.openapi.project.ProjectManager;

import java.lang.reflect.Field;
import java.lang.reflect.InvocationTargetException;
import java.lang.reflect.Method;
import java.lang.reflect.Modifier;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.util.ArrayList;
import java.util.Collections;
import java.util.List;
import java.util.concurrent.atomic.AtomicReference;
import java.util.function.IntSupplier;
import java.util.function.Supplier;

/** Freshly loaded implementation of one JetBrains plugin package replacement. */
public final class JetBrainsPluginUpgradeAction {
    private static final String PLUGIN_ID = "com.github.btakita.agent-doc";
    private static final String LIFECYCLE_CLASS =
        "com.github.btakita.agentdoc.PluginLifecycleListener";
    private static final String ASYNC_CLASSLOADER_AWAIT_STRATEGY =
        "com.intellij.ide.plugins.AwaitClassloaderUnloadAsyncPostReconfiguration";

    private JetBrainsPluginUpgradeAction() {}

    public static String run(String archiveValue, String pluginsDirValue, String expectedVersion) {
        AtomicReference<String> result = new AtomicReference<>();
        AtomicReference<Throwable> failure = new AtomicReference<>();
        AtomicReference<IdeaPluginDescriptorImpl> replacement = new AtomicReference<>();
        AtomicReference<IdeaPluginDescriptorImpl> released = new AtomicReference<>();
        ApplicationManager.getApplication().invokeAndWait(() -> {
            try {
                result.set(runOnEdt(archiveValue, pluginsDirValue, expectedVersion, replacement, released));
            } catch (Throwable caught) {
                failure.set(caught);
            }
        });
        if (failure.get() != null) {
            // GH #94: the outgoing generation released its open projects --
            // CRDT replica transport included -- before the unload was attempted. When the
            // upgrade then aborted with that generation still loaded, nothing re-registered the
            // transport and every write to a document this IDE holds deferred until a restart.
            // Rebuild it from the still-live generation before reporting the failure. This runs
            // here, off the EDT, because the rebuild waits for replica receipts.
            IdeaPluginDescriptorImpl outgoing = released.get();
            String reason = describeFailure(failure.get()) + recoverAbortedUnload(
                outgoing != null,
                () -> isLoaded(outgoing),
                () -> reattachOpenDocuments(outgoing)
            );
            // `#jbstageonfail` (GH #80 point 3): a failed hot-swap stages the package for
            // the next IDE start instead of letting the launcher replace jars under this
            // live JVM, which is what manufactured the `plugin_bytes_superseded` state.
            try {
                String stagingNotes = stageForRestart(archiveValue, pluginsDirValue, expectedVersion);
                String staged = stagingNotes.isEmpty() ? reason : reason + "; " + stagingNotes;
                return "staged:" + expectedVersion + ":" + staged.replace('\n', ' ').replace('\r', ' ');
            } catch (Throwable stagingFailure) {
                throw new IllegalStateException(
                    reason + " (staging for restart also failed: " + singleLine(stagingFailure) + ")",
                    failure.get()
                );
            }
        }
        IdeaPluginDescriptorImpl loaded = replacement.get();
        if (loaded == null) {
            return result.get();
        }
        return result.get() + ":" + reattachOpenDocuments(loaded);
    }

    /**
     * Restore the outgoing generation's project services after an upgrade that aborted once
     * they were released (GH #94), and say so in the failure receipt.
     *
     * Nothing to do when the release never ran. When the outgoing generation is still loaded,
     * its own lifecycle entry point rebuilds every open project's listeners and re-registers
     * each open document's replica. When it is no longer loaded the platform already unloaded
     * it and no generation of this plugin serves the IDE, which only a restart repairs.
     */
    static String recoverAbortedUnload(
        boolean released,
        java.util.function.BooleanSupplier stillLoaded,
        java.util.function.Supplier<String> reattach
    ) {
        if (!released) {
            return "";
        }
        if (!stillLoaded.getAsBoolean()) {
            return "; the outgoing plugin generation was already unloaded, so no replica transport"
                + " is served until the IDE restarts";
        }
        String receipt;
        try {
            receipt = reattach.get();
        } catch (Throwable restoreFailure) {
            receipt = "documents=0/0:reattach_error=" + singleLine(restoreFailure);
        }
        return "; restored the live plugin generation's replica transport: " + receipt;
    }

    private static String runOnEdt(
        String archiveValue,
        String pluginsDirValue,
        String expectedVersion,
        AtomicReference<IdeaPluginDescriptorImpl> replacement,
        AtomicReference<IdeaPluginDescriptorImpl> released
    ) {
        Path archive = Path.of(archiveValue).toAbsolutePath().normalize();
        Path pluginsDir = Path.of(pluginsDirValue).toAbsolutePath().normalize();

        IdeaPluginDescriptor descriptor = PluginManagerCore.getPlugin(PluginId.getId(PLUGIN_ID));
        if (!(descriptor instanceof IdeaPluginDescriptorImpl current)) {
            return "skip:plugin-not-loaded";
        }
        Path expectedRoot = pluginsDir.resolve("agent-doc-jetbrains").toAbsolutePath().normalize();
        if (!sameInstallRoot(current.getPluginPath(), expectedRoot)) {
            return "skip:different-plugin-root:" + current.getPluginPath();
        }

        String platformBlocker = dynamicUpgradeBlockerReason(
            asyncPostReconfigurationClassloaderAwaitIsPresent(DynamicPlugins.class.getClassLoader())
        );
        if (platformBlocker != null) {
            throw new IllegalStateException(platformBlocker);
        }

        if (isLoaded(current)) {
            unloadOutgoingGeneration(
                () -> invokeDescriptorMethod(
                    DynamicPlugins.class, DynamicPlugins.INSTANCE, "checkCanUnloadWithoutRestart", current
                ),
                () -> resolveUpdateUnload(DynamicPlugins.class, DynamicPlugins.INSTANCE, current),
                () -> {
                    released.set(current);
                    return cleanupOutgoingGeneration(current);
                }
            );
        }

        IdeaPluginDescriptor residualDescriptor = PluginManagerCore.getPlugin(PluginId.getId(PLUGIN_ID));
        if (residualDescriptor instanceof IdeaPluginDescriptorImpl residual && isLoaded(residual)) {
            throw new IllegalStateException(
                "JetBrains retained the current plugin generation after update unload: "
                    + residual.getVersion()
            );
        }

        boolean loaded = PluginInstaller.installAndLoadDynamicPlugin(archive, current);
        IdeaPluginDescriptor actualDescriptor = PluginManagerCore.getPlugin(PluginId.getId(PLUGIN_ID));
        if (!(actualDescriptor instanceof IdeaPluginDescriptorImpl actual)
            || !loaded
            || actual == current
            || actual.getPluginClassLoader() == current.getPluginClassLoader()
            || !expectedVersion.equals(actual.getVersion())) {
            String actualVersion = actualDescriptor == null ? "missing" : actualDescriptor.getVersion();
            throw new IllegalStateException(
                "dynamic install returned " + loaded + "; expected a fresh " + expectedVersion
                    + " generation, loaded " + actualVersion
            );
        }
        replacement.set(actual);
        return "ok:" + expectedVersion;
    }

    /**
     * Stage {@code archiveValue} through the IDE's own pending-install script so the next IDE
     * start replaces the plugin, leaving the jars this JVM maps untouched (`#jbstageonfail`).
     *
     * The archive the launcher passes is a temp file it deletes on exit, so a durable copy goes
     * into the IDE's plugin temp directory first and the install script owns its deletion.
     *
     * GH #115: two stagings of the same version used to append two delete+unzip blocks that
     * shared one fixed zip name. At the next start the first block installed the plugin and
     * deleted the zip; the second block then deleted the freshly installed plugin and its unzip
     * found no zip, leaving the IDE with no agent-doc plugin at all. Now each staging copies to
     * a unique, verified zip and rewrites the script in one atomic save that drops every prior
     * agent-doc staging and appends exactly one delete+unzip+cleanup block.
     *
     * @return receipt notes (prior stagings replaced, cleanup failures), empty when none
     */
    private static String stageForRestart(String archiveValue, String pluginsDirValue, String expectedVersion)
        throws Exception {
        IdeaPluginDescriptor descriptor = PluginManagerCore.getPlugin(PluginId.getId(PLUGIN_ID));
        if (descriptor == null) {
            throw new IllegalStateException("no " + PLUGIN_ID + " descriptor to stage against");
        }
        Path pluginsDir = Path.of(pluginsDirValue).toAbsolutePath().normalize();
        Path existing = pluginsDir.resolve(PLUGIN_DIR_NAME);
        Class<?> pathManager = Class.forName("com.intellij.openapi.application.PathManager");
        Path tempDir = Path.of(pathManager.getMethod("getPluginTempPath").invoke(null).toString());
        Path idePluginsPath = Path.of(pathManager.getMethod("getPluginsPath").invoke(null).toString());
        Files.createDirectories(tempDir);
        Path staged = copyVerifiedStagedArchive(Path.of(archiveValue), tempDir, expectedVersion);
        Class<?> scriptManager = Class.forName(SCRIPT_MANAGER_CLASS);
        Path script = actionScriptFile(scriptManager, tempDir);
        List<String> notes;
        try {
            notes = replaceStagingInActionScript(
                scriptManager,
                script,
                existing,
                idePluginsPath,
                staged,
                () -> {
                    Method install = findInstallAfterRestart(PluginInstaller.class, descriptor);
                    if (install == null) {
                        throw new IllegalStateException(
                            "StartupActionScriptManager commands are not constructible and PluginInstaller has no "
                                + "installAfterRestart accepting a descriptor and two paths on build " + platformBuild()
                                + "; found: " + signaturesNamed(PluginInstaller.class, "installAfterRestart")
                        );
                    }
                    Object accepted = install.invoke(
                        null, installAfterRestartArguments(install, descriptor, staged, existing)
                    );
                    if (Boolean.FALSE.equals(accepted)) {
                        throw new IllegalStateException("PluginInstaller.installAfterRestart declined " + staged);
                    }
                    return null;
                }
            );
        } catch (Exception stagingFailure) {
            // Nothing references the copy unless the script verified it; do not leak it.
            try {
                Files.deleteIfExists(staged);
            } catch (Exception cleanupFailure) {
                stagingFailure.addSuppressed(cleanupFailure);
            }
            throw stagingFailure;
        }
        return String.join("; ", notes);
    }

    private static final String PLUGIN_DIR_NAME = "agent-doc-jetbrains";
    private static final String STAGED_ARCHIVE_PREFIX = "agent-doc-jetbrains-";
    private static final String SCRIPT_MANAGER_CLASS = "com.intellij.ide.startup.StartupActionScriptManager";

    /** GH #115: unique per staging, so no other staging's {@code delete:<zip>} can name it. */
    static String stagedArchiveName(String expectedVersion, String nonce) {
        return STAGED_ARCHIVE_PREFIX + expectedVersion + "+" + nonce + ".zip";
    }

    static boolean isStagedArchive(String path) {
        if (path == null) {
            return false;
        }
        Path name = Path.of(path).getFileName();
        return name != null
            && name.toString().startsWith(STAGED_ARCHIVE_PREFIX)
            && name.toString().endsWith(".zip");
    }

    /**
     * GH #115 ask 1: copy the launcher's archive to a unique name, prove it is a readable plugin
     * package with an {@code agent-doc-jetbrains/lib/*.jar} entry, and only then move it into
     * place, so the script never names a partial or unreadable zip.
     */
    static Path copyVerifiedStagedArchive(Path archive, Path tempDir, String expectedVersion) throws Exception {
        String nonce = java.util.UUID.randomUUID().toString().replace("-", "").substring(0, 12);
        Path staged = tempDir.resolve(stagedArchiveName(expectedVersion, nonce));
        Path partial = tempDir.resolve(staged.getFileName() + ".partial");
        Files.copy(archive, partial, StandardCopyOption.REPLACE_EXISTING);
        try {
            verifyStagedArchive(partial);
            Files.move(partial, staged, StandardCopyOption.ATOMIC_MOVE);
        } catch (Exception failure) {
            try {
                Files.deleteIfExists(partial);
            } catch (Exception cleanupFailure) {
                failure.addSuppressed(cleanupFailure);
            }
            throw failure;
        }
        return staged;
    }

    static void verifyStagedArchive(Path zip) throws Exception {
        try (java.util.zip.ZipFile file = new java.util.zip.ZipFile(zip.toFile())) {
            boolean hasJar = file.stream().anyMatch(entry ->
                !entry.isDirectory()
                    && entry.getName().startsWith(PLUGIN_DIR_NAME + "/lib/")
                    && entry.getName().endsWith(".jar")
            );
            if (!hasJar) {
                throw new IllegalStateException(
                    "staged package " + zip + " has no " + PLUGIN_DIR_NAME + "/lib/*.jar entry; refusing to stage "
                        + "a delete of the installed plugin without a replacement"
                );
            }
        }
    }

    private static Path actionScriptFile(Class<?> scriptManager, Path tempDir) {
        try {
            Method file = scriptManager.getDeclaredMethod("getActionScriptFile");
            file.setAccessible(true);
            return Path.of(file.invoke(null).toString());
        } catch (ReflectiveOperationException | RuntimeException unavailable) {
            try {
                Object name = scriptManager.getField("ACTION_SCRIPT_FILE").get(null);
                return tempDir.resolve(name.toString());
            } catch (ReflectiveOperationException | RuntimeException missingConstant) {
                return tempDir.resolve("action.script");
            }
        }
    }

    /**
     * {kind, source, destination} of one pending-install command: kind is the command class's
     * simple name ({@code DeleteCommand}, {@code UnzipCommand}, ...). Fails closed when a delete
     * or unzip command's paths cannot be read, because dedupe would then be guesswork.
     */
    static String[] describeCommand(Object command) {
        String kind = command.getClass().getSimpleName();
        String source = readStringField(command, "mySource");
        String destination = readStringField(command, "myDestination");
        if (source == null) {
            try {
                Object value = command.getClass().getMethod("getSource").invoke(command);
                source = value == null ? null : value.toString();
            } catch (ReflectiveOperationException | RuntimeException unavailable) {
                source = null;
            }
        }
        if (source == null && (kind.equals("DeleteCommand") || kind.equals("UnzipCommand"))) {
            throw new IllegalStateException(
                "cannot read the path of pending-install command " + command + " on build " + platformBuild()
                    + "; refusing to stage without deduplicating the existing script"
            );
        }
        return new String[] {kind, source, destination};
    }

    private static String readStringField(Object target, String name) {
        for (Class<?> type = target.getClass(); type != null; type = type.getSuperclass()) {
            try {
                Field field = type.getDeclaredField(name);
                field.setAccessible(true);
                Object value = field.get(target);
                return value == null ? null : value.toString();
            } catch (NoSuchFieldException absent) {
                // keep walking the hierarchy
            } catch (ReflectiveOperationException | RuntimeException inaccessible) {
                return null;
            }
        }
        return null;
    }

    private static boolean samePath(String left, Path right) {
        return left != null && Path.of(left).toAbsolutePath().normalize().equals(right.toAbsolutePath().normalize());
    }

    /**
     * GH #115: whether a pending-install command belongs to an earlier agent-doc staging: the
     * delete of the installed plugin directory, an unzip of a staged agent-doc package, or the
     * cleanup delete of such a package.
     */
    static boolean isPriorAgentDocStaging(String[] command, Path pluginDir) {
        String kind = command[0];
        String source = command[1];
        if (kind.equals("UnzipCommand")) {
            return isStagedArchive(source);
        }
        if (kind.equals("DeleteCommand")) {
            return samePath(source, pluginDir) || isStagedArchive(source);
        }
        return false;
    }

    /**
     * GH #115: under the script manager's own monitor (its static mutators are
     * {@code synchronized} on the class, so no other writer in this JVM interleaves), drop every
     * earlier agent-doc staging, append exactly one delete+unzip+cleanup block for {@code staged},
     * save the whole script to a temp file and rename it over the original, then reload and
     * verify the result. Superseded staged packages are deleted afterwards.
     *
     * {@code fallback} installs the block through {@code PluginInstaller.installAfterRestart}
     * when this build's command classes are not constructible; the dedupe still precedes it.
     *
     * @return receipt notes
     */
    static List<String> replaceStagingInActionScript(
        Class<?> scriptManager,
        Path script,
        Path pluginDir,
        Path idePluginsPath,
        Path staged,
        java.util.concurrent.Callable<Object> fallback
    ) throws Exception {
        List<String> notes = new ArrayList<>();
        List<String> superseded = new ArrayList<>();
        synchronized (scriptManager) {
            Method load = scriptManager.getMethod("loadActionScript", Path.class);
            Method save = scriptManager.getMethod("saveActionScript", List.class, Path.class);
            List<Object> kept = new ArrayList<>();
            int removed = 0;
            for (Object command : Files.exists(script) ? (List<?>) load.invoke(null, script) : List.of()) {
                String[] described = describeCommand(command);
                if (isPriorAgentDocStaging(described, pluginDir)) {
                    removed++;
                    if (isStagedArchive(described[1]) && !samePath(described[1], staged)) {
                        superseded.add(described[1]);
                    }
                } else {
                    kept.add(command);
                }
            }
            List<Object> block = stagingBlock(scriptManager, pluginDir, idePluginsPath, staged);
            if (block != null) {
                kept.addAll(block);
                saveActionScriptAtomically(save, kept, script);
            } else {
                if (kept.isEmpty()) {
                    Files.deleteIfExists(script);
                } else {
                    saveActionScriptAtomically(save, kept, script);
                }
                // No nested types here (see JetBrainsPluginUpgradeActionShapeTest): a JDK
                // Callable carries the installAfterRestart fallback.
                fallback.call();
            }
            verifyActionScriptStaging((List<?>) load.invoke(null, script), pluginDir, staged);
            if (removed > 0) {
                notes.add("replaced a prior agent-doc staging (" + removed + " pending-install commands)");
            }
        }
        for (String zip : superseded) {
            try {
                Files.deleteIfExists(Path.of(zip));
            } catch (Exception cleanupFailure) {
                notes.add("could not delete superseded staged package " + zip + ": " + singleLine(cleanupFailure));
            }
        }
        return notes;
    }

    /**
     * delete(plugin dir), unzip(staged, plugins path), delete(staged) -- the block
     * {@code PluginInstaller.installAfterRestart} writes, minus its second delete of the same
     * directory. {@code null} when this build's command classes are not constructible.
     */
    private static List<Object> stagingBlock(Class<?> scriptManager, Path pluginDir, Path idePluginsPath, Path staged) {
        try {
            ClassLoader loader = scriptManager.getClassLoader();
            Class<?> delete = Class.forName(SCRIPT_MANAGER_CLASS + "$DeleteCommand", true, loader);
            Class<?> unzip = Class.forName(SCRIPT_MANAGER_CLASS + "$UnzipCommand", true, loader);
            List<Object> block = new ArrayList<>();
            block.add(delete.getConstructor(Path.class).newInstance(pluginDir));
            block.add(unzip.getConstructor(Path.class, Path.class).newInstance(staged, idePluginsPath));
            block.add(delete.getConstructor(Path.class).newInstance(staged));
            return block;
        } catch (ReflectiveOperationException | RuntimeException | LinkageError unavailable) {
            return null;
        }
    }

    private static void saveActionScriptAtomically(Method save, List<Object> commands, Path script) throws Exception {
        Path parent = script.toAbsolutePath().getParent();
        Files.createDirectories(parent);
        Path temp = Files.createTempFile(parent, script.getFileName().toString(), ".agent-doc.tmp");
        try {
            save.invoke(null, commands, temp);
            Files.move(temp, script, StandardCopyOption.ATOMIC_MOVE, StandardCopyOption.REPLACE_EXISTING);
        } catch (Exception failure) {
            try {
                Files.deleteIfExists(temp);
            } catch (Exception cleanupFailure) {
                failure.addSuppressed(cleanupFailure);
            }
            throw failure instanceof InvocationTargetException && failure.getCause() instanceof Exception
                ? (Exception) failure.getCause()
                : failure;
        }
    }

    /**
     * GH #115: the saved script must hold exactly one agent-doc unzip, of {@code staged}, whose
     * package exists, and no delete of the plugin directory or of {@code staged} ahead of it
     * beyond the block's own single delete.
     */
    static void verifyActionScriptStaging(List<?> commands, Path pluginDir, Path staged) {
        int unzips = 0;
        int pluginDeletesBeforeUnzip = 0;
        boolean unzipSeen = false;
        for (Object command : commands) {
            String[] described = describeCommand(command);
            if (described[0].equals("UnzipCommand") && isStagedArchive(described[1])) {
                unzips++;
                unzipSeen = true;
                if (!samePath(described[1], staged)) {
                    throw new IllegalStateException(
                        "pending-install script still unzips another agent-doc package " + described[1]
                    );
                }
            } else if (described[0].equals("DeleteCommand") && !unzipSeen
                && (samePath(described[1], pluginDir) || samePath(described[1], staged))) {
                pluginDeletesBeforeUnzip++;
                if (samePath(described[1], staged)) {
                    throw new IllegalStateException("pending-install script deletes " + staged + " before unzipping it");
                }
            }
        }
        if (unzips != 1) {
            throw new IllegalStateException(
                "pending-install script holds " + unzips + " agent-doc unzip commands after staging; expected 1"
            );
        }
        if (pluginDeletesBeforeUnzip > 2) {
            throw new IllegalStateException(
                "pending-install script deletes " + pluginDir + " " + pluginDeletesBeforeUnzip + " times before its unzip"
            );
        }
        if (!Files.isRegularFile(staged) || !Files.isReadable(staged)) {
            throw new IllegalStateException("staged package " + staged + " vanished before the script was verified");
        }
    }

    /**
     * The static {@code installAfterRestart} overload whose parameters are a descriptor, two
     * {@link Path}s (source, then existing plugin) and optionally one {@code boolean}, in any
     * order -- the platform has moved the descriptor between first and last across builds.
     */
    static Method findInstallAfterRestart(Class<?> owner, Object descriptor) {
        for (Method candidate : owner.getMethods()) {
            if (!candidate.getName().equals("installAfterRestart")
                || !Modifier.isStatic(candidate.getModifiers())) {
                continue;
            }
            int descriptors = 0;
            int paths = 0;
            int booleans = 0;
            boolean other = false;
            for (Class<?> type : candidate.getParameterTypes()) {
                if (type == Path.class) {
                    paths++;
                } else if (type == boolean.class) {
                    booleans++;
                } else if (type.isInstance(descriptor)) {
                    descriptors++;
                } else {
                    other = true;
                }
            }
            if (!other && descriptors == 1 && paths == 2 && booleans <= 1) {
                return candidate;
            }
        }
        return null;
    }

    /** Arguments for {@link #findInstallAfterRestart}'s method: delete the staged copy after use. */
    static Object[] installAfterRestartArguments(Method install, Object descriptor, Path source, Path existing) {
        Class<?>[] types = install.getParameterTypes();
        Object[] args = new Object[types.length];
        boolean sourceAssigned = false;
        for (int index = 0; index < types.length; index++) {
            if (types[index] == Path.class) {
                args[index] = sourceAssigned ? existing : source;
                sourceAssigned = true;
            } else if (types[index] == boolean.class) {
                args[index] = true;
            } else {
                args[index] = descriptor;
            }
        }
        return args;
    }

    /**
     * Dispose the current plugin generation before asking IntelliJ to detach its descriptor.
     *
     * Newer generations expose one public static hook. The companion fallback upgrades the
     * first generation that shipped dynamic unload but predates that hook; Kotlin's internal
     * method is public bytecode with a module suffix. Failure is closed because loading a
     * replacement beside live old-generation DocumentListeners creates a CRDT echo loop.
     */
    private static int cleanupOutgoingGeneration(IdeaPluginDescriptorImpl descriptor) {
        try {
            ClassLoader loader = descriptor.getPluginClassLoader();
            Class<?> lifecycle = Class.forName(LIFECYCLE_CLASS, true, loader);
            try {
                return (Integer) lifecycle
                    .getMethod("disposeOpenProjectsForDynamicUnload")
                    .invoke(null);
            } catch (NoSuchMethodException legacyGeneration) {
                Field companionField = lifecycle.getField("Companion");
                Object companion = companionField.get(null);
                Method cleanup = null;
                for (Method candidate : companion.getClass().getMethods()) {
                    if (candidate.getName().startsWith("disposeProjectResources$")
                        && candidate.getParameterCount() == 1
                        && candidate.getParameterTypes()[0] == Project.class) {
                        cleanup = candidate;
                        break;
                    }
                }
                if (cleanup == null) {
                    throw legacyGeneration;
                }
                Project[] projects = ProjectManager.getInstance().getOpenProjects();
                int cleaned = 0;
                for (Project project : projects) {
                    if (!project.isDisposed()) {
                        cleanup.invoke(companion, project);
                        cleaned++;
                    }
                }
                return cleaned;
            }
        } catch (ReflectiveOperationException failure) {
            Throwable cause = failure instanceof InvocationTargetException invocation
                && invocation.getCause() != null
                ? invocation.getCause()
                : failure;
            throw new IllegalStateException(
                "outgoing plugin generation could not release open projects: "
                    + cause.getMessage(),
                cause
            );
        }
    }

    /**
     * Ask the replacement generation to rebuild project services and reattach open documents,
     * then return its receipt.
     *
     * `#jbupgradereattach`: this runs only after the upgrade verdict is already decided.
     * {@code runOnEdt} returns {@code ok:} solely on converged bytes -- the package is installed
     * and a fresh descriptor of the expected version owns a new classloader. Open-document CRDT
     * re-registration is a different property, owned by whichever controller owns each document's
     * own project root, which this install does not own. So a shortfall, or even an unreachable
     * receipt, is reported here rather than raised: raising it failed the whole {@code make install}
     * after the replacement had already landed irreversibly, and the retry then correctly reported
     * the package byte-identical with no restart required.
     */
    private static String reattachOpenDocuments(IdeaPluginDescriptorImpl descriptor) {
        try {
            Class<?> lifecycle = Class.forName(
                LIFECYCLE_CLASS,
                true,
                descriptor.getPluginClassLoader()
            );
            return String.valueOf(
                lifecycle.getMethod("initializeOpenProjectsAfterDynamicLoad").invoke(null)
            );
        } catch (Throwable failure) {
            Throwable cause = failure instanceof InvocationTargetException invocation
                && invocation.getCause() != null
                ? invocation.getCause()
                : failure;
            return "documents=0/0:reattach_error=" + singleLine(cause);
        }
    }

    /** Keep a receipt to one line: the launcher reads the status file as a single status. */
    private static String singleLine(Throwable cause) {
        String message = cause.getMessage();
        String text = message == null || message.isBlank() ? cause.getClass().getName() : message;
        return text.replace('\n', ' ').replace('\r', ' ').trim();
    }

    /**
     * Name a failure by its class as well as its message (GH #80).
     *
     * A {@link LinkageError}'s message is only the binary class name in slash form, so wrapping
     * it as {@code IllegalStateException(message)} printed
     * {@code com/intellij/ide/plugins/DynamicPlugins$UnloadPluginOptions} with nothing saying the
     * upgrader failed to link a class rather than the platform refusing an unload. Failures this
     * class raises itself are already {@link IllegalStateException}s with their own wording.
     */
    static String describeFailure(Throwable failure) {
        if (failure instanceof IllegalStateException) {
            return String.valueOf(failure.getMessage());
        }
        return UPGRADER_FAILED + ": " + failure.getClass().getName() + ": " + singleLine(failure);
    }

    /*
     * No nested, inner, or anonymous classes in this file -- only lambdas, which compile into
     * this class. An attached agent jar is appended to the IDE system class path once per JVM,
     * so a nested class of this action resolves from the FIRST jar ever attached, whatever the
     * bootstrap's child-first loader does; a nested class from an older plugin then fails with
     * IllegalAccessError and every restart-free upgrade stages instead. Callbacks are JDK
     * functional types for the same reason. JetBrainsPluginUpgradeActionShapeTest pins it.
     */

    /**
     * Unload the outgoing generation, releasing its projects only once the unload is known to be
     * callable (GH #94).
     *
     * The release has to precede the unload itself -- live old-generation DocumentListeners
     * beside the replacement's create a CRDT echo loop -- but it must not precede anything that
     * can still abort the upgrade for a reason knowable up front. It used to run before the
     * {@code UnloadPluginOptions} lookup, so a build without that class (GH #80) tore down the
     * replica transport and then aborted, leaving a live plugin serving no replica. A failure
     * after the release is the caller's to repair via {@link #recoverAbortedUnload}.
     */
    static int unloadOutgoingGeneration(
        Supplier<Object> unloadVerdict,
        Supplier<Supplier<Object>> resolveUnload,
        IntSupplier releaseOpenProjects
    ) {
        String unloadBlocker = unloadBlockerReason(unloadVerdict.get());
        if (unloadBlocker != null) {
            throw new IllegalStateException(unloadBlocker);
        }
        Supplier<Object> unload = resolveUnload.get();
        int cleanedProjects = releaseOpenProjects.getAsInt();
        if (!Boolean.TRUE.equals(unload.get())) {
            throw new IllegalStateException(
                DYNAMIC_UNLOAD_REFUSED + ": JetBrains refused to unload the current plugin generation after cleaning "
                    + cleanedProjects + " open project(s)"
            );
        }
        return cleanedProjects;
    }

    /**
     * Resolve the update-mode {@code DynamicPlugins.unloadPlugin} for {@code descriptor} from the
     * signatures {@code owner} actually exposes (GH #80).
     *
     * Through 261 the method is {@code unloadPlugin(descriptor, UnloadPluginOptions)}, and the
     * options type is taken from that parameter -- its declaring loader always sees it, so no
     * classloader has to be guessed. From 263 the nested {@code UnloadPluginOptions} no longer
     * exists: the platform reconfigures to a computed plugin state and {@code unloadPlugin}
     * takes the descriptor alone, with no disable flag to clear. Looking the class up by name
     * there could only fail ("not visible ... from PathClassLoader, PluginClassLoader"), which
     * is what every upgrade on IU-263.6259.32 hit. The options overload is preferred whenever it
     * exists, because an older build's one-argument default disables the plugin.
     */
    static Supplier<Object> resolveUpdateUnload(Class<?> owner, Object receiver, Object descriptor) {
        Method withOptions = null;
        Method descriptorOnly = null;
        for (Method candidate : owner.getMethods()) {
            Class<?>[] types = candidate.getParameterTypes();
            if (!candidate.getName().equals("unloadPlugin")
                || Modifier.isStatic(candidate.getModifiers())
                || types.length == 0
                || !types[0].isInstance(descriptor)) {
                continue;
            }
            if (types.length == 2 && types[1].getSimpleName().equals("UnloadPluginOptions")) {
                withOptions = candidate;
            } else if (types.length == 1) {
                descriptorOnly = candidate;
            }
        }
        if (withOptions != null) {
            Object options = updateUnloadOptions(withOptions.getParameterTypes()[1]);
            return () -> invokeDescriptorMethod(owner, receiver, "unloadPlugin", descriptor, options);
        }
        if (descriptorOnly != null) {
            return () -> invokeDescriptorMethod(owner, receiver, "unloadPlugin", descriptor);
        }
        throw new IllegalStateException(
            UPGRADER_FAILED + ": DynamicPlugins.unloadPlugin has no signature accepting "
                + descriptor.getClass().getName() + " on build " + platformBuild()
                + "; found: " + signaturesNamed(owner, "unloadPlugin")
        );
    }

    /** Build update-mode options ({@code disable=false}, {@code update=true}) of {@code options}. */
    static Object updateUnloadOptions(Class<?> options) {
        try {
            Object value = options.getConstructor().newInstance();
            value = options.getMethod("withDisable", boolean.class).invoke(value, false);
            return options.getMethod("withUpdate", boolean.class).invoke(value, true);
        } catch (ReflectiveOperationException | LinkageError shape) {
            Throwable cause = shape instanceof InvocationTargetException invocation
                && invocation.getCause() != null
                ? invocation.getCause()
                : shape;
            throw new IllegalStateException(
                UPGRADER_FAILED + ": " + options.getName() + " has an unexpected shape on build "
                    + platformBuild() + ": " + singleLine(cause),
                cause
            );
        }
    }

    /**
     * Stable prefix for an upgrade agent-doc could not perform -- a platform API it could not
     * link or call -- as opposed to {@link #DYNAMIC_UNLOAD_REFUSED}, the platform declining.
     * The launcher keys on both to word the restart message (GH #80).
     */
    static final String UPGRADER_FAILED = "agent-doc upgrader could not call the platform";

    /**
     * Call a {@code DynamicPlugins} method whose descriptor parameter type moves across platform
     * builds (`#jbunloadsig`, GH #63 layer 2).
     *
     * A compile-time call binds the exact descriptor type of the build the plugin compiled
     * against; IU-262.9437.185 no longer exposes {@code checkCanUnloadWithoutRestart
     * (IdeaPluginDescriptorImpl)}, so the upgrade died with {@code NoSuchMethodError} before it
     * unloaded anything. Resolve by name at runtime instead: a public instance overload whose
     * first parameter accepts the live descriptor and whose remaining parameters accept
     * {@code trailing}, else (with no trailing arguments) the Kotlin {@code name$default} bridge
     * with every optional parameter defaulted. A miss names the build and every signature found,
     * so the next platform move is diagnosable from the upgrade receipt alone.
     */
    static Object invokeDescriptorMethod(
        Class<?> owner,
        Object receiver,
        String name,
        Object descriptor,
        Object... trailing
    ) {
        try {
            Method direct = findDirectOverload(owner, name, descriptor, trailing);
            if (direct != null) {
                Object[] args = new Object[trailing.length + 1];
                args[0] = descriptor;
                System.arraycopy(trailing, 0, args, 1, trailing.length);
                return direct.invoke(receiver, args);
            }
            Method bridge = trailing.length == 0 ? findDefaultBridge(owner, name, descriptor) : null;
            if (bridge != null) {
                return bridge.invoke(null, defaultBridgeArguments(bridge, receiver, descriptor));
            }
        } catch (IllegalAccessException failure) {
            throw new IllegalStateException(
                "DynamicPlugins." + name + " is not accessible: " + failure.getMessage(), failure
            );
        } catch (InvocationTargetException failure) {
            Throwable cause = failure.getCause() == null ? failure : failure.getCause();
            throw new IllegalStateException(
                "DynamicPlugins." + name + " failed: " + singleLine(cause), cause
            );
        }
        throw new IllegalStateException(
            "DynamicPlugins." + name + " has no signature accepting "
                + descriptor.getClass().getName() + " on build " + platformBuild()
                + "; found: " + signaturesNamed(owner, name)
        );
    }

    private static Method findDirectOverload(
        Class<?> owner,
        String name,
        Object descriptor,
        Object[] trailing
    ) {
        for (Method candidate : owner.getMethods()) {
            Class<?>[] types = candidate.getParameterTypes();
            if (!candidate.getName().equals(name)
                || Modifier.isStatic(candidate.getModifiers())
                || types.length != trailing.length + 1
                || !types[0].isInstance(descriptor)) {
                continue;
            }
            boolean accepts = true;
            for (int index = 0; index < trailing.length; index++) {
                if (trailing[index] != null && !types[index + 1].isInstance(trailing[index])) {
                    accepts = false;
                    break;
                }
            }
            if (accepts) {
                return candidate;
            }
        }
        return null;
    }

    /** Kotlin's bridge: {@code static name$default(Owner, P0, P1..Pn, int mask, Object)}. */
    private static Method findDefaultBridge(Class<?> owner, String name, Object descriptor) {
        for (Method candidate : owner.getDeclaredMethods()) {
            Class<?>[] types = candidate.getParameterTypes();
            if (candidate.getName().equals(name + "$default")
                && Modifier.isStatic(candidate.getModifiers())
                && types.length >= 4
                && types[0] == owner
                && types[1].isInstance(descriptor)
                && types[types.length - 2] == int.class
                && types[types.length - 1] == Object.class) {
                candidate.setAccessible(true);
                return candidate;
            }
        }
        return null;
    }

    private static Object[] defaultBridgeArguments(Method bridge, Object receiver, Object descriptor) {
        Class<?>[] types = bridge.getParameterTypes();
        int declared = types.length - 3;
        Object[] args = new Object[types.length];
        args[0] = receiver;
        args[1] = descriptor;
        int mask = 0;
        for (int index = 1; index < declared; index++) {
            args[index + 1] = zeroValue(types[index + 1]);
            mask |= 1 << index;
        }
        args[types.length - 2] = mask;
        args[types.length - 1] = null;
        return args;
    }

    private static Object zeroValue(Class<?> type) {
        if (!type.isPrimitive()) {
            return null;
        }
        if (type == boolean.class) {
            return false;
        }
        if (type == char.class) {
            return '\0';
        }
        if (type == long.class) {
            return 0L;
        }
        if (type == float.class) {
            return 0f;
        }
        if (type == double.class) {
            return 0d;
        }
        if (type == byte.class) {
            return (byte) 0;
        }
        if (type == short.class) {
            return (short) 0;
        }
        return 0;
    }

    static String signaturesNamed(Class<?> owner, String name) {
        List<String> found = new ArrayList<>();
        for (Method candidate : owner.getDeclaredMethods()) {
            if (candidate.getName().equals(name) || candidate.getName().equals(name + "$default")) {
                StringBuilder signature = new StringBuilder(candidate.getName()).append('(');
                Class<?>[] types = candidate.getParameterTypes();
                for (int index = 0; index < types.length; index++) {
                    signature.append(index == 0 ? "" : ",").append(types[index].getSimpleName());
                }
                found.add(signature.append(')').toString());
            }
        }
        Collections.sort(found);
        return found.isEmpty() ? "none" : String.join(" ", found);
    }

    private static String platformBuild() {
        try {
            return ApplicationInfo.getInstance().getBuild().asString();
        } catch (Throwable unavailable) {
            return "unknown";
        }
    }

    /**
     * Normalize {@code checkCanUnloadWithoutRestart}'s verdict across platform builds (GH #67).
     * Older builds return a nullable blocker reason; newer ones return whether the plugin CAN
     * unload. Treating any non-null result as a blocker printed "plugin cannot unload
     * dynamically: false", and would have refused a {@code true} verdict outright.
     */
    static String unloadBlockerReason(Object verdict) {
        if (verdict == null || Boolean.TRUE.equals(verdict)) {
            return null;
        }
        if (Boolean.FALSE.equals(verdict)) {
            return DYNAMIC_UNLOAD_REFUSED
                + " (the platform reported the plugin cannot unload without a restart)";
        }
        String reason = verdict.toString().trim();
        return reason.isEmpty()
            ? DYNAMIC_UNLOAD_REFUSED
            : DYNAMIC_UNLOAD_REFUSED + ": " + reason;
    }

    /**
     * Newer JetBrains builds load the replacement before checking whether the outgoing
     * classloader was actually collected. Their async strategy always returns success to the
     * reconfiguration caller, then raises a restart notification up to twenty seconds later.
     * There is therefore no synchronous retirement receipt on which agent-doc can safely swap
     * the live jars. Detect that strategy before even probing unloadability and stage instead.
     *
     * <p>GH #108: this is agent-doc declining, not the platform refusing -- the IDE is never
     * asked. The reason therefore carries {@link #DYNAMIC_UPGRADE_DECLINED}, never
     * {@link #DYNAMIC_UNLOAD_REFUSED}, and says the loss is permanent for this build so an
     * operator stops retrying the restart-free path.
     */
    static String dynamicUpgradeBlockerReason(boolean asyncPostReconfigurationAwait) {
        if (!asyncPostReconfigurationAwait) {
            return null;
        }
        return DYNAMIC_UPGRADE_DECLINED
            + ": this JetBrains build retires plugin classloaders asynchronously ("
            + ASYNC_CLASSLOADER_AWAIT_STRATEGY.substring(ASYNC_CLASSLOADER_AWAIT_STRATEGY.lastIndexOf('.') + 1)
            + "), so there is no safe synchronous swap point; restart-free upgrade is permanently "
            + "unavailable on this build, and the update was staged before touching the live plugin generation";
    }

    private static boolean asyncPostReconfigurationClassloaderAwaitIsPresent(ClassLoader platformLoader) {
        try {
            Class.forName(ASYNC_CLASSLOADER_AWAIT_STRATEGY, false, platformLoader);
            return true;
        } catch (ClassNotFoundException synchronousPlatform) {
            return false;
        }
    }

    /** Stable prefix the launcher and preflight key on to record a refused dynamic unload. */
    static final String DYNAMIC_UNLOAD_REFUSED = "plugin cannot unload dynamically";

    /**
     * GH #108: stable prefix for agent-doc's own decision not to attempt the restart-free
     * upgrade. The launcher keys on it to attribute the refusal to agent-doc, not the IDE.
     */
    static final String DYNAMIC_UPGRADE_DECLINED = "agent-doc declined the restart-free upgrade";

    static boolean sameInstallRoot(Path actual, Path expected) {
        return actual.toAbsolutePath().normalize().equals(expected.toAbsolutePath().normalize());
    }

    private static boolean isLoaded(IdeaPluginDescriptorImpl descriptor) {
        try {
            Method method = PluginManagerCore.class.getMethod("isLoaded", PluginDescriptor.class);
            return (Boolean) method.invoke(null, descriptor);
        } catch (ReflectiveOperationException unavailableOnOlderPlatform) {
            return PluginManagerCore.getLoadedPlugins().stream()
                .anyMatch(loaded -> loaded.getPluginId().equals(descriptor.getPluginId()));
        }
    }

}
