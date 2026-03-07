//! Quality analysis metrics and episode filtering for robotics datasets.
//!
//! This crate provides streaming, single-pass quality metrics for robotics
//! episodes (LDLJ smoothness, dead actions, gripper chatter, static detection,
//! timestamp regularity, action saturation, entropy) and episode filtering
//! by quality scores.
//!
//! # Architecture
//!
//! - **[`config`]**: Quality and filter configuration with YAML loading.
//! - **[`models`]**: Serializable report and result types for JSON export.
//! - **[`metrics`]**: Streaming [`MetricAccumulator`](metrics::MetricAccumulator) trait
//!   and [`MetricKind`](metrics::MetricKind) enum.
//! - **[`analyzer`]**: [`QualityAnalyzer`](analyzer::QualityAnalyzer) orchestrating
//!   metrics across episodes with early exit and JSON export.
//! - **[`simd`]**: SIMD-accelerated kernel functions (jerk, histogram, entropy, dot product).

pub mod analyzer;
pub mod config;
pub mod early_exit;
pub mod filter;
pub mod metrics;
pub mod models;
pub mod simd;

// Re-export key types for convenience.
pub use analyzer::QualityAnalyzer;
pub use config::{FilterConfig, QualityConfig};
pub use filter::FilterEngine;
pub use metrics::{MetricKind, MetricResult};
pub use models::{EpisodeQuality, FilterResult, MetricScore, QualityReport};
