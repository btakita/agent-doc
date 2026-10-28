//! Post-layout pane-order enforcement (`#tmuxpanesreversed`).
//!
//! The effectful half of [`agent_doc_tmux::pane_order`]: after a layout
//! operation, re-read the window's pane geometry, check the invariant
//! (wanted panes left to right by `pane_left` equal the wanted order), repair
//! a complete-but-misordered window with the pure `swap-pane` plan, re-read,
//! and report one diagnosable line either way.
//!
//! Geometry is read with a direct spawn, never through the command-scoped
//! observation cache: tmux-router mutates tmux through its own command
//! builder, which that cache cannot see, so a cached `list-panes` answer
//! after a layout operation would be the layout from BEFORE it. A repair's
//! own swaps also bypass the adapters, so they invalidate the cache with
//! [`crate::observation_cache::observe_external_mutation`].

use agent_doc_tmux::pane_order::{
    PANE_ORDER_FORMAT, PaneOrderSnapshot, PaneOrderVerdict, PaneSwap, active_pane,
    check_pane_order, parse_pane_order_snapshot, plan_pane_order_repair, render_positions,
    single_row_has_starved_pane,
};

/// Read one window's pane geometry and host facts. `None` when tmux does not
/// answer (dead window, server gone).
pub fn observe_pane_order(tmux: &tmux_router::Tmux, window: &str) -> Option<PaneOrderSnapshot> {
    let output = tmux
        .cmd()
        .args(["list-panes", "-t", window, "-F", PANE_ORDER_FORMAT])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(parse_pane_order_snapshot(&String::from_utf8_lossy(
        &output.stdout,
    )))
}

/// What one enforcement pass concluded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneOrderOutcome {
    /// The layout operation left the wanted order in place.
    Held,
    /// The layout operation left the wanted panes misordered; the swap plan
    /// restored the order and the re-read confirms it.
    Repaired,
    /// The order is still wrong after the repair (or could not be repaired).
    /// Loud: logged as an error on every pass.
    Violated,
    /// Some wanted panes are not in the window; order is not judged.
    Incomplete,
    /// tmux did not answer.
    Unobservable,
}

impl PaneOrderOutcome {
    pub fn label(self) -> &'static str {
        match self {
            Self::Held => "held",
            Self::Repaired => "repaired",
            Self::Violated => "violated",
            Self::Incomplete => "incomplete",
            Self::Unobservable => "unobservable",
        }
    }
}

/// One layout pass's pane-order evidence.
#[derive(Debug, Clone)]
pub struct PaneOrderPass {
    pub window: String,
    pub wanted: Vec<String>,
    /// Geometry before the layout operation, when the caller captured it.
    pub before: Option<PaneOrderSnapshot>,
    /// Geometry right after the layout operation.
    pub after_layout: Option<PaneOrderSnapshot>,
    /// Geometry after the repair (equal to `after_layout` when none ran).
    pub after: Option<PaneOrderSnapshot>,
    pub swaps: Vec<PaneSwap>,
    pub swap_error: Option<String>,
    pub outcome: PaneOrderOutcome,
}

impl PaneOrderPass {
    /// The ops.log line: everything needed to diagnose a reversal from the
    /// log alone (host tmux facts, wanted order, positions before and after).
    pub fn log_line(&self) -> String {
        let positions = |snapshot: &Option<PaneOrderSnapshot>| {
            snapshot
                .as_ref()
                .map(|snapshot| render_positions(&snapshot.panes))
                .unwrap_or_else(|| "unknown".to_string())
        };
        let environment = self
            .after
            .as_ref()
            .or(self.after_layout.as_ref())
            .or(self.before.as_ref())
            .map(|snapshot| snapshot.environment.to_string())
            .unwrap_or_else(|| {
                agent_doc_tmux::pane_order::TmuxLayoutEnvironment::default().to_string()
            });
        let swaps: Vec<String> = self
            .swaps
            .iter()
            .map(|swap| format!("{}<>{}", swap.source, swap.target))
            .collect();
        let mut line = format!(
            "tmux_pane_order outcome={} window={} {} wanted=[{}] before={} after_layout={} after={} swaps=[{}]",
            self.outcome.label(),
            self.window,
            environment,
            self.wanted.join(" "),
            positions(&self.before),
            positions(&self.after_layout),
            positions(&self.after),
            swaps.join(" "),
        );
        if let Some(error) = &self.swap_error {
            line.push_str(&format!(" swap_error={error:?}"));
        }
        line.push_str(" (#tmuxpanesreversed)");
        line
    }
}

/// Check the invariant after a layout operation and repair it when the
/// wanted panes are all present but misordered. One plan, one re-read: a
/// second violation is reported, never retried in a loop.
pub fn enforce_pane_order(
    tmux: &tmux_router::Tmux,
    window: &str,
    wanted: &[String],
    before: Option<PaneOrderSnapshot>,
) -> PaneOrderPass {
    let mut pass = PaneOrderPass {
        window: window.to_string(),
        wanted: wanted.to_vec(),
        before,
        after_layout: None,
        after: None,
        swaps: Vec::new(),
        swap_error: None,
        outcome: PaneOrderOutcome::Unobservable,
    };
    let Some(after_layout) = observe_pane_order(tmux, window) else {
        return pass;
    };
    let check = check_pane_order(wanted, &after_layout.panes);
    let operator_pane = active_pane(&after_layout.panes).map(str::to_string);
    pass.after_layout = Some(after_layout.clone());
    pass.after = Some(after_layout);
    match check.verdict {
        PaneOrderVerdict::Holds => {
            pass.outcome = PaneOrderOutcome::Held;
            return pass;
        }
        PaneOrderVerdict::Incomplete { .. } => {
            pass.outcome = PaneOrderOutcome::Incomplete;
            return pass;
        }
        PaneOrderVerdict::Violated => {}
    }
    let Some(plan) = plan_pane_order_repair(&check.observed, wanted) else {
        pass.outcome = PaneOrderOutcome::Violated;
        return pass;
    };
    for step in &plan {
        if let Err(error) = tmux.swap_pane(&step.source, &step.target) {
            pass.swap_error = Some(format!("{error:#}"));
            break;
        }
        pass.swaps.push(step.clone());
    }
    // `swap-pane -d` keeps the active SLOT, so swapping the active pane's
    // slot hands focus to another pane. The operator stays on their pane.
    let mut after = observe_pane_order(tmux, window);
    if let (Some(operator_pane), Some(snapshot)) = (operator_pane.as_deref(), after.as_ref())
        && active_pane(&snapshot.panes) != Some(operator_pane)
    {
        let _ = tmux.select_pane_preserving_window(operator_pane);
        after = observe_pane_order(tmux, window);
    }
    crate::observation_cache::observe_external_mutation();
    pass.after = after;
    pass.outcome = match pass
        .after
        .as_ref()
        .map(|after| check_pane_order(wanted, &after.panes).verdict)
    {
        Some(PaneOrderVerdict::Holds) => PaneOrderOutcome::Repaired,
        Some(_) => PaneOrderOutcome::Violated,
        None => PaneOrderOutcome::Unobservable,
    };
    pass
}

/// `#protectedextrasliver`: when a layout pass had to leave a protected pane
/// in a single-row window and that left some column a sliver, spread the row
/// evenly with `select-layout even-horizontal`. Returns the before/after
/// positions when it rebalanced, `None` when nothing was starved.
pub fn rebalance_starved_single_row(
    tmux: &tmux_router::Tmux,
    window: &str,
) -> Option<(String, String)> {
    let before = observe_pane_order(tmux, window)?;
    if !single_row_has_starved_pane(&before) {
        return None;
    }
    tmux.raw_cmd(&["select-layout", "-t", window, "even-horizontal"])
        .ok()?;
    crate::observation_cache::observe_external_mutation();
    let after = observe_pane_order(tmux, window)?;
    Some((render_widths(&before), render_widths(&after)))
}

fn render_widths(snapshot: &PaneOrderSnapshot) -> String {
    let panes: Vec<String> = snapshot
        .panes
        .iter()
        .map(|pane| {
            format!(
                "{}@{}w{}",
                pane.pane_id,
                pane.left,
                pane.width
                    .map_or_else(|| "?".to_string(), |width| width.to_string())
            )
        })
        .collect();
    format!("[{}]", panes.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_tmux::pane_order::{apply_pane_swaps, physical_order};

    fn permutations(items: &[String]) -> Vec<Vec<String>> {
        if items.len() <= 1 {
            return vec![items.to_vec()];
        }
        let mut out = Vec::new();
        for at in 0..items.len() {
            let mut rest = items.to_vec();
            let head = rest.remove(at);
            for mut tail in permutations(&rest) {
                tail.insert(0, head.clone());
                out.push(tail);
            }
        }
        out
    }

    /// Put the window's panes into `start` order with real `swap-pane`s.
    fn arrange(tmux: &tmux_router::Tmux, window: &str, start: &[String]) {
        let now = physical_order(&observe_pane_order(tmux, window).unwrap().panes);
        let plan = plan_pane_order_repair(&now, start).unwrap();
        for step in &plan {
            tmux.swap_pane(&step.source, &step.target).unwrap();
        }
        let arranged = physical_order(&observe_pane_order(tmux, window).unwrap().panes);
        assert_eq!(arranged, start, "test setup must reach the start order");
        assert_eq!(
            apply_pane_swaps(&now, &plan),
            arranged,
            "swap model matches tmux"
        );
    }

    /// Real tmux: a window of `width` side-by-side panes on an isolated
    /// server (killed on drop), with `base-index`/`pane-base-index` set.
    fn window_with_panes(
        label: &str,
        width: usize,
        base_index: &str,
    ) -> (crate::IsolatedTestTmux, String, Vec<String>) {
        let tmux = crate::isolated_test_tmux(label);
        let cwd = std::env::temp_dir();
        tmux.raw_cmd(&["set-option", "-g", "base-index", base_index])
            .ok();
        let first = tmux.new_session("order", &cwd).unwrap();
        tmux.raw_cmd(&["set-option", "-g", "base-index", base_index])
            .unwrap();
        tmux.raw_cmd(&["set-window-option", "-g", "pane-base-index", base_index])
            .unwrap();
        tmux.raw_cmd(&["resize-window", "-t", &first, "-x", "250", "-y", "40"])
            .ok();
        let window = tmux.pane_window(&first).unwrap();
        let mut panes = vec![first];
        while panes.len() < width {
            // Alternate before/after splits so creation order, pane index and
            // position all disagree.
            let flags = if panes.len() % 2 == 0 { "-dhb" } else { "-dh" };
            let target = panes.last().unwrap().clone();
            panes.push(tmux.split_window(&target, &cwd, flags).unwrap());
        }
        tmux.raw_cmd(&["select-layout", "-t", &window, "even-horizontal"])
            .unwrap();
        (tmux, window, panes)
    }

    fn assert_enforces(
        tmux: &tmux_router::Tmux,
        window: &str,
        start: &[String],
        wanted: &[String],
        base_index: &str,
    ) {
        arrange(tmux, window, start);
        let active_before = tmux
            .raw_cmd(&["display-message", "-p", "-t", window, "#{pane_id}"])
            .unwrap();
        let pass = enforce_pane_order(tmux, window, wanted, None);
        let expected = if start == wanted {
            PaneOrderOutcome::Held
        } else {
            PaneOrderOutcome::Repaired
        };
        assert_eq!(pass.outcome, expected, "{}", pass.log_line());
        let after = physical_order(&observe_pane_order(tmux, window).unwrap().panes);
        assert_eq!(after, wanted, "{}", pass.log_line());
        let active_after = tmux
            .raw_cmd(&["display-message", "-p", "-t", window, "#{pane_id}"])
            .unwrap();
        assert_eq!(active_before, active_after, "repair never moves focus");
        let line = pass.log_line();
        assert!(line.contains(&format!("base_index={base_index}")), "{line}");
        assert!(
            line.contains(&format!("pane_base_index={base_index}")),
            "{line}"
        );
        assert!(!line.contains("tmux_version=unknown"), "{line}");
    }

    /// `#protectedextrasliver`, real tmux: the live 2026-10-28 window. A
    /// protected foreign pane plus a 50%-equalized first column left the last
    /// column ONE cell wide; the rebalance spreads the row evenly and keeps
    /// pane order, and an even row is left alone.
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn a_protected_extra_sliver_is_rebalanced_to_an_even_row() {
        let (tmux, window, panes) = window_with_panes("protected-extra-sliver", 3, "0");
        tmux.raw_cmd(&["resize-window", "-t", &window, "-x", "240", "-y", "55"])
            .unwrap();
        let order = physical_order(&observe_pane_order(&tmux, &window).unwrap().panes);
        tmux.raw_cmd(&["resize-pane", "-t", &order[0], "-x", "117"])
            .unwrap();
        tmux.raw_cmd(&["resize-pane", "-t", &order[1], "-x", "120"])
            .unwrap();
        let sliver = observe_pane_order(&tmux, &window).unwrap();
        let widths: Vec<Option<u32>> = sliver.panes.iter().map(|pane| pane.width).collect();
        assert!(
            widths.contains(&Some(1)),
            "fixture reproduces the one-column sliver: {widths:?}"
        );

        let (before, after) =
            rebalance_starved_single_row(&tmux, &window).expect("a starved row is rebalanced");
        assert!(before.contains("w1"), "{before}");
        let rebalanced = observe_pane_order(&tmux, &window).unwrap();
        assert!(
            rebalanced
                .panes
                .iter()
                .all(|pane| pane.width.is_some_and(|width| width >= 78)),
            "{after}"
        );
        assert_eq!(physical_order(&rebalanced.panes), order, "order is kept");
        assert_eq!(panes.len(), 3);
        assert_eq!(
            rebalance_starved_single_row(&tmux, &window),
            None,
            "an even row is left alone"
        );
    }

    /// Every start arrangement x every wanted order for 2..=4 columns, under
    /// `base-index`/`pane-base-index` 0 and 1.
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn real_tmux_enforcement_reaches_every_wanted_order_up_to_four_columns() {
        for base_index in ["0", "1"] {
            for width in 2..=4 {
                let (tmux, window, panes) = window_with_panes(
                    &format!("pane-order-{width}-{base_index}"),
                    width,
                    base_index,
                );
                let orders = permutations(&panes);
                for start in &orders {
                    for wanted in &orders {
                        assert_enforces(&tmux, &window, start, wanted, base_index);
                    }
                }
            }
        }
    }

    /// Five columns: every wanted order from the creation order and from its
    /// reverse (the reported symptom).
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn real_tmux_enforcement_reaches_every_wanted_order_at_five_columns() {
        let (tmux, window, panes) = window_with_panes("pane-order-5", 5, "1");
        let mut reversed = panes.clone();
        reversed.reverse();
        let created = physical_order(&observe_pane_order(&tmux, &window).unwrap().panes);
        for start in [created, reversed] {
            for wanted in permutations(&panes) {
                assert_enforces(&tmux, &window, &start, &wanted, "1");
            }
        }
    }

    /// A wanted pane that is not in the window is reported, never "repaired".
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn real_tmux_enforcement_leaves_an_incomplete_window_alone() {
        let (tmux, window, panes) = window_with_panes("pane-order-incomplete", 2, "0");
        let mut wanted = vec![panes[1].clone(), "%999".to_string(), panes[0].clone()];
        let before = physical_order(&observe_pane_order(&tmux, &window).unwrap().panes);
        let pass = enforce_pane_order(&tmux, &window, &wanted, None);
        assert_eq!(pass.outcome, PaneOrderOutcome::Incomplete);
        assert!(pass.swaps.is_empty());
        let after = physical_order(&observe_pane_order(&tmux, &window).unwrap().panes);
        assert_eq!(before, after);
        wanted.retain(|pane| pane != "%999");
        assert_eq!(
            enforce_pane_order(&tmux, "@99999", &wanted, None).outcome,
            PaneOrderOutcome::Unobservable
        );
    }
}
