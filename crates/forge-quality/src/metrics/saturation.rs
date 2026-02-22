//! Action saturation metric.
//!
//! Tracks per-dimension running min/max of action values and counts frames
//! where at least one dimension is at (or very near) an observed extreme.
//! High saturation ratios indicate clipped or railed actuator commands.

use crate::metrics::{FrameContext, MetricAccumulator, MetricDetails, MetricKind, MetricResult};

/// Streaming action saturation accumulator.
///
/// Maintains per-dimension running min and max values. A frame is
/// classified as "saturated" if any dimension's value is within
/// `epsilon` of its observed min or max (i.e., at a limit).
///
/// Memory: O(D) where D is the action dimensionality.
pub struct SaturationAccumulator {
    /// Per-dimension running minimum values.
    min_vals: Vec<f64>,
    /// Per-dimension running maximum values.
    max_vals: Vec<f64>,
    /// Number of frames classified as saturated.
    saturated_count: usize,
    /// Total frames processed.
    total_count: usize,
    /// Number of action dimensions.
    dims: usize,
    /// Tolerance for matching min/max values.
    epsilon: f64,
}

impl Default for SaturationAccumulator {
    fn default() -> Self {
        Self::new()
    }
}

impl SaturationAccumulator {
    /// Create a new saturation accumulator.
    pub fn new() -> Self {
        Self {
            min_vals: Vec::new(),
            max_vals: Vec::new(),
            saturated_count: 0,
            total_count: 0,
            dims: 0,
            epsilon: 1e-6,
        }
    }
}

impl MetricAccumulator for SaturationAccumulator {
    fn push_frame(&mut self, ctx: &FrameContext<'_>) {
        let action = match ctx.action {
            Some(a) if !a.is_empty() => a,
            _ => return,
        };

        // Initialize dimensions on first frame.
        if self.dims == 0 {
            self.dims = action.len();
            self.min_vals = action.to_vec();
            self.max_vals = action.to_vec();
            self.total_count += 1;
            // First frame is trivially at both min and max, count it as saturated.
            self.saturated_count += 1;
            return;
        }

        let d = self.dims.min(action.len());
        self.total_count += 1;

        // Check saturation BEFORE updating min/max.
        // A frame is saturated if any dimension is at its current observed limit.
        let is_saturated = action.iter().take(d).enumerate().any(|(i, &val)| {
            (val - self.min_vals[i]).abs() < self.epsilon
                || (val - self.max_vals[i]).abs() < self.epsilon
        });

        if is_saturated {
            self.saturated_count += 1;
        }

        // Update running min/max.
        for (&val, (mn, mx)) in action.iter().take(d).zip(
            self.min_vals.iter_mut().zip(self.max_vals.iter_mut()),
        ) {
            if val < *mn {
                *mn = val;
            }
            if val > *mx {
                *mx = val;
            }
        }
    }

    fn finalize(&mut self) -> MetricResult {
        if self.total_count == 0 {
            return MetricResult {
                kind: MetricKind::ActionSaturation,
                normalized_score: None,
                raw_value: None,
                details: MetricDetails::None,
                warning: Some("no action data available".to_string()),
            };
        }

        let saturated_ratio = self.saturated_count as f64 / self.total_count as f64;
        // Score: 1.0 = no saturation (best), 0.0 = all saturated (worst).
        let score = 1.0 - saturated_ratio;

        MetricResult {
            kind: MetricKind::ActionSaturation,
            normalized_score: Some(score),
            raw_value: Some(saturated_ratio),
            details: MetricDetails::ActionSaturation { saturated_ratio },
            warning: None,
        }
    }

    fn kind(&self) -> MetricKind {
        MetricKind::ActionSaturation
    }

    fn score_bounds(&self) -> (f64, f64) {
        (0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::FrameContext;

    fn make_ctx<'a>(frame_index: usize, action: &'a [f64]) -> FrameContext<'a> {
        FrameContext {
            frame_index,
            action: Some(action),
            state: None,
            timestamp: None,
            dt: 1.0 / 30.0,
        }
    }

    #[test]
    fn extreme_actions_low_score() {
        let mut acc = SaturationAccumulator::new();

        // Actions alternating between two extreme values.
        for i in 0..100 {
            let val = if i % 2 == 0 { -1.0 } else { 1.0 };
            acc.push_frame(&make_ctx(i, &[val, val, val]));
        }

        let result = acc.finalize();
        assert!(result.normalized_score.is_some());
        let score = result.normalized_score.unwrap();
        // Most frames hit min or max.
        assert!(
            score < 0.1,
            "saturated actions should score low, got {score}"
        );
    }

    #[test]
    fn midrange_actions_high_score() {
        let mut acc = SaturationAccumulator::new();

        // First, establish range with extreme values.
        acc.push_frame(&make_ctx(0, &[-1.0, -1.0, -1.0]));
        acc.push_frame(&make_ctx(1, &[1.0, 1.0, 1.0]));

        // Then, feed many mid-range actions.
        for i in 2..102 {
            let t = i as f64 * 0.01;
            let action = vec![t.sin() * 0.3, t.cos() * 0.3, 0.0];
            acc.push_frame(&make_ctx(i, &action));
        }

        let result = acc.finalize();
        assert!(result.normalized_score.is_some());
        let score = result.normalized_score.unwrap();
        // Most frames are mid-range, only the first 2 are at extremes.
        assert!(
            score > 0.9,
            "mid-range actions should score high, got {score}"
        );
    }

    #[test]
    fn midrange_scores_higher_than_saturated() {
        // Saturated.
        let mut sat_acc = SaturationAccumulator::new();
        for i in 0..100 {
            let val = if i % 2 == 0 { -1.0 } else { 1.0 };
            sat_acc.push_frame(&make_ctx(i, &[val, val]));
        }
        let sat_score = sat_acc.finalize().normalized_score.unwrap();

        // Mid-range.
        let mut mid_acc = SaturationAccumulator::new();
        mid_acc.push_frame(&make_ctx(0, &[-1.0, -1.0]));
        mid_acc.push_frame(&make_ctx(1, &[1.0, 1.0]));
        for i in 2..102 {
            let t = i as f64 * 0.01;
            mid_acc.push_frame(&make_ctx(i, &[t.sin() * 0.3, t.cos() * 0.3]));
        }
        let mid_score = mid_acc.finalize().normalized_score.unwrap();

        assert!(
            mid_score > sat_score,
            "mid-range ({mid_score}) should score higher than saturated ({sat_score})"
        );
    }

    #[test]
    fn no_data_returns_none() {
        let mut acc = SaturationAccumulator::new();
        let result = acc.finalize();
        assert!(result.normalized_score.is_none());
        assert!(result.warning.is_some());
    }
}
