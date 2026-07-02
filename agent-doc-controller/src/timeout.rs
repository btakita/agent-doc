//! Pure project-controller IO timeout classification.

use std::io::{Error, ErrorKind};

pub fn is_timeout_error(err: &Error) -> bool {
    matches!(err.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_project_controller_io_timeouts() {
        assert!(is_timeout_error(&Error::from(ErrorKind::TimedOut)));
        assert!(is_timeout_error(&Error::from(ErrorKind::WouldBlock)));

        assert!(!is_timeout_error(&Error::from(ErrorKind::Interrupted)));
        assert!(!is_timeout_error(&Error::from(ErrorKind::UnexpectedEof)));
    }
}
