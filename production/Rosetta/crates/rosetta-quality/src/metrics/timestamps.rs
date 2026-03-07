//! Timestamp regularity metric.
//!
//! Uses Welford's online algorithm to compute mean and variance of
//! inter-frame time deltas (dt). Low jitter ratios (std/mean) indicate
//! regular frame timing; gaps exceeding a threshold factor are counted.

use crate::metrics::{FrameContext, MetricAccumulator, MetricDetails, MetricKind, MetricResult};

/// Streaming timestamp regularity accumulator.
///
/// Tracks inter-frame timing statistics using Welford's online algorithm
/// for numerically stable mean and variance computation. Also counts
/// "gaps" where dt exceeds `gap_threshold_factor * expected_dt`.
///
/// Memory: O(1).
pub struct TimestampsAccumulator {
    /// Previous frame timestamp (seconds).
    prev_timestamp: Option<f64>,
    /// Welford count (number of dt observations).
    welford_count: usize,
    /// Welford running mean of dt.
    welford_mean: f64,
    /// Welford running M2 (sum of squared deviations).
    welford_m2: f64,
    /// Number of gaps (dt > gap_threshold_factor * expected_dt).
    gap_count: usize,
    /// Expected dt between frames (1.0 / fps).
    expected_dt: f64,
    /// Factor above which a dt is classified as a gap (default: 2.0).
    gap_threshold_factor: f64,
}

impl TimestampsAccumulator {
    /// Create a new timestamps accumulator.
    ///
    /// # Arguments
    /// * `expected_dt` - Expected time between frames in seconds (e.g., 1.0/30.0).
    pub fn new(expected_dt: f64) -> Self {
        Self {
            prev_timestamp: None,
            welford_count: 0,
            welford_mean: 0.0,
            welford_m2: 0.0,
            gap_count: 0,
            expected_dt,
            gap_threshold_factor: 2.0,
        }
    }
}

impl MetricAccumulator for TimestampsAccumulator {
    fn push_frame(&mut self, ctx: &FrameContext<'_>) {
        let timestamp = match ctx.timestamp {
            Some(ts) => ts,
            None => return,
        };

        if let Some(prev_ts) = self.prev_timestamp {
            let dt = timestamp - prev_ts;

            // Welford online update for mean and variance.
            self.welford_count += 1;
            let delta = dt - self.welford_mean;
            self.welford_mean += delta / self.welford_count as f64;
            let delta2 = dt - self.welford_mean;
            self.welford_m2 += delta * delta2;

            // Check for timestamp gap.
            if dt > self.expected_dt * self.gap_threshold_factor {
                self.gap_count += 1;
            }
        }

        self.prev_timestamp = Some(timestamp);
    }

    fn finalize(&mut self) -> MetricResult {
        if self.welford_count < 2 {
            return MetricResult {
                kind: MetricKind::TimestampRegularity,
                normalized_score: None,
                raw_value: None,
                details: MetricDetails::None,
                warning: Some("skipped, no timestamp data".to_string()),
            };
        }

        let variance = self.welford_m2 / (self.welford_count - 1) as f64;
        let std_dt = variance.sqrt();
        let jitter_ratio = std_dt / self.welford_mean.abs().max(1e-10);

        // Normalize: low jitter = good.
        // score = (1.0 - jitter_ratio) clamped to [0, 1].
        let score = (1.0 - jitter_ratio).clamp(0.0, 1.0);

        MetricResult {
            kind: MetricKind::TimestampRegularity,
            normalized_score: Some(score),
            raw_value: Some(jitter_ratio),
            details: MetricDetails::TimestampRegularity {
                mean_dt: self.welford_mean,
                std_dt,
                jitter_ratio,
                gap_count: self.gap_count,
            },
            warning: None,
        }
    }

    fn kind(&self) -> MetricKind {
        MetricKind::TimestampRegularity
    }

    fn score_bounds(&self) -> (f64, f64) {
        if self.welford_count >= 20 {
            // After enough samples, jitter ratio converges.
            let variance = if self.welford_count > 1 {
                self.welford_m2 / (self.welford_count - 1) as f64
            } else {
                0.0
            };
            let std_dt = variance.sqrt();
            let jitter = std_dt / self.welford_mean.abs().max(1e-10);
            let current = (1.0 - jitter).clamp(0.0, 1.0);
            // Score can only degrade slightly from here.
            (current * 0.8, 1.0_f64.min(current * 1.2))
        } else {
            (0.0, 1.0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::FrameContext;

    fn make_ctx(frame_index: usize, timestamp: f64) -> FrameContext<'static> {
        FrameContext {
            frame_index,
            action: None,
            state: None,
            timestamp: Some(timestamp),
            dt: 1.0 / 30.0,
        }
    }

    #[test]
    fn regular_timestamps_score_near_one() {
        let dt = 1.0 / 30.0;
        let mut acc = TimestampsAccumulator::new(dt);

        for i in 0..100 {
            acc.push_frame(&make_ctx(i, i as f64 * dt));
        }

        let result = acc.finalize();
        assert!(result.normalized_score.is_some());
        let score = result.normalized_score.unwrap();
        assert!(
            score > 0.99,
            "perfectly regular timestamps should score near 1.0, got {score}"
        );

        if let MetricDetails::TimestampRegularity {
            jitter_ratio,
            gap_count,
            ..
        } = result.details
        {
            assert!(
                jitter_ratio < 0.001,
                "jitter should be near 0, got {jitter_ratio}"
            );
            assert_eq!(gap_count, 0, "should have 0 gaps");
        } else {
            panic!("expected TimestampRegularity details");
        }
    }

    #[test]
    fn irregular_timestamps_score_lower() {
        let dt = 1.0 / 30.0;
        let mut acc = TimestampsAccumulator::new(dt);

        // Highly irregular: alternating short and long intervals.
        for i in 0..100 {
            let jitter = if i % 2 == 0 { 0.5 } else { 2.0 };
            acc.push_frame(&make_ctx(i, i as f64 * dt * jitter));
        }

        let result = acc.finalize();
        assert!(result.normalized_score.is_some());
        let score = result.normalized_score.unwrap();
        assert!(
            score < 0.9,
            "irregular timestamps should score lower, got {score}"
        );
    }

    #[test]
    fn regular_scores_higher_than_irregular() {
        let dt = 1.0 / 30.0;

        // Regular.
        let mut regular = TimestampsAccumulator::new(dt);
        for i in 0..100 {
            regular.push_frame(&make_ctx(i, i as f64 * dt));
        }
        let regular_score = regular.finalize().normalized_score.unwrap();

        // Irregular.
        let mut irregular = TimestampsAccumulator::new(dt);
        for i in 0..100 {
            let jitter = if i % 2 == 0 { 0.5 } else { 2.0 };
            irregular.push_frame(&make_ctx(i, i as f64 * dt * jitter));
        }
        let irregular_score = irregular.finalize().normalized_score.unwrap();

        assert!(
            regular_score > irregular_score,
            "regular ({regular_score}) should score higher than irregular ({irregular_score})"
        );
    }

    #[test]
    fn gap_detection() {
        let dt = 1.0 / 30.0;
        let mut acc = TimestampsAccumulator::new(dt);

        // Normal frames with one big gap.
        for i in 0..10 {
            acc.push_frame(&make_ctx(i, i as f64 * dt));
        }
        // Big gap: skip 5 frames worth of time.
        acc.push_frame(&make_ctx(10, 10.0 * dt + 5.0 * dt));

        let result = acc.finalize();
        if let MetricDetails::TimestampRegularity { gap_count, .. } = result.details {
            assert!(gap_count >= 1, "should detect at least 1 gap, got {gap_count}");
        } else {
            panic!("expected TimestampRegularity details");
        }
    }

    #[test]
    fn no_timestamps_returns_none() {
        let mut acc = TimestampsAccumulator::new(1.0 / 30.0);

        // Push frames without timestamps.
        let ctx = FrameContext {
            frame_index: 0,
            action: Some(&[0.0]),
            state: None,
            timestamp: None,
            dt: 1.0 / 30.0,
        };
        acc.push_frame(&ctx);

        let result = acc.finalize();
        assert!(result.normalized_score.is_none());
        assert!(result.warning.is_some());
    }
}
