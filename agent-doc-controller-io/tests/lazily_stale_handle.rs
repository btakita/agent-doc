use lazily::ThreadSafeContext;

/// Agent-doc keeps effect handles across controller-owned lifecycle maps. A
/// retired handle must stay fenced after Lazily recycles its arena slot for a
/// successor of the same node kind.
#[test]
fn stale_effect_handle_cannot_retire_recycled_successor() {
    let ctx = ThreadSafeContext::new();
    let stale = ctx.effect(|_| {});
    ctx.dispose_effect(&stale);

    let successor = ctx.effect(|_| {});
    assert!(ctx.is_effect_active(&successor));

    ctx.dispose_effect(&stale);
    assert!(
        ctx.is_effect_active(&successor),
        "stale disposal must not retire the successor generation"
    );
}
