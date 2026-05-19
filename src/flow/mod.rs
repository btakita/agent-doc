//! Typed flow contracts for agent-doc hot paths.
//!
//! The first implementation phase is intentionally mirror-mode: existing command
//! modules still own behavior, while the flow layer provides pure decisions and
//! typed events that those modules can emit and test without tmux.

#![allow(dead_code)]

pub(crate) mod closeout;
pub(crate) mod document_mutation;
pub(crate) mod operator_clear;
pub(crate) mod orchestration_batch;
pub(crate) mod proof;
pub(crate) mod routed_reopen;
pub(crate) mod session_cycle;
pub(crate) mod types;
