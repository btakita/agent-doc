//! Compatibility re-exports for the shared lazily durable outbox.
//!
//! Keeping this module path avoids a flag day for agent-doc callers while the
//! storage-independent protocol and SQLite implementation live in lazily.

pub use lazily::{SqliteOutbox, SqliteStore, SqliteStoreError, ensure_outbox_schema};
