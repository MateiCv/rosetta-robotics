//! Action entropy metric.
//!
//! Buffers per-dimension action values and computes histogram-based Shannon
//! entropy after all frames are pushed. Higher entropy indicates more diverse
//! action usage; constant actions yield zero entropy.
//!
//! Unlike other accumulators, this metric uses O(T*D) memory to store
//! all action values (acceptable per research -- max ~1.6MB per episode
//! for 10K frames x 20 dims).

use crate::metrics::{FrameContext, MetricAccumulator, MetricDetails, MetricKind, MetricResult};
use crate::simd;

/// Streaming (buffered) action entropy accumulator.
///
/// Stores all per-dimension action values during `push_frame()`, then
/// computes per-dimension histogram-based Shannon entropy in `finalize()`.
/// Uses Sturges' rule for bin count and SIMD-accelerated histogram binning
/// and entropy summation.
///
/// Memory: O(T * D) where T = frames and D = action dims.
pub struct EntropyAccumulator {
    /// Buffered per-dimension action values: `values[dim][frame]`.
    values: Vec<Vec<f64>>,
    /// Number of action dimensions (set on first frame).
    dims: usize,
    /// Total frames ingested.
    frame_count: usize,
}

impl Default for EntropyAccumulator {
    fn default() -> Self {
        Self::new()
    }
}

impl EntropyAccumulator {
    /// Create a new entropy accumulator.
    pub fn new() -> Self {
        Self {
            values: Vec::new(),
            dims: 0,
            frame_count: 0,
        }
    }
}

impl MetricAccumulator for EntropyAccumulator {
    fn push_frame(&mut self, ctx: &FrameContext<'_>) {
        let action = match ctx.action {
            Some(a) if !a.is_empty() => a,
            _ => return,
        };

        // Initialize dimensions on first frame.
        if self.dims == 0 {
            self.dims = action.len();
            self.values = vec![Vec::new(); self.dims];
        }

        let d = self.dims.min(action.len());
        for (buf, &val) in self.values.iter_mut().zip(action.iter()).take(d) {
            buf.push(val);
        }
        self.frame_count += 1;
    }

    fn finalize(&mut self) -> MetricResult {
        if self.frame_count < 2 || self.dims == 0 {
            return MetricResult {
                kind: MetricKind::ActionEntropy,
                normalized_score: None,
                raw_value: None,
                details: MetricDetails::None,
                warning: Some("insufficient action data for entropy computation".to_string()),
            };
        }

        // Sturges' rule for number of bins: ceil(log2(n)) + 1, minimum 2.
        let n_bins = ((self.frame_count as f64).log2().ceil() as usize + 1).max(2);
        let max_entropy = (n_bins as f64).log2();

        let mut total_normalized_entropy = 0.0;
        let mut valid_dims = 0;

        for d in 0..self.dims {
            let vals = &self.values[d];
            if vals.is_empty() {
                continue;
            }

            // Compute range.
            let mut min_val = f64::MAX;
            let mut max_val = f64::MIN;
            for &v in vals {
                if v < min_val {
                    min_val = v;
                }
                if v > max_val {
                    max_val = v;
                }
            }

            let range = max_val - min_val;
            if range < 1e-10 {
                // Constant-valued dimension: entropy = 0.
                valid_dims += 1;
                continue;
            }

            // Compute histogram using SIMD kernel.
            let bin_width = range / n_bins as f64;
            let counts = simd::histogram_bin_assign(vals, min_val, bin_width, n_bins);

            // Compute probabilities.
            let total = vals.len() as f64;
            let probabilities: Vec<f64> = counts.iter().map(|&c| c as f64 / total).collect();

            // Compute entropy using SIMD kernel.
            let dim_entropy = simd::entropy_sum(&probabilities);

            // Normalize dimension entropy to [0, 1].
            let normalized = if max_entropy > 0.0 {
                (dim_entropy / max_entropy).clamp(0.0, 1.0)
            } else {
                0.0
            };

            total_normalized_entropy += normalized;
            valid_dims += 1;
        }

        if valid_dims == 0 {
            return MetricResult {
                kind: MetricKind::ActionEntropy,
                normalized_score: None,
                raw_value: None,
                details: MetricDetails::None,
                warning: Some("no valid dimensions for entropy computation".to_string()),
            };
        }

        let mean_entropy = total_normalized_entropy / valid_dims as f64;

        MetricResult {
            kind: MetricKind::ActionEntropy,
            normalized_score: Some(mean_entropy),
            raw_value: Some(mean_entropy),
            details: MetricDetails::ActionEntropy { mean_entropy },
            warning: None,
        }
    }

    fn kind(&self) -> MetricKind {
        MetricKind::ActionEntropy
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
    fn uniform_actions_high_entropy() {
        let mut acc = EntropyAccumulator::new();

        // Spread values evenly across a range to maximize entropy.
        for i in 0..200 {
            let t = i as f64 / 200.0;
            let action = vec![t, 1.0 - t, t * 2.0 - 1.0];
            acc.push_frame(&make_ctx(i, &action));
        }

        let result = acc.finalize();
        assert!(result.normalized_score.is_some());
        let score = result.normalized_score.unwrap();
        assert!(
            score > 0.7,
            "uniform-like distribution should have high entropy, got {score}"
        );
    }

    #[test]
    fn constant_actions_zero_entropy() {
        let mut acc = EntropyAccumulator::new();

        // Same value every frame -> zero entropy.
        for i in 0..100 {
            acc.push_frame(&make_ctx(i, &[0.5, 0.5, 0.5]));
        }

        let result = acc.finalize();
        assert!(result.normalized_score.is_some());
        let score = result.normalized_score.unwrap();
        assert!(
            score < 0.01,
            "constant actions should have near-zero entropy, got {score}"
        );
    }

    #[test]
    fn diverse_scores_higher_than_constant() {
        // Diverse actions.
        let mut diverse = EntropyAccumulator::new();
        for i in 0..200 {
            let t = i as f64 / 200.0;
            diverse.push_frame(&make_ctx(i, &[t, 1.0 - t]));
        }
        let diverse_score = diverse.finalize().normalized_score.unwrap();

        // Constant actions.
        let mut constant = EntropyAccumulator::new();
        for i in 0..200 {
            constant.push_frame(&make_ctx(i, &[0.5, 0.5]));
        }
        let constant_score = constant.finalize().normalized_score.unwrap();

        assert!(
            diverse_score > constant_score,
            "diverse ({diverse_score}) should score higher than constant ({constant_score})"
        );
    }

    #[test]
    fn insufficient_data_returns_none() {
        let mut acc = EntropyAccumulator::new();
        acc.push_frame(&make_ctx(0, &[0.5]));

        let result = acc.finalize();
        assert!(result.normalized_score.is_none());
        assert!(result.warning.is_some());
    }

    #[test]
    fn no_data_returns_none() {
        let mut acc = EntropyAccumulator::new();
        let result = acc.finalize();
        assert!(result.normalized_score.is_none());
    }
}
