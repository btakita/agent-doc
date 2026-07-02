//! Filesystem/process adapters for editor plugin-owner cleanup.
//!
//! `agent-doc-plugin-owner` owns the editor-owner vocabulary and cleanup policy.
//! This crate owns the effectful traversal/removal adapters used by hook
//! closeout and realtime cleanup paths.

pub mod stale_cleanup;
