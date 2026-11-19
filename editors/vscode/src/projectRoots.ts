import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';

function canonical(dir: string): string {
    try {
        return fs.realpathSync.native(dir);
    } catch {
        return path.resolve(dir);
    }
}

function hasProjectStore(dir: string): boolean {
    try {
        return fs.statSync(path.join(dir, '.agent-doc'), { throwIfNoEntry: false })?.isDirectory() ?? false;
    } catch {
        return false;
    }
}

function contains(parent: string, child: string): boolean {
    const relative = path.relative(parent, child);
    return relative !== '..' && !relative.startsWith(`..${path.sep}`) && !path.isAbsolute(relative);
}

/** Local policy used before any native version assumes root-discovery parity. */
export class ProjectRoots {
    private readonly home: string | undefined;
    private readonly temp: Set<string>;
    private readonly ceilings: Set<string>;

    constructor(home: string | undefined, temp: string[], ceilings: string[], private readonly allowHome: boolean) {
        this.home = home && canonical(home);
        this.temp = new Set(temp.map(canonical));
        this.ceilings = new Set(ceilings.flatMap((dir) => [path.resolve(dir), canonical(dir)]));
    }

    allows(dir: string): boolean {
        const resolved = canonical(dir);
        return path.dirname(resolved) !== resolved && !this.temp.has(resolved)
            && (resolved !== this.home || this.allowHome || fs.existsSync(path.join(resolved, '.git')));
    }

    find(start: string): string | undefined {
        const first = path.resolve(start);
        let dir = first;
        for (;;) {
            const atCeiling = this.ceilings.size > 0
                && (this.ceilings.has(dir) || this.ceilings.has(canonical(dir)));
            if (atCeiling && dir !== first) return undefined;
            if (hasProjectStore(dir) && this.allows(dir)) return dir;
            const parent = path.dirname(dir);
            if (atCeiling || parent === dir) return undefined;
            dir = parent;
        }
    }

    allowsFallback(start: string, candidate: string): boolean {
        if (!this.allows(candidate)) return false;
        if (this.ceilings.size === 0) return true;
        const first = canonical(start);
        const root = canonical(candidate);
        for (const ceiling of this.ceilings) {
            if (contains(ceiling, first) && (!contains(ceiling, root) || (root === ceiling && first !== ceiling))) {
                return false;
            }
        }
        return true;
    }
}

export const projectRoots = new ProjectRoots(
    [process.env.HOME, process.env.USERPROFILE, os.homedir()].find((dir) => dir && path.isAbsolute(dir)),
    [os.tmpdir(), ...(process.platform === 'win32' ? [] : ['/tmp', '/var/tmp'])],
    (process.env.AGENT_DOC_ROOT_CEILING_DIRECTORIES ?? '').split(path.delimiter).filter(path.isAbsolute),
    process.env.AGENT_DOC_ALLOW_HOME_ROOT === '1',
);
