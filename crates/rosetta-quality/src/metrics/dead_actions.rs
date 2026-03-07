//! Dead action detection metric.
//!
//! Detects episodes where action dimensions remain constant (stuck at their
//! initial value or at zero), indicating a malfunctioning controller or
//! recording artifact.

use crate::metrics::{FrameContext, MetricAccumulator, MetricDetails, MetricKind, MetricResult};

/// Streaming dead action accumulator.
///
/// Compares each frame's action values against the first frame's values.
/// A frame is "dead" if all dimensions are within `dead_threshold` of the
/// first-frame values AND within `dead_threshold` of zero.
///
/// Memory: O(D) where D is the action dimensionality.
pub struct DeadActionsAccumulator {
    /// Action values from the first frame (for reference comparison).
    first_action: Option<Vec<f64>>,
    /// Number of frames classified as dead.
    dead_frame_count: usize,
    /// Total frames with action data.
    total_frames: usize,
    /// Number of action dimensions.
    dims: usize,
    /// Threshold below which a per-dimension difference is considered "dead".
    dead_threshold: f64,
}

impl Default for DeadActionsAccumulator {
    fn default() -> Self {
        Self::new()
    }
}

impl DeadActionsAccumulator {
    /// Create a new dead actions accumulator with default threshold (1e-6).
    pub fn new() -> Self {
        Self {
            first_action: None,
            dead_frame_count: 0,
            total_frames: 0,
            dims: 0,
            dead_threshold: 1e-6,
        }
    }
}

impl MetricAccumulator for DeadActionsAccumulator {
    fn push_frame(&mut self, ctx: &FrameContext<'_>) {
        let action = match ctx.action {
            Some(a) if !a.is_empty() => a,
            _ => return,
        };

        self.total_frames += 1;

        if self.first_action.is_none() {
            self.dims = action.len();
            self.first_action = Some(action.to_vec());
            // First frame: check if it's all zeros (dead from the start).
            let all_zero = action.iter().all(|&v| v.abs() < self.dead_threshold);
            if all_zero {
                self.dead_frame_count += 1;
            }
            return;
        }

        let first = self.first_action.as_ref().unwrap();
        let d = self.dims.min(action.len());

        // A frame is dead if all dimensions match the first frame AND are near zero.
        let matches_first = (0..d).all(|i| (action[i] - first[i]).abs() < self.dead_threshold);
        let near_zero = (0..d).all(|i| action[i].abs() < self.dead_threshold);

        if matches_first && near_zero {
            self.dead_frame_count += 1;
        }
    }

    fn finalize(&mut self) -> MetricResult {
        if self.total_frames == 0 {
            return MetricResult {
                kind: MetricKind::DeadActions,
                normalized_score: None,
                raw_value: None,
                details: MetricDetails::None,
                warning: Some("no action data available".to_string()),
            };
        }

        let dead_ratio = self.dead_frame_count as f64 / self.total_frames as f64;
        // Score: 1.0 = no dead frames (best), 0.0 = all dead frames (worst).
        let score = 1.0 - dead_ratio;

        // Count dead dimensions (dims that are constant across first frame).
        let dead_dims = if let Some(ref first) = self.first_action {
            first
                .iter()
                .filter(|v| v.abs() < self.dead_threshold)
                .count()
        } else {
            0
        };

        MetricResult {
            kind: MetricKind::DeadActions,
            normalized_score: Some(score),
            raw_value: Some(dead_ratio),
            details: MetricDetails::DeadActions {
                dead_ratio,
                dead_dims,
                total_dims: self.dims,
            },
            warning: None,
        }
    }

    fn kind(&self) -> MetricKind {
        MetricKind::DeadActions
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
    fn all_zero_actions_score_near_zero() {
        let mut acc = DeadActionsAccumulator::new();

        for i in 0..50 {
            let action = vec![0.0, 0.0, 0.0];
            acc.push_frame(&make_ctx(i, &action));
        }

        let result = acc.finalize();
        assert!(result.normalized_score.is_some());
        let score = result.normalized_score.unwrap();
        assert!(
            score < 0.01,
            "constant-zero actions should score near 0.0, got {score}"
        );

        if let MetricDetails::DeadActions { dead_ratio, .. } = result.details {
            assert!(
                (dead_ratio - 1.0).abs() < 1e-10,
                "dead_ratio should be 1.0, got {dead_ratio}"
            );
        } else {
            panic!("expected DeadActions details");
        }
    }

    #[test]
    fn varied_actions_score_near_one() {
        let mut acc = DeadActionsAccumulator::new();

        for i in 0..50 {
            let t = i as f64 * 0.1;
            let action = vec![t.sin(), t.cos(), t * 0.1];
            acc.push_frame(&make_ctx(i, &action));
        }

        let result = acc.finalize();
        assert!(result.normalized_score.is_some());
        let score = result.normalized_score.unwrap();
        assert!(
            score > 0.9,
            "varied actions should score near 1.0, got {score}"
        );

        if let MetricDetails::DeadActions { dead_ratio, .. } = result.details {
            assert!(
                dead_ratio < 0.1,
                "dead_ratio should be near 0.0, got {dead_ratio}"
            );
        } else {
            panic!("expected DeadActions details");
        }
    }

    #[test]
    fn no_frames_returns_none() {
        let mut acc = DeadActionsAccumulator::new();
        let result = acc.finalize();
        assert!(result.normalized_score.is_none());
        assert!(result.warning.is_some());
    }

    #[test]
    fn mixed_dead_and_active_frames() {
        let mut acc = DeadActionsAccumulator::new();

        // First 25 frames: dead (all zeros).
        for i in 0..25 {
            acc.push_frame(&make_ctx(i, &[0.0, 0.0, 0.0]));
        }
        // Next 25 frames: active.
        for i in 25..50 {
            let t = i as f64 * 0.1;
            acc.push_frame(&make_ctx(i, &[t.sin(), t.cos(), t * 0.1]));
        }

        let result = acc.finalize();
        let score = result.normalized_score.unwrap();
        // 25 dead out of 50 => dead_ratio = 0.5 => score = 0.5
        assert!(
            (score - 0.5).abs() < 0.05,
            "half-dead episode should score near 0.5, got {score}"
        );
    }
}
