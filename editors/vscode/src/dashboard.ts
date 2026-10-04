import * as path from 'path';

/**
 * `gvqv`: the Agent Doc dashboard projection. `agent-doc dashboard --write`
 * renders the fleet work board plus controller/supervisor liveness into this
 * project-root-relative file and arms the project controller to keep it
 * current. The projection is never a session document.
 */
export const DASHBOARD_RELATIVE_PATH = path.join('.agent-doc', 'dashboard.md');

export const DASHBOARD_COMMAND_ARGS: readonly string[] = ['dashboard', '--write'];

export function dashboardPath(projectRoot: string): string {
    return path.join(projectRoot, DASHBOARD_RELATIVE_PATH);
}
