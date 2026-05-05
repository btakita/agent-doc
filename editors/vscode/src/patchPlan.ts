export interface RepositionPatchShape {
    reposition_boundary?: boolean;
    patches?: unknown[];
    normalize_prefix_lines?: string[];
    unmatched?: string;
    frontmatter?: string;
    fullContent?: string;
}

export function isPureRepositionSignal(patch: RepositionPatchShape): boolean {
    if (!patch.reposition_boundary) {
        return false;
    }
    if ((patch.patches?.length ?? 0) > 0) {
        return false;
    }
    if ((patch.normalize_prefix_lines?.length ?? 0) > 0) {
        return false;
    }
    if ((patch.unmatched ?? '').trim() !== '') {
        return false;
    }
    if ((patch.frontmatter ?? '').trim() !== '') {
        return false;
    }
    if ((patch.fullContent ?? '') !== '') {
        return false;
    }
    return true;
}
