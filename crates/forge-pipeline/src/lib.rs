//! Conversion pipeline orchestrating format readers and writers.
//!
//! The pipeline reads episodes from a source dataset format, applies
//! field and camera remapping, and writes to a target format. The
//! [`Converter`] type coordinates the full read-transform-write flow
//! using the canonical [`forge_core::Episode`] as the intermediate
//! representation.
//!
//! Parallel processing via rayon will be added in Phase 3.

pub mod convert;

pub use convert::{ConversionResult, Converter};
