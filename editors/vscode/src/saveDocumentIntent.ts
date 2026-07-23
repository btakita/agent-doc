export type SaveDocumentStatus = 'missing_file' | 'missing_document' | 'saved' | 'failed';

export interface SaveDocumentHandle {
    save(): Thenable<boolean>;
    getText(): string;
}

export interface SaveDocumentIntentEffects {
    fileExists(filePath: string): boolean;
    findOpenDocument(filePath: string): SaveDocumentHandle | undefined;
    publishSavedContent(filePath: string, content: string): boolean;
    observeSavedContent(document: SaveDocumentHandle): void;
    recordOutcome(filePath: string, status: SaveDocumentStatus): void;
    reportFailure(filePath: string, error: unknown): void;
}

/**
 * Execute the typed editor-owned `save_document` intent.
 *
 * The content receipt is published only after save succeeds, and the `saved`
 * surface event is recorded only after that receipt and the current-content
 * observation both complete.
 */
export async function processSaveDocumentIntent(
    filePath: string | undefined,
    effects: SaveDocumentIntentEffects,
): Promise<number> {
    if (!filePath || !effects.fileExists(filePath)) {
        effects.recordOutcome(filePath ?? '.', 'missing_file');
        return 0;
    }
    const document = effects.findOpenDocument(filePath);
    if (!document) {
        effects.recordOutcome(filePath, 'missing_document');
        return 0;
    }
    try {
        if (!await document.save()) {
            effects.recordOutcome(filePath, 'failed');
            return 0;
        }
        if (!effects.publishSavedContent(filePath, document.getText())) {
            effects.recordOutcome(filePath, 'failed');
            return 0;
        }
        effects.observeSavedContent(document);
        effects.recordOutcome(filePath, 'saved');
        return 1;
    } catch (error: unknown) {
        effects.reportFailure(filePath, error);
        effects.recordOutcome(filePath, 'failed');
        return 0;
    }
}
