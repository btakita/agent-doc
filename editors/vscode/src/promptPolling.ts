export interface PromptOption {
    index: number;
    label: string;
}

export interface PromptInfo {
    active: boolean;
    question?: string;
    options?: PromptOption[];
    selected?: number;
}

export interface PromptAllEntry {
    session_id: string;
    file: string;
    cwd?: string;
    info?: PromptInfo;
    active?: boolean;
    question?: string;
    options?: PromptOption[];
    selected?: number;
}

export interface NormalizedPromptEntry {
    file: string;
    cwd?: string;
    key: string;
    info: PromptInfo;
}

export interface PromptQuickPickItem {
    label: string;
    optionIndex: number;
    answerIndex: number;
    picked?: boolean;
}

export function normalizePromptEntries(entries: PromptAllEntry[]): NormalizedPromptEntry[] {
    const normalized: NormalizedPromptEntry[] = [];
    for (const entry of entries) {
        const info: PromptInfo = entry.info ?? {
            active: entry.active ?? false,
            question: entry.question,
            options: entry.options,
            selected: entry.selected,
        };
        if (!info.active || !info.options || info.options.length === 0) continue;
        normalized.push({
            file: entry.file,
            cwd: entry.cwd,
            key: `${entry.cwd ?? ''}:${entry.file}:${info.question}`,
            info,
        });
    }
    return normalized;
}

export function buildPromptQuickPickItems(
    options: PromptOption[],
    selected?: number,
): PromptQuickPickItem[] {
    return options.map((option, ordinal) => ({
        label: `[${option.index}] ${option.label}`,
        optionIndex: option.index,
        answerIndex: ordinal + 1,
        picked: selected === ordinal,
    }));
}
