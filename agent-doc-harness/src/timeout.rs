//! Harness error classification helpers.

/// Return true when an error chain represents a harness dispatch timeout.
///
/// Callers still own the recovery action and logging. This helper only
/// classifies the error chain so timeout recovery stays consistent across
/// direct-run adapters.
pub fn error_chain_is_timeout(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .is_some_and(|io| io.kind() == std::io::ErrorKind::TimedOut)
            || cause.to_string().contains("timed out")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeout_error_matches_timed_out_io_kind() {
        let err = anyhow::Error::new(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "backend wait exceeded",
        ));

        assert!(error_chain_is_timeout(&err));
    }

    #[test]
    fn timeout_error_matches_context_message() {
        let err = anyhow::anyhow!("operation timed out while reading response");

        assert!(error_chain_is_timeout(&err));
    }

    #[test]
    fn timeout_error_rejects_other_io_errors() {
        let err = anyhow::Error::new(std::io::Error::new(
            std::io::ErrorKind::ConnectionRefused,
            "agent unavailable",
        ));

        assert!(!error_chain_is_timeout(&err));
    }
}
