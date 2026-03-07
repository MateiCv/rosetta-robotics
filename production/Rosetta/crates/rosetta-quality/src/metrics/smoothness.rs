//! LDLJ (Log Dimensionless Jerk) smoothness metric.
//!
//! Computes trajectory smoothness using third-order finite differences
//! over a sliding window of 4 position samples, with Kahan summation
//! for numerical stability. Higher LDLJ values (less negative) indicate
//! smoother trajectories.

use crate::metrics::{FrameContext, MetricAccumulator, MetricDetails, MetricKind, MetricResult};
use crate::simd;

/// Streaming LDLJ smoothness accumulator.
///
/// Maintains a ring buffer of 4 position values per dimension and computes
/// third-order finite differences (jerk) at each step. Uses Kahan summation
/// to accumulate ||jerk||^2 * dt without floating-point drift.
///
/// Memory: O(D) where D is the action dimensionality (4 samples per dim).
pub struct SmoothnessAccumulator {
    /// Ring buffer of action values: `window[dim][slot]`.
    window: Vec<[f64; 4]>,
    /// Current write position in the ring buffer (0..4).
    window_pos: usize,
    /// Number of frames inserted so far (saturates at 4).
    window_filled: usize,
    /// Running Kahan-compensated sum of ||jerk||^2 * dt.
    jerk_sum_sq: f64,
    /// Kahan compensation term.
    kahan_comp: f64,
    /// Running maximum of ||velocity||^2 (for peak velocity tracking).
    peak_vel_sq: f64,
    /// Total frames ingested.
    frame_count: usize,
    /// Time delta between frames (seconds).
    dt: f64,
    /// Number of action dimensions (set on first frame).
    dims: usize,
}

impl SmoothnessAccumulator {
    /// Create a new smoothness accumulator.
    ///
    /// # Arguments
    /// * `dt` - Time delta between frames in seconds (e.g., 1.0/30.0 for 30 fps).
    pub fn new(dt: f64) -> Self {
        Self {
            window: Vec::new(),
            window_pos: 0,
            window_filled: 0,
            jerk_sum_sq: 0.0,
            kahan_comp: 0.0,
            peak_vel_sq: 0.0,
            frame_count: 0,
            dt,
            dims: 0,
        }
    }
}

impl MetricAccumulator for SmoothnessAccumulator {
    fn push_frame(&mut self, ctx: &FrameContext<'_>) {
        let action = match ctx.action {
            Some(a) if !a.is_empty() => a,
            _ => return,
        };

        // Initialize dimensions on first frame with action data.
        if self.dims == 0 {
            self.dims = action.len();
            self.window = vec![[0.0; 4]; self.dims];
        }

        let d = self.dims.min(action.len());

        // Insert action values into ring buffer.
        for (win, &val) in self.window.iter_mut().zip(action.iter()).take(d) {
            win[self.window_pos] = val;
        }

        self.frame_count += 1;
        self.window_filled = self.window_filled.saturating_add(1).min(4);

        // Need at least 4 samples to compute third-order finite difference (jerk).
        if self.window_filled >= 4 {
            // Retrieve samples in chronological order from ring buffer.
            // window_pos just received the newest sample.
            let newest = self.window_pos;
            let p2 = if newest >= 1 { newest - 1 } else { 3 };
            let p1 = if newest >= 2 { newest - 2 } else { newest + 4 - 2 };
            let p0 = if newest >= 3 { newest - 3 } else { newest + 4 - 3 };

            // Compute jerk via third forward finite difference:
            // jerk[d] = (p3 - 3*p2 + 3*p1 - p0) / dt^3
            let dt3 = self.dt * self.dt * self.dt;
            let mut jerk_vec = vec![0.0; d];
            for (i, jerk_val) in jerk_vec.iter_mut().enumerate().take(d) {
                let val =
                    self.window[i][newest] - 3.0 * self.window[i][p2] + 3.0 * self.window[i][p1]
                        - self.window[i][p0];
                *jerk_val = val / dt3;
            }

            let jerk_sq = simd::jerk_magnitude_sq(&jerk_vec);

            // Kahan summation: accumulate jerk_sq * dt.
            let y = jerk_sq * self.dt - self.kahan_comp;
            let t = self.jerk_sum_sq + y;
            self.kahan_comp = (t - self.jerk_sum_sq) - y;
            self.jerk_sum_sq = t;

            // Compute velocity for peak tracking using two most recent samples.
            let mut vel_sq = 0.0;
            for i in 0..d {
                let v = (self.window[i][newest] - self.window[i][p2]) / self.dt;
                vel_sq += v * v;
            }
            if vel_sq > self.peak_vel_sq {
                self.peak_vel_sq = vel_sq;
            }
        }

        // Advance ring buffer position.
        self.window_pos = (self.window_pos + 1) % 4;
    }

    fn finalize(&mut self) -> MetricResult {
        if self.frame_count < 4 {
            return MetricResult {
                kind: MetricKind::Smoothness,
                normalized_score: None,
                raw_value: None,
                details: MetricDetails::None,
                warning: Some("insufficient data (need at least 4 frames)".to_string()),
            };
        }

        if self.peak_vel_sq < 1e-10 {
            return MetricResult {
                kind: MetricKind::Smoothness,
                normalized_score: None,
                raw_value: None,
                details: MetricDetails::None,
                warning: Some("no motion detected".to_string()),
            };
        }

        let duration = self.frame_count as f64 * self.dt;
        let v_peak_sq = self.peak_vel_sq;

        // LDLJ = -ln(T^3 / V_peak^2 * integral(||jerk||^2 dt))
        let arg = duration.powi(3) / v_peak_sq * self.jerk_sum_sq;
        if arg <= 0.0 {
            return MetricResult {
                kind: MetricKind::Smoothness,
                normalized_score: None,
                raw_value: None,
                details: MetricDetails::None,
                warning: Some("invalid LDLJ computation (non-positive argument)".to_string()),
            };
        }

        let ldlj = -(arg.ln());

        // Normalize to 0-1 using sigmoid: LDLJ ranges widely depending on trajectory
        // characteristics. More negative = jerkier, less negative = smoother.
        // Sigmoid provides smooth normalization across any LDLJ range:
        //   score = 1 / (1 + exp(-ldlj / scale))
        // With scale=5.0: ldlj=0 -> 0.5, ldlj=-10 -> 0.12, ldlj=5 -> 0.73
        let scale = 5.0;
        let score = 1.0 / (1.0 + (-ldlj / scale).exp());

        MetricResult {
            kind: MetricKind::Smoothness,
            normalized_score: Some(score),
            raw_value: Some(ldlj),
            details: MetricDetails::Smoothness { ldlj_value: ldlj },
            warning: None,
        }
    }

    fn kind(&self) -> MetricKind {
        MetricKind::Smoothness
    }

    fn score_bounds(&self) -> (f64, f64) {
        (0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::FrameContext;

    fn make_ctx<'a>(
        frame_index: usize,
        action: &'a [f64],
        dt: f64,
    ) -> FrameContext<'a> {
        FrameContext {
            frame_index,
            action: Some(action),
            state: None,
            timestamp: None,
            dt,
        }
    }

    #[test]
    fn smooth_trajectory_produces_score() {
        let dt = 1.0 / 30.0;
        let mut acc = SmoothnessAccumulator::new(dt);

        // Slow smooth sinusoidal trajectory with low frequency.
        let freq = 0.5; // Hz
        for i in 0..200 {
            let t = i as f64 * dt;
            let action = vec![
                (2.0 * std::f64::consts::PI * freq * t).sin() * 0.1,
                (2.0 * std::f64::consts::PI * freq * t).cos() * 0.1,
                t * 0.001,
            ];
            let ctx = make_ctx(i, &action, dt);
            acc.push_frame(&ctx);
        }

        let result = acc.finalize();
        assert!(result.normalized_score.is_some(), "should produce a score");
        let score = result.normalized_score.unwrap();
        assert!(
            score >= 0.0 && score <= 1.0,
            "score should be in [0, 1], got {score}"
        );
        // Verify raw LDLJ value exists.
        assert!(result.raw_value.is_some(), "should produce raw LDLJ value");
    }

    #[test]
    fn jerky_trajectory_scores_lower() {
        let dt = 1.0 / 30.0;
        let mut acc = SmoothnessAccumulator::new(dt);

        // Jerky motion: random-like alternating values.
        for i in 0..100 {
            let sign = if i % 2 == 0 { 1.0 } else { -1.0 };
            let action = vec![
                sign * 0.5,
                -sign * 0.3,
                sign * 0.4,
            ];
            let ctx = make_ctx(i, &action, dt);
            acc.push_frame(&ctx);
        }

        let result = acc.finalize();
        assert!(result.normalized_score.is_some(), "should produce a score");
        let score = result.normalized_score.unwrap();
        assert!(
            score < 0.5,
            "jerky trajectory should score < 0.5, got {score}"
        );
    }

    #[test]
    fn smooth_scores_higher_than_jerky() {
        let dt = 1.0 / 30.0;

        // Smooth trajectory.
        let mut smooth_acc = SmoothnessAccumulator::new(dt);
        for i in 0..100 {
            let t = i as f64 * dt;
            let action = vec![t.sin() * 0.5, t.cos() * 0.5];
            smooth_acc.push_frame(&make_ctx(i, &action, dt));
        }
        let smooth_score = smooth_acc.finalize().normalized_score.unwrap();

        // Jerky trajectory.
        let mut jerky_acc = SmoothnessAccumulator::new(dt);
        for i in 0..100 {
            let sign = if i % 2 == 0 { 1.0 } else { -1.0 };
            let action = vec![sign * 0.5, -sign * 0.3];
            jerky_acc.push_frame(&make_ctx(i, &action, dt));
        }
        let jerky_score = jerky_acc.finalize().normalized_score.unwrap();

        assert!(
            smooth_score > jerky_score,
            "smooth ({smooth_score}) should score higher than jerky ({jerky_score})"
        );
    }

    #[test]
    fn insufficient_frames_returns_none() {
        let dt = 1.0 / 30.0;
        let mut acc = SmoothnessAccumulator::new(dt);

        for i in 0..3 {
            let action = vec![i as f64 * 0.1];
            acc.push_frame(&make_ctx(i, &action, dt));
        }

        let result = acc.finalize();
        assert!(result.normalized_score.is_none());
        assert!(result.warning.is_some());
    }

    #[test]
    fn no_motion_returns_none() {
        let dt = 1.0 / 30.0;
        let mut acc = SmoothnessAccumulator::new(dt);

        // All identical action values -- no motion.
        for i in 0..20 {
            let action = vec![0.5, 0.5, 0.5];
            acc.push_frame(&make_ctx(i, &action, dt));
        }

        let result = acc.finalize();
        assert!(result.normalized_score.is_none());
        assert!(result.warning.is_some());
    }
}
