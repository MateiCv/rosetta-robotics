//! Core data models for the Rosetta robotics dataset converter.
//!
//! This crate defines the canonical intermediate representation used by all
//! format readers and writers: [`Episode`], [`Frame`], [`LazyArray`],
//! [`LazyImage`], [`DatasetInfo`], [`RosettaError`], and [`ConversionConfig`].
//!
//! Dependencies are intentionally minimal (serde, thiserror, bytes) to keep
//! compilation fast and avoid coupling to format-specific libraries.

pub mod config;
pub mod episode;
pub mod error;
pub mod frame;
pub mod lazy;
pub mod schema;

// Top-level re-exports for convenience.
pub use config::ConversionConfig;
pub use episode::Episode;
pub use error::{EpisodeFailure, RosettaError, RosettaResult, ReadOutput, ReadWarning, WarningSeverity};
pub use frame::Frame;
pub use lazy::{ImageEncoding, LazyArray, LazyImage};
pub use schema::{CameraInfo, DatasetInfo, Dtype, FieldSchema};
