//! GH #339 R1 controller identity: the JetBrains `Register` op carries the backend
//! JVM's `agent_doc_activation_receipt/v1` line.
//!
//! The fixture is the exact op the plugin's `ReliableSyncLivenessGraph` builds;
//! `ReliableSyncLivenessGraphTest` (editors/jetbrains) asserts the plugin still
//! produces it byte for byte, so the two sides cannot drift apart silently.

use agent_doc_reliable_sync_io::liveness::{ActivationRole, LivenessOp};

const FIXTURE: &str = include_str!(
    "../../editors/jetbrains/src/test/resources/reliable-sync/jetbrains-register-with-receipt.json"
);

#[test]
fn jetbrains_register_op_carries_the_backend_activation_receipt() {
    let LivenessOp::Register(registration) =
        serde_json::from_str(FIXTURE.trim()).expect("the JetBrains register op decodes")
    else {
        panic!("the fixture is a register op");
    };
    assert_eq!(registration.editor_kind, "jetbrains");
    let receipt = registration
        .activation()
        .expect("the registration carries a receipt")
        .expect("the receipt parses");
    assert_eq!(receipt.role, ActivationRole::Backend);
    assert_eq!(receipt.spi, 1);
    assert_eq!(receipt.impl_version.as_deref(), Some("0.2.851"));
    // R1 has no separate implementation jar, so nothing was verified by sha256.
    assert_eq!(receipt.impl_sha256, None);
    assert_eq!((receipt.generation, receipt.epoch), (1, 0));
    assert_eq!(
        Some(receipt.encode().as_str()),
        registration.activation_receipt.as_deref(),
        "the controller keeps the plugin's line verbatim",
    );
    assert_eq!(
        serde_json::to_string(&LivenessOp::Register(registration)).unwrap(),
        FIXTURE.trim(),
        "the controller re-encodes the plugin's op unchanged",
    );
}
