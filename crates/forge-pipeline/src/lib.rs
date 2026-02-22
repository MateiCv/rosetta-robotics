//! Parallel conversion pipeline orchestrating format readers and writers.
//!
//! The pipeline reads episodes from a source dataset format, applies
//! field and camera remapping in parallel via rayon, and writes to a
//! target format. The [`Converter`] type coordinates the full
//! read-transform-write flow using the canonical [`forge_core::Episode`]
//! as the intermediate representation.
//!
//! Episode processing uses a two-phase pattern:
//! 1. **Parallel read + remap** via rayon work-stealing thread pool
//! 2. **Sequential write** preserving episode order for deterministic output

pub mod convert;
pub mod error_collector;
pub mod prefetch;
pub mod progress;

pub use convert::{ConversionResult, Converter};
pub use error_collector::{ErrorCollector, print_failure_summary};
pub use prefetch::prefetch_episodes;
pub use progress::{ProgressMode, ProgressReporter};
