//! Streaming quality metric accumulators.
//!
//! Defines the [`MetricAccumulator`] trait for single-pass metric computation,
//! the [`MetricKind`] enum identifying all quality metrics, and supporting types
//! ([`FrameContext`], [`MetricResult`], [`MetricDetails`]).

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::config::QualityConfig;

/// Identifies a specific quality metric.
///
/// Each variant corresponds to one dimension of robotics episode quality.
/// Used as keys in weight maps, skip lists, and metric score lookups.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MetricKind {
    Smoothness,
    DeadActions,
    GripperChatter,
    StaticDetection,
    TimestampRegularity,
    ActionSaturation,
    ActionEntropy,
}

impl MetricKind {
    /// Returns a static slice of all metric variants.
    pub fn all() -> &'static [MetricKind] {
        &[
            MetricKind::Smoothness,
            MetricKind::DeadActions,
            MetricKind::GripperChatter,
            MetricKind::StaticDetection,
            MetricKind::TimestampRegularity,
            MetricKind::ActionSaturation,
            MetricKind::ActionEntropy,
        ]
    }
}

impl fmt::Display for MetricKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            MetricKind::Smoothness => "smoothness",
            MetricKind::DeadActions => "dead_actions",
            MetricKind::GripperChatter => "gripper_chatter",
            MetricKind::StaticDetection => "static_detection",
            MetricKind::TimestampRegularity => "timestamp_regularity",
            MetricKind::ActionSaturation => "action_saturation",
            MetricKind::ActionEntropy => "action_entropy",
        };
        write!(f, "{name}")
    }
}

/// Per-frame data passed to metric accumulators.
///
/// Borrows slices from the episode's lazily-loaded arrays. Each frame context
/// carries the action and state vectors (if available), the timestamp, and
/// the time delta between frames.
pub struct FrameContext<'a> {
    /// Zero-based frame index within the episode.
    pub frame_index: usize,
    /// Action vector for this frame (borrowed from `LazyArray::load()`).
    pub action: Option<&'a [f64]>,
    /// State/observation vector for this frame (borrowed from `LazyArray::load()`).
    pub state: Option<&'a [f64]>,
    /// Absolute timestamp for this frame (seconds), if available.
    pub timestamp: Option<f64>,
    /// Time delta between frames: `1.0 / fps`.
    pub dt: f64,
}

/// Additional metric-specific details attached to a [`MetricResult`].
///
/// Provides raw diagnostic values beyond the normalized score, enabling
/// downstream consumers to understand *why* a score was assigned.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MetricDetails {
    /// No extra details available.
    None,
    /// Raw LDLJ value before normalization.
    Smoothness { ldlj_value: f64 },
    /// Dead action ratio and dimension counts.
    DeadActions {
        dead_ratio: f64,
        dead_dims: usize,
        total_dims: usize,
    },
    /// Gripper transition count and rate.
    GripperChatter {
        transitions: usize,
        rate: f64,
    },
    /// Ratio of static (non-moving) frames.
    StaticDetection { static_ratio: f64 },
    /// Timestamp regularity statistics.
    TimestampRegularity {
        mean_dt: f64,
        std_dt: f64,
        jitter_ratio: f64,
        gap_count: usize,
    },
    /// Ratio of action dimensions at saturation limits.
    ActionSaturation { saturated_ratio: f64 },
    /// Mean per-dimension entropy of the action distribution.
    ActionEntropy { mean_entropy: f64 },
}

/// Result of a single metric computation for one episode.
///
/// Contains both the normalized 0-1 score and the raw metric value,
/// plus optional details and warnings.
#[derive(Debug, Clone)]
pub struct MetricResult {
    /// Which metric produced this result.
    pub kind: MetricKind,
    /// Normalized score in `[0.0, 1.0]` (higher = better). `None` if
    /// the metric could not be computed (e.g., missing data).
    pub normalized_score: Option<f64>,
    /// Raw metric output value before normalization.
    pub raw_value: Option<f64>,
    /// Metric-specific diagnostic details.
    pub details: MetricDetails,
    /// Warning message if the metric was degraded or skipped.
    pub warning: Option<String>,
}

/// Streaming metric accumulator trait.
///
/// Implementations receive frames one at a time via [`push_frame`](MetricAccumulator::push_frame)
/// and produce a final [`MetricResult`] via [`finalize`](MetricAccumulator::finalize).
/// This design enables single-pass O(1)-memory metric computation.
pub trait MetricAccumulator: Send {
    /// Ingest one frame of data. Called in frame order (index 0, 1, 2, ...).
    fn push_frame(&mut self, ctx: &FrameContext<'_>);

    /// Finalize the metric and return the result. Called once after all frames.
    fn finalize(&mut self) -> MetricResult;

    /// Which metric this accumulator computes.
    fn kind(&self) -> MetricKind;

    /// Report current score bounds for early-exit optimization.
    ///
    /// Returns `(min_possible, max_possible)` given frames seen so far.
    /// Default returns `(0.0, 1.0)` (no bound information).
    fn score_bounds(&self) -> (f64, f64) {
        (0.0, 1.0)
    }
}

/// Create the set of metric accumulators for a given configuration.
///
/// Returns one accumulator per non-skipped metric. Concrete implementations
/// will be added in Plan 02; this currently returns an empty vector.
pub fn create_accumulators(_config: &QualityConfig) -> Vec<Box<dyn MetricAccumulator>> {
    Vec::new()
}
