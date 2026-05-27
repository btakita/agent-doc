# Plan: OpenCode Dispatch-Start Proof via Pane State Transition

## Problem

OpenCode dispatch-only routing always fails with `FailClosedAcceptedOnly` because:

1. `dispatch_only_dispatch_start_proof_required` (`routed_reopen.rs:319`) returns `true` for OpenCode — requiring dispatch-start proof
2. `build_routed_dispatch_start_tracker` (`route.rs:491`) returns `None` for non-Codex harnesses — OpenCode can never produce proof
3. Result: `CommandAcceptedOnly` + `proof_required=true` always yields `FailClosedAcceptedOnly` (`routed_reopen.rs:276`)

The SPEC (`SPEC.md:71`) confirms OpenCode has no prompt-submit hook equivalent.

## Approach: Pane State Transition Detection

Observe the tmux pane transitioning from idle (dispatch-ready) to active (processing). This is the most reliable observable for dispatch-only sessions.

### Why pane state transition?

| Observable | Reliability | Latency | Available for dispatch-only? |
|---|---|---|---|
| Hook state file (Codex) | High | ~200ms | No (hook mechanism) |
| Pane state transition | Medium-High | 500ms-15s | Yes |
| OpenCode session log | Unknown | Unknown | Unknown (not documented) |
| Supervisor screen model | High | ~200ms | No (managed sessions only) |

For dispatch-only sessions, tmux `capture-pane` is the only observation mechanism. The transition from idle chrome to active output is the best available signal.

### Observable signals

**Idle state** (dispatch-ready):
- Prompt glyph `>` or `›` present (harness.rs:209)
- Idle chrome: "Ask anything...", "context X% used" (harness.rs:422-441)
- No busy cues

**Active state** (processing started):
- Prompt glyph disappears
- Non-idle content appears (agent output, spinner, permission prompt)
- `is_dispatch_ready_prompt_line` returns false for all visible lines
- `is_idle_chrome_only_output` returns false

## Implementation Steps

### Step 1: Add OpenCode busy cue detection in `harness.rs`

Currently `dispatch_blocker_reason` (harness.rs:296) returns `None` for non-Codex. Add OpenCode-specific detection:

```rust
// In dispatch_blocker_reason, after the permission prompt check:
if self.binary == "opencode" {
    // Detect when OpenCode is actively processing (not idle)
    let stripped = output.lines()
        .map(crate::prompt::strip_ansi)
        .map(|l| l.trim().to_string())
        .collect::<Vec<_>>();
    
    // If no idle chrome lines and no prompt glyph → busy
    let has_prompt = stripped.iter().any(|l| matches!(l.as_str(), ">" | "›"));
    let has_idle_splash = stripped.iter().any(|l| l.contains("Ask anything"));
    
    if !has_prompt && !has_idle_splash && !stripped.iter().all(|l| l.is_empty()) {
        // Check if there's actual content (not just empty/whitespace)
        let has_content = stripped.iter().any(|l| !l.is_empty() && !is_opencode_idle_chrome_line(l));
        if has_content {
            return Some("opencode active turn".to_string());
        }
    }
    return None;
}
```

**Note:** This needs careful design to avoid false positives during TUI redraws. The existing `is_idle_chrome_only_output` helper may be reusable here.

### Step 2: Extend `RoutedDispatchStartTracker` in `route.rs`

Add a variant for pane-state-based tracking:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
enum DispatchStartTrackerKind {
    CodexHook {
        trigger: String,
        previous_session_id: Option<String>,
        previous_turn_id: Option<String>,
        previous_updated_at: Option<u64>,
    },
    OpenCodePane {
        pane: String,
        pre_dispatch_content_hash: String,  // hash of idle pane content
        trigger: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RoutedDispatchStartTracker {
    kind: DispatchStartTrackerKind,
}
```

Alternatively, add fields directly to the existing struct with an enum tag. The key is that OpenCode tracking needs `pane` and `pre_dispatch_content_hash`.

### Step 3: Modify `build_routed_dispatch_start_tracker` in `route.rs`

```rust
fn build_routed_dispatch_start_tracker(
    file: &Path, file_path: &str, harness: &HarnessConfig,
    tmux: Option<&Tmux>,  // new param for capture-pane access
) -> Result<Option<RoutedDispatchStartTracker>> {
    match harness.binary.as_str() {
        "codex" if codex_dispatch_start_tracking_enabled(file) => {
            // existing Codex hook tracking (unchanged)
        }
        "opencode" => {
            // Capture current pane content as baseline
            let Some(tmux) = tmux else { return Ok(None) };
            let pane = /* resolve target pane for file */;
            let content = sessions::capture_pane(tmux, &pane)?;
            let hash = sha256(&content);
            Ok(Some(RoutedDispatchStartTracker::openencode_pane(
                pane, hash, harness.trigger_command(file_path),
            )))
        }
        _ => Ok(None),
    }
}
```

**Challenge:** The current function signature does not have `tmux` or `pane` access. These need to be threaded through from the callers (`dispatch_routed_reopen_with_mode` and `dispatch_via_supervisor_ipc_with_mode`).

### Step 4: Implement OpenCode proof polling

New function alongside `wait_for_routed_dispatch_start`:

```rust
fn wait_for_opencode_pane_state_change(
    tmux: &Tmux,
    tracker: &OpenCodePaneTracker,
    harness: &HarnessConfig,
    timeout: Duration,
) -> Result<Option<RoutedDispatchStartProof>> {
    let start = std::time::Instant::now();
    let poll = Duration::from_millis(200);

    while start.elapsed() < timeout {
        let content = sessions::capture_pane(tmux, &tracker.pane)?;
        let current_hash = sha256(&content);
        
        // Content changed from pre-dispatch baseline
        if current_hash != tracker.pre_dispatch_content_hash {
            // Verify it's not just idle chrome anymore
            if !harness.is_idle_chrome_only_output(&content) {
                return Ok(Some(RoutedDispatchStartProof::PaneStateChanged));
            }
        }
        std::thread::sleep(poll);
    }

    Ok(None)  // timeout → CommandAcceptedOnly
}
```

### Step 5: Integrate into dispatch flow

In `dispatch_routed_reopen_with_mode` (route.rs:5779):

```rust
let tracker = build_routed_dispatch_start_tracker(file, file_path, harness, Some(tmux))?;
let submit_result = send_command_checked(tmux, pane, file_path, harness)?;

let Some(tracker) = tracker else {
    return Ok(RoutedDispatchStartProof::CommandAcceptedOnly);
};

let proof = match tracker.kind {
    DispatchStartTrackerKind::CodexHook { .. } => {
        // existing hook polling
        wait_for_routed_dispatch_start(file, &tracker, timeout)?
            .unwrap_or(RoutedDispatchStartProof::CommandAcceptedOnly)
    }
    DispatchStartTrackerKind::OpenCodePane { .. } => {
        // new pane state polling
        wait_for_opencode_pane_state_change(tmux, &tracker, harness, timeout)?
            .unwrap_or(RoutedDispatchStartProof::CommandAcceptedOnly)
    }
};
```

### Step 6: Add `PaneStateChanged` proof variant

In `routed_reopen.rs`, extend `RoutedDispatchStartProof`:

```rust
pub(crate) enum RoutedDispatchStartProof {
    CommandAcceptedOnly,
    HookPromptMatched,
    HookStateAdvanced,
    PaneStateChanged,  // NEW: pane content changed from idle to active
}
```

Update `classify_dispatch_start_proof` to accept `PaneStateChanged`:

```rust
pub(crate) fn decide_dispatch_start_proof(proof, required) -> Decision {
    match proof {
        PaneStateChanged if required => DispatchStartProofDecision::Accepted,
        HookPromptMatched | HookStateAdvanced => Accepted,
        CommandAcceptedOnly if required => FailClosedAcceptedOnly,
        _ => Accepted,
    }
}
```

### Step 7: Consider timeout budget

OpenCode's TUI has a 15s redraw budget (routed_reopen.rs:66-67). The dispatch-start proof timeout is 10s (routed_reopen.rs:22-28). Consider extending to 15s for OpenCode:

```rust
fn opencode_dispatch_start_timeout(test_mode: bool) -> Duration {
    if test_mode { Duration::from_secs(2) }
    else { Duration::from_secs(15) }
}
```

### Step 8: Update SPEC.md

Document the new proof mechanism:

> OpenCode dispatch-only proof now uses pane state transition detection. When a routed trigger is submitted to an OpenCode pane, agent-doc polls for the pane to transition from idle chrome to active output. A detected state change yields `PaneStateChanged` proof; timeout yields `CommandAcceptedOnly`.

## Files to Modify

| File | Change |
|---|---|
| `src/agent-doc/src/route.rs` | Extend tracker builder, add pane state polling, thread tmux/pane through callers |
| `src/agent-doc/src/flow/routed_reopen.rs` | Add `PaneStateChanged` variant, update policy/classification |
| `src/agent-doc/src/harness.rs` | Add OpenCode busy cue detection |
| `src/agent-doc/SPEC.md` | Document new proof mechanism |

## Test Plan

1. **Unit test:** `build_routed_dispatch_start_tracker_returns_tracker_for_opencode`
2. **Unit test:** `opencode_pane_state_change_yields_pane_state_changed_proof`
3. **Unit test:** `opencode_pane_state_timeout_yields_accepted_only`
4. **Unit test:** `classify_dispatch_start_proof_accepts_pane_state_changed`
5. **Unit test:** `dispatch_only_dispatch_start_proof_required_opencode_with_tracker_returns_true`
6. **Integration:** tmux CI dispatch-only reopen for OpenCode harness
7. **SimWorld:** Deterministic pane state transition simulation

## Risks and Mitigations

| Risk | Mitigation |
|---|---|
| OpenCode TUI redraw latency exceeds 10s proof timeout | Extend to 15s for OpenCode (matches existing redraw budget) |
| Intermediate TUI states during redraw cause false positives | Use `is_idle_chrome_only_output` as the idle check, require full departure from idle |
| Permission prompts misclassified as idle | `parse_prompt(output).active` already detects permission prompts |
| `capture-pane` overhead in tight 200ms poll loop | OpenCode redraw is slower; use 500ms poll interval (matches `wait_for_agent_ready_outcome`) |
| Pane content hash collisions on identical-length idle output | Use full content comparison, not just hash |

## Future Improvements (out of scope)

- OpenCode hook mechanism (requires OpenCode to expose lifecycle events)
- Supervisor screen model integration for managed sessions
- OpenCode session log parsing for proof
