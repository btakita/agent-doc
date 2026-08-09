//! `#stalereexecstarve` — the idle-queue watch's drain must never exit the TICK.
//!
//! The watch does two independent jobs per tick: drain the go-mode queue, and
//! decide whether a stale supervisor should hot-reload onto the freshly-installed
//! binary. The recycle decision is sequenced *after* the drain, so a drain-scoped
//! `continue` skips it — including the staleness publication and the
//! `⚠ STALE SUPERVISOR` pane marker, which is why the failure produced no
//! diagnostic at all.
//!
//! Observed live 2026-08-09: `src/boost-client/tasks/monsterrodholders.md`'s
//! supervisor (PID 4069526) ran four days on a deleted binary image because its
//! queue authority stayed unavailable, so every tick took the
//! `QueueHeadObservation::AuthorityUnavailable` early exit. 227 CP recycle
//! requests were written and none consumed; zero `supervisor_binary_stale_*`
//! lines were emitted for the entire window.
//!
//! The fix wraps the drain in a `'drain:` labeled block so its early exits leave
//! the DRAIN, not the tick. This guard keeps it that way: a future `continue`
//! added inside that block would silently restore the starvation and no behavioral
//! test would necessarily cover the specific new bail-out path.
//!
//! The behavioral half lives in SimWorld
//! (`stale_supervisor_recycles_even_when_the_drain_bails_early`).

const SOURCE: &str = include_str!("../src/idle_watch.rs");

/// Byte offsets of the drain block: the `'drain: {` label through the
/// `drain_completed = true;` statement that closes it.
fn drain_block_span() -> (usize, usize) {
    let start = SOURCE
        .find("'drain: {")
        .expect("idle_watch.rs must wrap the queue drain in a `'drain:` labeled block so a \
                 drain-scoped early exit cannot skip the stale-binary recycle decision \
                 (#stalereexecstarve)");
    let end = SOURCE[start..]
        .find("drain_completed = true;")
        .map(|offset| start + offset)
        .expect("the `'drain:` block must close by setting `drain_completed = true;` — that \
                 marker is both the block terminator and the signal the recycle fallthrough \
                 reads (#stalereexecstarve)");
    (start, end)
}

#[test]
fn drain_early_exits_leave_the_drain_not_the_tick() {
    let (start, end) = drain_block_span();
    let block = &SOURCE[start..end];

    // `rustc` already rejects a BARE `continue` inside a labeled block (E0695), so
    // that half of the regression cannot even compile. This guard covers the half
    // the compiler accepts: a `continue 'some_label` aimed past the drain block,
    // which would restore the starvation silently.
    let offenders: Vec<(usize, &str)> = block
        .lines()
        .enumerate()
        .filter(|(_, line)| {
            let code = line.split("//").next().unwrap_or(line).trim();
            let stmt = code.strip_suffix(&[';', ','][..]).unwrap_or(code);
            stmt == "continue"
                || stmt.starts_with("continue '")
                || stmt.ends_with("=> continue")
                || stmt.contains("=> continue '")
        })
        .map(|(index, line)| (index, line.trim()))
        .collect();

    assert!(
        offenders.is_empty(),
        "the queue drain must exit with `break 'drain`, never `continue`: a `continue` returns \
         to the top of the tick and skips the stale-binary recycle decision below, which is how \
         a supervisor ends up running a deleted binary indefinitely (#stalereexecstarve). \
         Offending lines (relative to the `'drain:` label): {offenders:?}"
    );
}

#[test]
fn the_recycle_decision_is_sequenced_after_the_drain_block() {
    // The guard above is only meaningful while the recycle decision still lives
    // after the drain. If someone reorders them, `break 'drain` stops being the
    // thing that protects the recycle and this guard would pass vacuously.
    let (_, end) = drain_block_span();
    let recycle = SOURCE
        .find("let supervisor_stale = supervisor_stale_fast;")
        .expect("idle_watch.rs must still resolve `supervisor_stale` per tick");
    assert!(
        recycle > end,
        "the stale-binary recycle decision must remain sequenced AFTER the `'drain:` block — \
         that ordering is what makes `break 'drain` (rather than `continue`) the load-bearing \
         detail (#stalereexecstarve)"
    );
}

#[test]
fn the_fallthrough_emits_an_observable_receipt() {
    // The original defect was invisible: no ops-log line proved the decision was
    // skipped. Keep a receipt so the guard can be verified from a live log rather
    // than only from source.
    assert!(
        SOURCE.contains("supervisor_stale_recycle_drain_fallthrough"),
        "the drain fallthrough must log `supervisor_stale_recycle_drain_fallthrough` when a \
         stale supervisor reaches the recycle decision through an early-exited drain, so the \
         guard is provable from ops.log (#stalereexecstarve)"
    );
}
