//! Compatibility facade for the identified queue graph owner.
//!
//! New policy belongs in [`crate::queue_graph`]. This module remains only so
//! callers can migrate without creating a second scheduling owner.

pub use crate::queue_graph::*;
