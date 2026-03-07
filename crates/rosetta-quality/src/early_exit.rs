//! Early exit decision logic for quality analysis.
//!
//! Determines whether an episode's quality can be conclusively decided
//! before all frames have been processed, by checking if the composite
//! score bounds are both above or both below the threshold.

use std::collections::HashMap;

use crate::metrics::composite::composite_bounds;
use crate::metrics::{MetricAccumulator, MetricKind};

/// Decision from early exit evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EarlyExitDecision {
    /// Composite score is guaranteed >= threshold. Episode definitely passes.
    Pass,
    /// Composite score is guaranteed < threshold. Episode definitely fails.
    Fail,
    /// Outcome is still uncertain. Continue processing frames.
    Continue,
}

/// Check whether the episode quality can be decided early.
///
/// Uses the optimistic and pessimistic bounds from all accumulators to
/// determine if the composite score is guaranteed to pass or fail the
/// given threshold, regardless of future frames.
///
/// # Arguments
///
/// * `accumulators` - Current metric accumulators with partial frame data.
/// * `weights` - Per-metric weights for composite score computation.
/// * `threshold` - Composite score threshold for pass/fail.
///
/// # Returns
///
/// - [`EarlyExitDecision::Pass`] if `min_composite >= threshold`
/// - [`EarlyExitDecision::Fail`] if `max_composite < threshold`
/// - [`EarlyExitDecision::Continue`] otherwise
pub fn check_early_exit(
    accumulators: &[Box<dyn MetricAccumulator>],
    weights: &HashMap<MetricKind, f64>,
    threshold: f64,
) -> EarlyExitDecision {
    let (min_composite, max_composite) = composite_bounds(accumulators, weights);

    if min_composite >= threshold {
        EarlyExitDecision::Pass
    } else if max_composite < threshold {
        EarlyExitDecision::Fail
    } else {
        EarlyExitDecision::Continue
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::{FrameContext, MetricDetails, MetricResult};

    /// Stub accumulator that reports fixed score bounds.
    struct FixedBoundsAccumulator {
        kind: MetricKind,
        bounds: (f64, f64),
    }

    impl MetricAccumulator for FixedBoundsAccumulator {
        fn push_frame(&mut self, _ctx: &FrameContext<'_>) {}

        fn finalize(&mut self) -> MetricResult {
            MetricResult {
                kind: self.kind,
                normalized_score: Some((self.bounds.0 + self.bounds.1) / 2.0),
                raw_value: None,
                details: MetricDetails::None,
                warning: None,
            }
        }

        fn kind(&self) -> MetricKind {
            self.kind
        }

        fn score_bounds(&self) -> (f64, f64) {
            self.bounds
        }
    }

    fn make_accumulators(specs: &[(MetricKind, (f64, f64))]) -> Vec<Box<dyn MetricAccumulator>> {
        specs
            .iter()
            .map(|&(kind, bounds)| -> Box<dyn MetricAccumulator> {
                Box::new(FixedBoundsAccumulator { kind, bounds })
            })
            .collect()
    }

    #[test]
    fn all_high_bounds_returns_pass() {
        let accumulators = make_accumulators(&[
            (MetricKind::Smoothness, (0.8, 1.0)),
            (MetricKind::DeadActions, (0.8, 1.0)),
            (MetricKind::ActionEntropy, (0.8, 1.0)),
        ]);
        let mut weights = HashMap::new();
        weights.insert(MetricKind::Smoothness, 0.5);
        weights.insert(MetricKind::DeadActions, 0.3);
        weights.insert(MetricKind::ActionEntropy, 0.2);

        // min_composite = 0.8 (all mins are 0.8), threshold = 0.5
        let decision = check_early_exit(&accumulators, &weights, 0.5);
        assert_eq!(decision, EarlyExitDecision::Pass);
    }

    #[test]
    fn all_low_bounds_returns_fail() {
        let accumulators = make_accumulators(&[
            (MetricKind::Smoothness, (0.0, 0.3)),
            (MetricKind::DeadActions, (0.0, 0.3)),
            (MetricKind::ActionEntropy, (0.0, 0.3)),
        ]);
        let mut weights = HashMap::new();
        weights.insert(MetricKind::Smoothness, 0.5);
        weights.insert(MetricKind::DeadActions, 0.3);
        weights.insert(MetricKind::ActionEntropy, 0.2);

        // max_composite = 0.3, threshold = 0.5
        let decision = check_early_exit(&accumulators, &weights, 0.5);
        assert_eq!(decision, EarlyExitDecision::Fail);
    }

    #[test]
    fn mixed_wide_bounds_returns_continue() {
        let accumulators = make_accumulators(&[
            (MetricKind::Smoothness, (0.2, 0.9)),
            (MetricKind::DeadActions, (0.1, 0.8)),
            (MetricKind::ActionEntropy, (0.3, 0.7)),
        ]);
        let mut weights = HashMap::new();
        weights.insert(MetricKind::Smoothness, 0.5);
        weights.insert(MetricKind::DeadActions, 0.3);
        weights.insert(MetricKind::ActionEntropy, 0.2);

        // min_composite < 0.5, max_composite > 0.5 => Continue
        let decision = check_early_exit(&accumulators, &weights, 0.5);
        assert_eq!(decision, EarlyExitDecision::Continue);
    }

    #[test]
    fn exact_threshold_boundary_is_pass() {
        // min_composite exactly equals threshold => Pass
        let accumulators = make_accumulators(&[
            (MetricKind::Smoothness, (0.5, 0.5)),
            (MetricKind::DeadActions, (0.5, 0.5)),
        ]);
        let mut weights = HashMap::new();
        weights.insert(MetricKind::Smoothness, 0.5);
        weights.insert(MetricKind::DeadActions, 0.5);

        let decision = check_early_exit(&accumulators, &weights, 0.5);
        assert_eq!(decision, EarlyExitDecision::Pass);
    }
}
