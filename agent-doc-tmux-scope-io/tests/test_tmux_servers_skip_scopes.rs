//! `#tmuxscopesteward` prevention guard: every Makefile test target must give
//! its processes (and so every tmux server a test spawns) an unreachable
//! session D-Bus, because that is the only switch that stops tmux 3.8 from
//! creating a transient systemd scope per test pane. Install and release
//! targets must keep the real bus.

use std::path::Path;

const TEST_TARGETS: &[&str] = &[
    "check",
    "check-fast",
    "test",
    "tmux-ci",
    "sim-medium",
    "sim-net",
    "cross-editor-simworld",
];

fn makefile() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../Makefile");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

#[test]
fn makefile_test_targets_export_an_unreachable_session_bus() {
    let text = makefile();
    assert!(
        text.lines()
            .any(|line| line.starts_with("TEST_SESSION_BUS_ADDRESS := unix:path=/nonexistent/")),
        "TEST_SESSION_BUS_ADDRESS must name an unreachable unix socket"
    );
    let export = text
        .lines()
        .find(|line| {
            line.contains(": export DBUS_SESSION_BUS_ADDRESS = $(TEST_SESSION_BUS_ADDRESS)")
        })
        .expect("test targets must export DBUS_SESSION_BUS_ADDRESS");
    let targets: Vec<&str> = export
        .split(':')
        .next()
        .unwrap_or_default()
        .split_whitespace()
        .collect();
    for target in TEST_TARGETS {
        assert!(
            targets.contains(target),
            "`{target}` must export the unreachable bus: {export}"
        );
    }
    for target in ["install", "install-full", "release", "build-release"] {
        assert!(
            !targets.contains(&target),
            "`{target}` must keep the real session bus"
        );
    }
    assert!(
        !text
            .lines()
            .any(|line| line.starts_with("export DBUS_SESSION_BUS_ADDRESS")),
        "the bus override must stay target-specific"
    );
}

/// Under `make` (the normal `make test` path) the test process itself sees the
/// unreachable address.
#[test]
fn make_applied_the_unreachable_session_bus_to_this_test_process() {
    if std::env::var_os("MAKELEVEL").is_none() {
        return;
    }
    let value = std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap_or_default();
    assert!(
        value.starts_with("unix:path=/nonexistent/"),
        "tests must not reach the session bus (got {value:?})"
    );
}
