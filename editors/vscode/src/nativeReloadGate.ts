export interface NativeReloadLease {
    readonly owner: boolean;
    readonly completion: Promise<boolean>;
    complete(succeeded: boolean): void;
}

interface ActiveNativeReload {
    readonly completion: Promise<boolean>;
    readonly resolve: (succeeded: boolean) => void;
}

/**
 * Event-driven handoff gate for the native library reload lifecycle.
 *
 * The first reload intent owns the handoff. Later intents coalesce onto its
 * completion event, while user actions can wait for the same event with a
 * bounded presentation timeout. No polling lane is involved.
 */
export class NativeReloadGate {
    private active: ActiveNativeReload | undefined;

    begin(): NativeReloadLease {
        const existing = this.active;
        if (existing) {
            return {
                owner: false,
                completion: existing.completion,
                complete: () => undefined,
            };
        }

        let resolve!: (succeeded: boolean) => void;
        const completion = new Promise<boolean>((complete) => {
            resolve = complete;
        });
        const active = { completion, resolve };
        this.active = active;
        return {
            owner: true,
            completion,
            complete: (succeeded: boolean) => {
                if (this.active !== active) return;
                this.active = undefined;
                active.resolve(succeeded);
            },
        };
    }

    async awaitReady(timeoutMs: number): Promise<boolean> {
        const active = this.active;
        if (!active) return true;

        let timer: ReturnType<typeof setTimeout> | undefined;
        try {
            return await Promise.race([
                active.completion.then(() => true),
                new Promise<boolean>((resolve) => {
                    timer = setTimeout(() => resolve(false), timeoutMs);
                }),
            ]);
        } finally {
            if (timer) clearTimeout(timer);
        }
    }
}
