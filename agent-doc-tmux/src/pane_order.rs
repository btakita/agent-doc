//! Physical pane-order invariant (`#tmuxpanesreversed`).
//!
//! **Invariant.** After every layout operation, the panes of the wanted
//! layout, read left to right by tmux's own geometry (`pane_left`, then
//! `pane_top` inside a stacked column), are exactly the wanted column-major
//! order.
//!
//! This module is the pure half of enforcing it:
//!
//! - [`parse_pane_order_snapshot`] reads one `list-panes` answer in
//!   [`PANE_ORDER_FORMAT`], including the tmux facts that differ between
//!   hosts (server version, `base-index`, `pane-base-index`, window size).
//! - [`physical_order`] orders panes by position only. `pane_index` is parsed
//!   for diagnostics and never used for ordering: its numbering depends on
//!   `pane-base-index`, and `list-panes` line order is not a geometry claim.
//! - [`check_pane_order`] compares the physical order of the wanted panes
//!   with the wanted order. Panes outside the wanted set (a protected busy
//!   pane, say) are ignored; they neither satisfy nor break the invariant.
//! - [`plan_pane_order_repair`] turns a violated-but-complete order into the
//!   minimal sequence of `swap-pane` steps that restores it. `swap-pane`
//!   exchanges two panes' slots and leaves every other slot alone, so the
//!   plan is a pure permutation of the wanted panes' slots.
//!
//! The effectful half (read, swap, re-read, log) lives in
//! `agent_doc_tmux_io::pane_order`.

use std::collections::HashSet;
use std::fmt;
use std::path::PathBuf;

/// `list-panes -F` format for one pane-order observation. Option values are
/// read through formats so one spawn answers the whole observation; `|` never
/// occurs in a pane id or a number.
pub const PANE_ORDER_FORMAT: &str = "#{pane_id}|#{pane_left}|#{pane_top}|#{pane_index}|#{pane_width}|#{window_width}|#{window_height}|#{version}|#{base-index}|#{pane-base-index}|#{pane_active}";

/// One pane's position as tmux reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedPane {
    pub pane_id: String,
    pub left: u32,
    pub top: u32,
    /// Diagnostic only; never an ordering input.
    pub index: Option<u32>,
    pub width: Option<u32>,
    /// The window's active pane (operator focus inside the window).
    pub active: bool,
}

/// Host facts that can differ between a workstation and a remote (Coder)
/// tmux server.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TmuxLayoutEnvironment {
    pub version: String,
    pub base_index: String,
    pub pane_base_index: String,
    pub window_size: String,
}

impl fmt::Display for TmuxLayoutEnvironment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let or_unknown = |value: &str| {
            if value.is_empty() {
                "unknown".to_string()
            } else {
                value.to_string()
            }
        };
        write!(
            f,
            "tmux_version={} base_index={} pane_base_index={} window_size={}",
            or_unknown(&self.version),
            or_unknown(&self.base_index),
            or_unknown(&self.pane_base_index),
            or_unknown(&self.window_size),
        )
    }
}

/// One parsed `list-panes` answer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PaneOrderSnapshot {
    pub panes: Vec<ObservedPane>,
    pub environment: TmuxLayoutEnvironment,
}

/// Parse `list-panes -F PANE_ORDER_FORMAT` output. A line whose id or
/// position does not parse is dropped rather than defaulted: a defaulted
/// `pane_left` of 0 would silently move that pane to the far left.
pub fn parse_pane_order_snapshot(output: &str) -> PaneOrderSnapshot {
    let mut snapshot = PaneOrderSnapshot::default();
    for line in output.lines() {
        let fields: Vec<&str> = line.trim().split('|').collect();
        let [id, left, top, rest @ ..] = fields.as_slice() else {
            continue;
        };
        let (Ok(left), Ok(top)) = (left.trim().parse::<u32>(), top.trim().parse::<u32>()) else {
            continue;
        };
        if !id.starts_with('%') {
            continue;
        }
        let field = |at: usize| rest.get(at).map(|value| value.trim()).unwrap_or("");
        snapshot.panes.push(ObservedPane {
            pane_id: id.to_string(),
            left,
            top,
            index: field(0).parse().ok(),
            width: field(1).parse().ok(),
            active: field(7) == "1",
        });
        if snapshot.environment.version.is_empty() {
            let (window_width, window_height) = (field(2), field(3));
            snapshot.environment = TmuxLayoutEnvironment {
                version: field(4).to_string(),
                base_index: field(5).to_string(),
                pane_base_index: field(6).to_string(),
                window_size: if window_width.is_empty() {
                    String::new()
                } else {
                    format!("{window_width}x{window_height}")
                },
            };
        }
    }
    snapshot
}

/// The window's active pane, if the snapshot names one.
pub fn active_pane(panes: &[ObservedPane]) -> Option<&str> {
    panes
        .iter()
        .find(|pane| pane.active)
        .map(|pane| pane.pane_id.as_str())
}

/// Pane ids left to right (then top to bottom), by geometry alone. The pane
/// id breaks an exact tie so the answer never depends on input line order.
pub fn physical_order(panes: &[ObservedPane]) -> Vec<String> {
    let mut sorted: Vec<&ObservedPane> = panes.iter().collect();
    sorted.sort_by(|a, b| {
        (a.left, a.top)
            .cmp(&(b.left, b.top))
            .then_with(|| a.pane_id.cmp(&b.pane_id))
    });
    sorted
        .into_iter()
        .map(|pane| pane.pane_id.clone())
        .collect()
}

/// Render positions for a log line: `%3@0,0#1` (id @ left,top # index), in
/// physical order.
pub fn render_positions(panes: &[ObservedPane]) -> String {
    let order = physical_order(panes);
    let rendered: Vec<String> = order
        .iter()
        .filter_map(|id| panes.iter().find(|pane| &pane.pane_id == id))
        .map(|pane| {
            let index = pane
                .index
                .map(|index| index.to_string())
                .unwrap_or_else(|| "?".to_string());
            format!("{}@{},{}#{}", pane.pane_id, pane.left, pane.top, index)
        })
        .collect();
    format!("[{}]", rendered.join(" "))
}

/// The wanted pane order of a layout, exactly as tmux-router builds it: each
/// `--col` argument is a comma-separated file list, columns left to right,
/// files top to bottom; a file maps to the pane it was realised in, and a
/// pane realising several files keeps only its first slot.
pub fn wanted_pane_order(col_args: &[String], file_panes: &[(PathBuf, String)]) -> Vec<String> {
    let mut seen: HashSet<&str> = HashSet::new();
    let mut wanted = Vec::new();
    for column in col_args {
        for file in column.split(',').map(str::trim).filter(|f| !f.is_empty()) {
            let file = PathBuf::from(file);
            let Some((_, pane)) = file_panes.iter().find(|(path, _)| *path == file) else {
                continue;
            };
            if seen.insert(pane.as_str()) {
                wanted.push(pane.clone());
            }
        }
    }
    wanted
}

/// Result of comparing tmux's physical order with the wanted order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaneOrderVerdict {
    /// The wanted panes appear left to right in the wanted order.
    Holds,
    /// Every wanted pane is in the window, in the wrong order.
    Violated,
    /// Some wanted panes are not in the window, so order cannot be judged
    /// (membership is a different reconciliation's job).
    Incomplete { missing: Vec<String> },
}

impl PaneOrderVerdict {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Holds => "holds",
            Self::Violated => "violated",
            Self::Incomplete { .. } => "incomplete",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneOrderCheck {
    pub verdict: PaneOrderVerdict,
    /// The wanted panes present in the window, in physical order.
    pub observed: Vec<String>,
}

/// Check the invariant for `wanted` against the window's pane positions.
pub fn check_pane_order(wanted: &[String], panes: &[ObservedPane]) -> PaneOrderCheck {
    let wanted_set: HashSet<&str> = wanted.iter().map(String::as_str).collect();
    let observed: Vec<String> = physical_order(panes)
        .into_iter()
        .filter(|pane| wanted_set.contains(pane.as_str()))
        .collect();
    let missing: Vec<String> = wanted
        .iter()
        .filter(|pane| !observed.contains(pane))
        .cloned()
        .collect();
    let verdict = if !missing.is_empty() {
        PaneOrderVerdict::Incomplete { missing }
    } else if observed == wanted {
        PaneOrderVerdict::Holds
    } else {
        PaneOrderVerdict::Violated
    };
    PaneOrderCheck { verdict, observed }
}

/// One `swap-pane -d -s <source> -t <target>` step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneSwap {
    pub source: String,
    pub target: String,
}

/// The minimal `swap-pane` plan that turns `observed` (the wanted panes in
/// physical order) into `wanted`. `None` when the two are not permutations of
/// one another (missing, extra, or duplicated panes): swaps can only reorder.
///
/// The plan has `n - c` steps, where `c` is the number of cycles of the
/// permutation, so at most `n - 1`, and is empty exactly when the order
/// already holds.
pub fn plan_pane_order_repair(observed: &[String], wanted: &[String]) -> Option<Vec<PaneSwap>> {
    let distinct = |panes: &[String]| panes.iter().collect::<HashSet<_>>().len() == panes.len();
    if observed.len() != wanted.len()
        || !distinct(observed)
        || !distinct(wanted)
        || wanted.iter().any(|pane| !observed.contains(pane))
    {
        return None;
    }
    let mut slots = observed.to_vec();
    let mut plan = Vec::new();
    for slot in 0..wanted.len() {
        if slots[slot] == wanted[slot] {
            continue;
        }
        let from = slots.iter().position(|pane| *pane == wanted[slot])?;
        plan.push(PaneSwap {
            source: wanted[slot].clone(),
            target: slots[slot].clone(),
        });
        slots.swap(slot, from);
    }
    Some(plan)
}

/// Apply `swap-pane` steps to a slot list (the model of tmux's semantics:
/// the two panes exchange slots, every other slot is unchanged).
pub fn apply_pane_swaps(slots: &[String], plan: &[PaneSwap]) -> Vec<String> {
    let mut slots = slots.to_vec();
    for step in plan {
        let source = slots.iter().position(|pane| *pane == step.source);
        let target = slots.iter().position(|pane| *pane == step.target);
        if let (Some(source), Some(target)) = (source, target) {
            slots.swap(source, target);
        }
    }
    slots
}

/// `#protectedextrasliver`: true when a single-row window leaves some pane a
/// sliver of its even share.
///
/// tmux-router's equalize step sizes a two-column layout by resizing the
/// FIRST wanted pane to 50% of the window. When a third, protected pane (a
/// foreign root's column the router must not stash) still sits in the row,
/// that resize takes every column from the LAST pane: observed live
/// 2026-10-28 as `[%6 117 | %16 120 | %3 1]` in a 240-column window. A pane
/// narrower than half its even share (and never wider than 1 column short of
/// 2) is starved. Stacked layouts (panes at different `top`) are never judged:
/// an even-horizontal rebalance would flatten their rows.
pub fn single_row_has_starved_pane(snapshot: &PaneOrderSnapshot) -> bool {
    let panes = &snapshot.panes;
    if panes.len() < 2 {
        return false;
    }
    let top = panes[0].top;
    if panes.iter().any(|pane| pane.top != top) {
        return false;
    }
    let Some(window_width) = snapshot
        .environment
        .window_size
        .split_once('x')
        .and_then(|(width, _)| width.parse::<u32>().ok())
    else {
        return false;
    };
    let even_share = window_width / panes.len() as u32;
    let floor = (even_share / 2).max(2);
    panes
        .iter()
        .any(|pane| pane.width.is_some_and(|width| width < floor))
}

#[cfg(test)]
mod tests {

    fn row_snapshot(widths: &[u32], window_width: u32) -> PaneOrderSnapshot {
        let mut left = 0;
        let lines: Vec<String> = widths
            .iter()
            .enumerate()
            .map(|(index, width)| {
                let line = format!("%{index}|{left}|1|{index}|{width}|{window_width}|55|3.8|0|0|0");
                left += width + 1;
                line
            })
            .collect();
        parse_pane_order_snapshot(&lines.join("\n"))
    }

    #[test]
    fn protected_extra_sliver_is_starved_but_even_rows_are_not() {
        // The live 2026-10-28 window: agent-loop's protected pane, then the
        // nested controller's two 50%-equalized columns.
        assert!(single_row_has_starved_pane(&row_snapshot(
            &[117, 120, 1],
            240
        )));
        assert!(!single_row_has_starved_pane(&row_snapshot(
            &[120, 119],
            240
        )));
        assert!(!single_row_has_starved_pane(&row_snapshot(
            &[79, 79, 80],
            240
        )));
        assert!(!single_row_has_starved_pane(&row_snapshot(&[240], 240)));
        // A stacked column is never judged by a single-row rule.
        let mut stacked = row_snapshot(&[120, 1], 240);
        stacked.panes[1].top = 20;
        assert!(!single_row_has_starved_pane(&stacked));
    }
    use super::*;

    const PANES: [&str; 5] = ["%1", "%2", "%3", "%4", "%5"];

    fn ids(panes: &[&str]) -> Vec<String> {
        panes.iter().map(|pane| (*pane).to_string()).collect()
    }

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

    fn cycles(observed: &[String], wanted: &[String]) -> usize {
        let mut visited = vec![false; observed.len()];
        let mut count = 0;
        for start in 0..observed.len() {
            if visited[start] {
                continue;
            }
            count += 1;
            let mut at = start;
            while !visited[at] {
                visited[at] = true;
                at = wanted
                    .iter()
                    .position(|pane| *pane == observed[at])
                    .unwrap();
            }
        }
        count
    }

    /// A tmux window model: `slots` is the physical left-to-right pane list.
    /// tmux renders `list-panes` in pane-index order; the model renders it
    /// adversarially (reversed, numbered from `pane_base_index`, with the
    /// index deliberately disagreeing with position), so any consumer that
    /// orders by line order or by `pane_index` instead of `pane_left` fails.
    fn render_window(slots: &[String], pane_base_index: u32, column_width: u32) -> String {
        let count = slots.len() as u32;
        let mut lines: Vec<String> = slots
            .iter()
            .enumerate()
            .map(|(slot, pane)| {
                let slot = slot as u32;
                // Index runs right to left: the leftmost pane has the highest.
                let index = pane_base_index + (count - 1 - slot);
                format!(
                    "{pane}|{}|0|{index}|{column_width}|{}|50|3.8|{pane_base_index}|{pane_base_index}|{}",
                    slot * (column_width + 1),
                    count * (column_width + 1),
                    u8::from(slot == 0),
                )
            })
            .collect();
        lines.reverse();
        lines.join("\n")
    }

    #[test]
    fn parses_positions_and_host_environment() {
        let snapshot = parse_pane_order_snapshot(
            "%7|101|0|2|100|201|50|3.4|1|1\n%3|0|0|1|100|201|50|3.4|1|1\ngarbage\n%9|x|0|3|1|1|1|3.4|1|1\n",
        );
        assert_eq!(physical_order(&snapshot.panes), ids(&["%3", "%7"]));
        assert_eq!(
            snapshot.environment.to_string(),
            "tmux_version=3.4 base_index=1 pane_base_index=1 window_size=201x50"
        );
        assert_eq!(render_positions(&snapshot.panes), "[%3@0,0#1 %7@101,0#2]");
    }

    #[test]
    fn an_unknown_environment_says_so() {
        assert_eq!(
            TmuxLayoutEnvironment::default().to_string(),
            "tmux_version=unknown base_index=unknown pane_base_index=unknown window_size=unknown"
        );
    }

    #[test]
    fn stacked_column_orders_left_then_top() {
        let snapshot = parse_pane_order_snapshot("%4|81|0|3\n%2|0|25|2\n%1|0|0|1\n%5|81|25|4\n");
        assert_eq!(
            physical_order(&snapshot.panes),
            ids(&["%1", "%2", "%4", "%5"])
        );
    }

    #[test]
    fn wanted_order_follows_columns_then_rows_and_dedupes_donor_panes() {
        let cols = vec![
            "a.md,b.md".to_string(),
            "c.md".to_string(),
            "d.md".to_string(),
        ];
        let file_panes = vec![
            (PathBuf::from("d.md"), "%4".to_string()),
            (PathBuf::from("c.md"), "%1".to_string()),
            (PathBuf::from("b.md"), "%2".to_string()),
            (PathBuf::from("a.md"), "%1".to_string()),
        ];
        assert_eq!(
            wanted_pane_order(&cols, &file_panes),
            ids(&["%1", "%2", "%4"])
        );
    }

    #[test]
    fn extra_panes_neither_satisfy_nor_break_the_invariant() {
        let panes =
            parse_pane_order_snapshot("%9|0|0|0\n%1|50|0|1\n%8|100|0|2\n%2|150|0|3\n").panes;
        assert_eq!(
            check_pane_order(&ids(&["%1", "%2"]), &panes).verdict,
            PaneOrderVerdict::Holds
        );
        assert_eq!(
            check_pane_order(&ids(&["%2", "%1"]), &panes).verdict,
            PaneOrderVerdict::Violated
        );
        assert_eq!(
            check_pane_order(&ids(&["%1", "%3"]), &panes).verdict,
            PaneOrderVerdict::Incomplete {
                missing: ids(&["%3"])
            }
        );
    }

    #[test]
    fn repair_refuses_anything_but_a_permutation() {
        assert_eq!(
            plan_pane_order_repair(&ids(&["%1"]), &ids(&["%1", "%2"])),
            None
        );
        assert_eq!(
            plan_pane_order_repair(&ids(&["%1", "%3"]), &ids(&["%1", "%2"])),
            None
        );
        assert_eq!(
            plan_pane_order_repair(&ids(&["%1", "%1"]), &ids(&["%1", "%1"])),
            None
        );
    }

    /// The invariant, exhaustively: every wanted order of up to five columns,
    /// from every starting arrangement, under `pane-base-index` 0 and 1 and two
    /// window widths. The check reads the rendered `list-panes` answer (index
    /// numbering and line order adversarial), the plan is applied with
    /// `swap-pane` semantics, and the re-read must hold. The plan is minimal
    /// and empty exactly when the order already held.
    #[test]
    fn every_start_reaches_every_wanted_order_by_position() {
        let mut checked = 0usize;
        for width in 1..=PANES.len() {
            let panes = ids(&PANES[..width]);
            let orders = permutations(&panes);
            for start in &orders {
                for wanted in &orders {
                    for pane_base_index in [0, 1] {
                        for column_width in [40, 99] {
                            let before = parse_pane_order_snapshot(&render_window(
                                start,
                                pane_base_index,
                                column_width,
                            ));
                            let check = check_pane_order(wanted, &before.panes);
                            assert_eq!(&check.observed, start, "position order, not index order");
                            assert_eq!(
                                check.verdict == PaneOrderVerdict::Holds,
                                start == wanted,
                                "start={start:?} wanted={wanted:?}"
                            );
                            let plan = plan_pane_order_repair(&check.observed, wanted)
                                .expect("a complete window is always repairable");
                            assert_eq!(plan.len(), width - cycles(start, wanted));
                            assert_eq!(plan.is_empty(), start == wanted);
                            let repaired = apply_pane_swaps(start, &plan);
                            let after = parse_pane_order_snapshot(&render_window(
                                &repaired,
                                pane_base_index,
                                column_width,
                            ));
                            assert_eq!(
                                check_pane_order(wanted, &after.panes).verdict,
                                PaneOrderVerdict::Holds,
                                "start={start:?} wanted={wanted:?} plan={plan:?}"
                            );
                            checked += 1;
                        }
                    }
                }
            }
        }
        // 1 + 4 + 36 + 576 + 14400 start/wanted pairs, times 4 environments.
        assert_eq!(checked, (1 + 4 + 36 + 576 + 14_400) * 4);
    }

    /// Extra (unwanted) panes interleaved anywhere keep their slots through a
    /// repair, and the wanted panes still end in the wanted order.
    #[test]
    fn repair_with_interleaved_extra_panes_keeps_the_extras_in_place() {
        let wanted_panes = ids(&["%1", "%2", "%3", "%4"]);
        for start in permutations(&wanted_panes) {
            for wanted in permutations(&wanted_panes) {
                for extra_at in 0..=start.len() {
                    let mut slots = start.clone();
                    slots.insert(extra_at, "%9".to_string());
                    let panes = parse_pane_order_snapshot(&render_window(&slots, 0, 40)).panes;
                    let check = check_pane_order(&wanted, &panes);
                    let plan = plan_pane_order_repair(&check.observed, &wanted).unwrap();
                    let repaired = apply_pane_swaps(&slots, &plan);
                    assert_eq!(repaired[extra_at], "%9");
                    let after = parse_pane_order_snapshot(&render_window(&repaired, 0, 40)).panes;
                    assert_eq!(
                        check_pane_order(&wanted, &after).verdict,
                        PaneOrderVerdict::Holds
                    );
                }
            }
        }
    }
}
