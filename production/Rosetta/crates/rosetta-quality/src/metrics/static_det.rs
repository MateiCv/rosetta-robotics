//! Static episode detection metric.
//!
//! Detects episodes where the robot state shows no meaningful change between
//! consecutive frames. High static ratios indicate recording artifacts,
//! paused robots, or failed controllers.

use crate::metrics::{FrameContext, MetricAccumulator, MetricDetails, MetricKind, MetricResult};

/// Streaming static detection accumulator.
///
/// Computes the L2 norm of state difference between consecutive frames.
/// Frames with norm below `motion_threshold` are classified as static.
///
/// Prefers state vectors for detection; falls back to action vectors if
/// no state data is available.
///
/// Memory: O(D) where D is the state/action dimensionality.
pub struct StaticDetAccumulator {
    /// Previous frame's state/action vector.
    prev_state: Option<Vec<f64>>,
    /// Number of frames classified as static.
    static_count: usize,
    /// Total frames compared (one less than total frames pushed).
    total_frames: usize,
    /// L2 norm threshold below which a frame is considered static.
    motion_threshold: f64,
}

impl Default for StaticDetAccumulator {
    fn default() -> Self {
        Self::new()
    }
}

impl StaticDetAccumulator {
    /// Create a new static detection accumulator with default threshold (1e-4).
    pub fn new() -> Self {
        Self {
            prev_state: None,
            static_count: 0,
            total_frames: 0,
            motion_threshold: 1e-4,
        }
    }
}

impl MetricAccumulator for StaticDetAccumulator {
    fn push_frame(&mut self, ctx: &FrameContext<'_>) {
        // Prefer state, fall back to action.
        let values: &[f64] = match (ctx.state, ctx.action) {
            (Some(s), _) if !s.is_empty() => s,
            (_, Some(a)) if !a.is_empty() => a,
            _ => return,
        };

        if let Some(ref prev) = self.prev_state {
            let d = prev.len().min(values.len());
            let mut norm_sq = 0.0;
            for i in 0..d {
                let diff = values[i] - prev[i];
                norm_sq += diff * diff;
            }
            let norm = norm_sq.sqrt();

            if norm < self.motion_threshold {
                self.static_count += 1;
            }
            self.total_frames += 1;
        }

        self.prev_state = Some(values.to_vec());
    }

    fn finalize(&mut self) -> MetricResult {
        if self.total_frames == 0 {
            return MetricResult {
                kind: MetricKind::StaticDetection,
                normalized_score: None,
                raw_value: None,
                details: MetricDetails::None,
                warning: Some("insufficient data (need at least 2 frames)".to_string()),
            };
        }

        let static_ratio = self.static_count as f64 / self.total_frames as f64;
        // Score: 1.0 = always moving (best), 0.0 = completely static (worst).
        let score = 1.0 - static_ratio;

        MetricResult {
            kind: MetricKind::StaticDetection,
            normalized_score: Some(score),
            raw_value: Some(static_ratio),
            details: MetricDetails::StaticDetection { static_ratio },
            warning: None,
        }
    }

    fn kind(&self) -> MetricKind {
        MetricKind::StaticDetection
    }

    fn score_bounds(&self) -> (f64, f64) {
        (0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::FrameContext;

    fn make_ctx_state<'a>(
        frame_index: usize,
        state: &'a [f64],
    ) -> FrameContext<'a> {
        FrameContext {
            frame_index,
            action: None,
            state: Some(state),
            timestamp: None,
            dt: 1.0 / 30.0,
        }
    }

    fn make_ctx_action<'a>(
        frame_index: usize,
        action: &'a [f64],
    ) -> FrameContext<'a> {
        FrameContext {
            frame_index,
            action: Some(action),
            state: None,
            timestamp: None,
            dt: 1.0 / 30.0,
        }
    }

    #[test]
    fn identical_states_score_near_zero() {
        let mut acc = StaticDetAccumulator::new();

        for i in 0..50 {
            let state = vec![1.0, 2.0, 3.0]; // identical every frame
            acc.push_frame(&make_ctx_state(i, &state));
        }

        let result = acc.finalize();
        assert!(result.normalized_score.is_some());
        let score = result.normalized_score.unwrap();
        assert!(
            score < 0.01,
            "static episode should score near 0.0, got {score}"
        );

        if let MetricDetails::StaticDetection { static_ratio } = result.details {
            assert!(
                (static_ratio - 1.0).abs() < 1e-10,
                "static_ratio should be 1.0, got {static_ratio}"
            );
        } else {
            panic!("expected StaticDetection details");
        }
    }

    #[test]
    fn varying_states_score_near_one() {
        let mut acc = StaticDetAccumulator::new();

        for i in 0..50 {
            let t = i as f64 * 0.1;
            let state = vec![t.sin(), t.cos(), t * 0.05];
            acc.push_frame(&make_ctx_state(i, &state));
        }

        let result = acc.finalize();
        assert!(result.normalized_score.is_some());
        let score = result.normalized_score.unwrap();
        assert!(
            score > 0.9,
            "moving episode should score near 1.0, got {score}"
        );
    }

    #[test]
    fn moving_scores_higher_than_static() {
        // Moving episode.
        let mut moving = StaticDetAccumulator::new();
        for i in 0..50 {
            let t = i as f64 * 0.1;
            moving.push_frame(&make_ctx_state(i, &[t.sin(), t.cos()]));
        }
        let moving_score = moving.finalize().normalized_score.unwrap();

        // Static episode.
        let mut static_ep = StaticDetAccumulator::new();
        for i in 0..50 {
            static_ep.push_frame(&make_ctx_state(i, &[1.0, 2.0]));
        }
        let static_score = static_ep.finalize().normalized_score.unwrap();

        assert!(
            moving_score > static_score,
            "moving ({moving_score}) should score higher than static ({static_score})"
        );
    }

    #[test]
    fn falls_back_to_action_when_no_state() {
        let mut acc = StaticDetAccumulator::new();

        for i in 0..50 {
            let t = i as f64 * 0.1;
            acc.push_frame(&make_ctx_action(i, &[t.sin(), t.cos()]));
        }

        let result = acc.finalize();
        assert!(result.normalized_score.is_some());
        let score = result.normalized_score.unwrap();
        assert!(score > 0.9, "should work with action fallback, got {score}");
    }

    #[test]
    fn single_frame_returns_none() {
        let mut acc = StaticDetAccumulator::new();
        acc.push_frame(&make_ctx_state(0, &[1.0, 2.0]));

        let result = acc.finalize();
        assert!(result.normalized_score.is_none());
        assert!(result.warning.is_some());
    }
}
