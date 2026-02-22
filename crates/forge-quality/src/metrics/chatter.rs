//! Gripper chatter detection metric.
//!
//! Counts rapid binary state transitions of the gripper (open/close) and
//! computes a transition rate. High chatter rates indicate noisy gripper
//! commands or controller instability.

use crate::metrics::{FrameContext, MetricAccumulator, MetricDetails, MetricKind, MetricResult};

/// Streaming gripper chatter accumulator.
///
/// Binarizes the gripper dimension (> threshold = open, <= threshold = closed)
/// and counts transitions between states. The transition rate (per second)
/// is normalized to a 0-1 score.
///
/// Memory: O(1).
pub struct ChatterAccumulator {
    /// Previous gripper binary state (true = open, false = closed).
    prev_gripper_binary: Option<bool>,
    /// Total number of open/close transitions.
    transitions: usize,
    /// Total frames processed.
    frame_count: usize,
    /// Which action dimension is the gripper (None = last dim).
    gripper_dim: Option<usize>,
    /// Threshold for binarizing gripper value.
    gripper_threshold: f64,
    /// Time delta between frames (seconds).
    dt: f64,
}

impl ChatterAccumulator {
    /// Create a new chatter accumulator.
    ///
    /// # Arguments
    /// * `gripper_dim` - Index of the gripper action dimension, or `None` to
    ///   use the last dimension.
    pub fn new(gripper_dim: Option<usize>) -> Self {
        Self {
            prev_gripper_binary: None,
            transitions: 0,
            frame_count: 0,
            gripper_dim,
            gripper_threshold: 0.5,
            dt: 1.0 / 30.0,
        }
    }

    /// Create with explicit dt.
    pub fn with_dt(gripper_dim: Option<usize>, dt: f64) -> Self {
        Self {
            prev_gripper_binary: None,
            transitions: 0,
            frame_count: 0,
            gripper_dim,
            gripper_threshold: 0.5,
            dt,
        }
    }
}

impl MetricAccumulator for ChatterAccumulator {
    fn push_frame(&mut self, ctx: &FrameContext<'_>) {
        let action = match ctx.action {
            Some(a) if !a.is_empty() => a,
            _ => return,
        };

        // Determine gripper dimension index.
        let dim = self.gripper_dim.unwrap_or(action.len() - 1);
        if dim >= action.len() {
            return;
        }

        let gripper_val = action[dim];
        let current_binary = gripper_val > self.gripper_threshold;

        if let Some(prev) = self.prev_gripper_binary {
            if prev != current_binary {
                self.transitions += 1;
            }
        }

        self.prev_gripper_binary = Some(current_binary);
        self.frame_count += 1;
    }

    fn finalize(&mut self) -> MetricResult {
        if self.frame_count == 0 {
            return MetricResult {
                kind: MetricKind::GripperChatter,
                normalized_score: None,
                raw_value: None,
                details: MetricDetails::None,
                warning: Some("skipped, no gripper data".to_string()),
            };
        }

        let duration = self.frame_count as f64 * self.dt;
        let rate = if duration > 0.0 {
            self.transitions as f64 / duration
        } else {
            0.0
        };

        // Normalize: low rate = good (no chatter).
        // max_rate = 10.0 transitions/sec is considered terrible chatter.
        let max_rate = 10.0;
        let score = (1.0 - (rate / max_rate).clamp(0.0, 1.0)).max(0.0);

        MetricResult {
            kind: MetricKind::GripperChatter,
            normalized_score: Some(score),
            raw_value: Some(rate),
            details: MetricDetails::GripperChatter {
                transitions: self.transitions,
                rate,
            },
            warning: None,
        }
    }

    fn kind(&self) -> MetricKind {
        MetricKind::GripperChatter
    }

    fn score_bounds(&self) -> (f64, f64) {
        (0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::FrameContext;

    fn make_ctx<'a>(frame_index: usize, action: &'a [f64], dt: f64) -> FrameContext<'a> {
        FrameContext {
            frame_index,
            action: Some(action),
            state: None,
            timestamp: None,
            dt,
        }
    }

    #[test]
    fn alternating_gripper_low_score() {
        let dt = 1.0 / 30.0;
        let mut acc = ChatterAccumulator::with_dt(None, dt);

        // Alternating 0/1 every frame = maximum chatter.
        for i in 0..60 {
            let gripper = if i % 2 == 0 { 0.0 } else { 1.0 };
            let action = vec![0.0, 0.0, gripper]; // gripper is last dim
            acc.push_frame(&make_ctx(i, &action, dt));
        }

        let result = acc.finalize();
        assert!(result.normalized_score.is_some());
        let score = result.normalized_score.unwrap();
        // 59 transitions in 2 seconds = 29.5 trans/sec, well above max_rate 10
        assert!(
            score < 0.2,
            "alternating gripper should score low, got {score}"
        );

        if let MetricDetails::GripperChatter { transitions, rate } = result.details {
            assert_eq!(transitions, 59);
            assert!(rate > 10.0, "rate should be high, got {rate}");
        } else {
            panic!("expected GripperChatter details");
        }
    }

    #[test]
    fn constant_gripper_high_score() {
        let dt = 1.0 / 30.0;
        let mut acc = ChatterAccumulator::with_dt(None, dt);

        // Constant gripper = no chatter.
        for i in 0..60 {
            let action = vec![0.1, 0.2, 0.0]; // gripper always closed
            acc.push_frame(&make_ctx(i, &action, dt));
        }

        let result = acc.finalize();
        assert!(result.normalized_score.is_some());
        let score = result.normalized_score.unwrap();
        assert!(
            (score - 1.0).abs() < 1e-10,
            "constant gripper should score 1.0, got {score}"
        );

        if let MetricDetails::GripperChatter { transitions, .. } = result.details {
            assert_eq!(transitions, 0);
        } else {
            panic!("expected GripperChatter details");
        }
    }

    #[test]
    fn stable_scores_higher_than_chattery() {
        let dt = 1.0 / 30.0;

        // Stable gripper.
        let mut stable = ChatterAccumulator::with_dt(None, dt);
        for i in 0..60 {
            stable.push_frame(&make_ctx(i, &[0.0, 0.0, 0.0], dt));
        }
        let stable_score = stable.finalize().normalized_score.unwrap();

        // Chattery gripper.
        let mut chattery = ChatterAccumulator::with_dt(None, dt);
        for i in 0..60 {
            let g = if i % 2 == 0 { 0.0 } else { 1.0 };
            chattery.push_frame(&make_ctx(i, &[0.0, 0.0, g], dt));
        }
        let chattery_score = chattery.finalize().normalized_score.unwrap();

        assert!(
            stable_score > chattery_score,
            "stable ({stable_score}) should score higher than chattery ({chattery_score})"
        );
    }

    #[test]
    fn no_data_returns_none() {
        let mut acc = ChatterAccumulator::new(None);
        let result = acc.finalize();
        assert!(result.normalized_score.is_none());
        assert!(result.warning.is_some());
    }

    #[test]
    fn custom_gripper_dim() {
        let dt = 1.0 / 30.0;
        let mut acc = ChatterAccumulator::with_dt(Some(1), dt);

        // Gripper at dim 1, alternating.
        for i in 0..60 {
            let g = if i % 2 == 0 { 0.0 } else { 1.0 };
            let action = vec![0.0, g, 0.0]; // gripper at index 1
            acc.push_frame(&make_ctx(i, &action, dt));
        }

        let result = acc.finalize();
        if let MetricDetails::GripperChatter { transitions, .. } = result.details {
            assert_eq!(transitions, 59);
        } else {
            panic!("expected GripperChatter details");
        }
    }
}
