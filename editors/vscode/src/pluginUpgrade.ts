/**
 * Stale-extension detection (`#vscodeupgradelayout`, `editors/SPEC.md` § 13).
 *
 * `agent-doc plugin install vscode` (or any `--install-extension`) writes a new
 * VSIX generation into the extensions directory while this extension host keeps
 * running the generation it activated. VS Code records the installed set in that
 * directory's `extensions.json` and lists directories it will delete in
 * `.obsolete`. The extension host that loaded this extension is the only process
 * that knows which extensions directory it came from (the local
 * `~/.vscode/extensions`, a Remote-SSH `~/.vscode-server/extensions`, a
 * code-server directory), so the comparison lives here rather than in
 * `installed_plugin_version`, which only reaches JetBrains installs.
 *
 * The remedy is the host's own reload. Since VS Code 1.88 a local window can
 * restart extensions instead of reloading, through the Restart Extensions
 * action VS Code shows on the updated extension; a remote connection still
 * needs Reload Window (1.88 release notes). These are pure functions; the
 * extension supplies the file contents and runs the chosen command.
 *
 * Mirrors the JetBrains `PluginRestartRequiredNotifier` verdict: any installed
 * version other than the running one (including a downgrade) needs the reload.
 */

export const AGENT_DOC_EXTENSION_ID = 'btakita.agent-doc';

/** The installed generation that differs from the running one. */
export interface StaleExtension {
    running: string;
    installed: string;
    /** The installed generation's directory, when the manifest names one. */
    location: string | null;
}

export interface InstalledExtension {
    version: string;
    location: string | null;
}

export interface UpgradePromptAction {
    label: string;
    command: string;
    args: unknown[];
}

export interface UpgradePrompt {
    message: string;
    /** `restart_extensions` locally, `reload_window` for a remote or web host. */
    remedy: 'restart_extensions' | 'reload_window';
    actions: UpgradePromptAction[];
}

function numericDotVersion(version: string): number[] | null {
    if (!version) return null;
    const parts = version.split('.');
    if (parts.some((part) => part.length === 0 || !/^\d+$/.test(part))) return null;
    return parts.map((part) => Number(part));
}

function compareDotVersions(left: number[], right: number[]): number {
    for (let index = 0; index < Math.max(left.length, right.length); index += 1) {
        const difference = (left[index] ?? 0) - (right[index] ?? 0);
        if (difference !== 0) return difference;
    }
    return 0;
}

function entryLocation(entry: any): string | null {
    const location = entry?.location;
    if (typeof location === 'string') return location;
    if (location && typeof location === 'object') {
        if (typeof location.fsPath === 'string') return location.fsPath;
        if (typeof location.path === 'string') return location.path;
    }
    return null;
}

function entryDirectoryName(entry: any): string | null {
    if (typeof entry?.relativeLocation === 'string' && entry.relativeLocation) {
        return entry.relativeLocation;
    }
    const location = entryLocation(entry);
    if (!location) return null;
    const trimmed = location.replace(/[\\/]+$/, '');
    const slash = Math.max(trimmed.lastIndexOf('/'), trimmed.lastIndexOf('\\'));
    return slash >= 0 ? trimmed.slice(slash + 1) : trimmed;
}

/** Directory names VS Code's `.obsolete` file marks for deletion. */
export function obsoleteDirectories(obsoleteJson: string | null | undefined): Set<string> {
    if (!obsoleteJson) return new Set();
    try {
        const parsed = JSON.parse(obsoleteJson);
        if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) return new Set();
        return new Set(Object.entries(parsed).filter(([, value]) => value === true).map(([key]) => key));
    } catch {
        return new Set();
    }
}

/**
 * The installed generation of `extensionId` that `extensions.json` lists, not
 * counting a directory `.obsolete` marks for deletion. Several entries (a
 * half-written manifest) resolve to the newest numeric version.
 */
export function installedExtension(
    manifestJson: string | null | undefined,
    extensionId: string,
    obsoleteJson?: string | null,
): InstalledExtension | null {
    if (!manifestJson) return null;
    let entries: unknown;
    try {
        entries = JSON.parse(manifestJson);
    } catch {
        return null;
    }
    if (!Array.isArray(entries)) return null;
    const obsolete = obsoleteDirectories(obsoleteJson);
    const wanted = extensionId.toLowerCase();
    let best: { installed: InstalledExtension; parts: number[] } | null = null;
    for (const entry of entries as any[]) {
        const id = entry?.identifier?.id;
        const version = entry?.version;
        if (typeof id !== 'string' || id.toLowerCase() !== wanted) continue;
        if (typeof version !== 'string') continue;
        const parts = numericDotVersion(version);
        if (!parts) continue;
        const directory = entryDirectoryName(entry);
        if (directory && obsolete.has(directory)) continue;
        if (!best || compareDotVersions(parts, best.parts) > 0) {
            best = { installed: { version, location: entryLocation(entry) }, parts };
        }
    }
    return best?.installed ?? null;
}

/** A reload is needed when the installed generation is not the running one. */
export function staleExtensionVerdict(
    running: string | null | undefined,
    installed: InstalledExtension | null,
): StaleExtension | null {
    const loaded = running?.trim();
    if (!loaded || loaded === 'unknown' || !installed) return null;
    const pending = installed.version.trim();
    if (!pending || pending === loaded) return null;
    return { running: loaded, installed: pending, location: installed.location };
}

/**
 * Whether this extension host can restart extensions in place. A remote
 * (`vscode.env.remoteName`, e.g. `ssh-remote` for Coder Remote-SSH) or web
 * (code-server, VS Code Web) host needs Reload Window.
 */
export function needsWindowReload(remoteName: string | null | undefined, webUi: boolean): boolean {
    return webUi || (typeof remoteName === 'string' && remoteName.length > 0);
}

/** The operator prompt for a stale generation, offering the host's own reload. */
export function upgradePrompt(
    verdict: StaleExtension,
    host: { remoteName?: string | null; webUi?: boolean; extensionId?: string },
): UpgradePrompt {
    const extensionId = host.extensionId ?? AGENT_DOC_EXTENSION_ID;
    const reloadWindow: UpgradePromptAction = {
        label: 'Reload Window',
        command: 'workbench.action.reloadWindow',
        args: [],
    };
    const pending = `Agent Doc v${verdict.installed} is installed`;
    if (needsWindowReload(host.remoteName, host.webUi === true)) {
        const where = host.remoteName ? ` on the ${host.remoteName} host` : '';
        return {
            message: `${pending}${where}, but this window is still running v${verdict.running}. `
                + 'A remote extension host loads an updated extension only after Reload Window.',
            remedy: 'reload_window',
            actions: [reloadWindow],
        };
    }
    return {
        message: `${pending}, but this window is still running v${verdict.running}. `
            + 'Restart Extensions (on the Agent Doc extension page) loads it without reloading the window.',
        remedy: 'restart_extensions',
        actions: [
            // VS Code exposes Restart Extensions only as the updated extension's
            // own action (`updateRunningExtensions`); the Developer "Restart
            // Extension Host" command restarts the stale generation.
            { label: 'Restart Extensions', command: 'extension.open', args: [extensionId] },
            reloadWindow,
        ],
    };
}
