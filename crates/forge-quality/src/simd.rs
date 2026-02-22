//! SIMD-accelerated kernel functions for quality metric computation.
//!
//! Uses [`pulp`] for runtime CPU feature detection and automatic dispatch
//! to the best available SIMD instruction set (AVX2, SSE4.1, NEON, or scalar).
//!
//! # Kernels
//!
//! - [`jerk_magnitude_sq`]: Sum of squares (jerk vector magnitude squared).
//! - [`histogram_bin_assign`]: Bin assignment for action value histograms.
//! - [`entropy_sum`]: Shannon entropy from a probability distribution.
//! - [`dot_product`]: General vectorized dot product.

use pulp::Arch;

/// Compute the squared magnitude of a jerk vector: `sum(jerk[i]^2)`.
///
/// Used by the smoothness metric to compute LDLJ (log dimensionless jerk).
/// Returns 0.0 for empty input.
pub fn jerk_magnitude_sq(jerk: &[f64]) -> f64 {
    if jerk.is_empty() {
        return 0.0;
    }

    let arch = Arch::new();
    arch.dispatch(|| {
        let mut sum = 0.0_f64;
        for &v in jerk {
            sum += v * v;
        }
        sum
    })
}

/// Assign values to histogram bins and return bin counts.
///
/// For each value, computes `floor((value - min) / bin_width)` clamped to
/// `[0, n_bins - 1]`. Returns a count vector of length `n_bins`.
///
/// Used by the action entropy metric to build per-dimension histograms.
///
/// # Panics
///
/// Panics if `n_bins` is 0 or `bin_width` is not positive.
pub fn histogram_bin_assign(values: &[f64], min: f64, bin_width: f64, n_bins: usize) -> Vec<u32> {
    assert!(n_bins > 0, "n_bins must be > 0");
    assert!(bin_width > 0.0, "bin_width must be positive");

    let mut counts = vec![0u32; n_bins];
    if values.is_empty() {
        return counts;
    }

    let max_bin = n_bins - 1;
    let arch = Arch::new();
    arch.dispatch(|| {
        for &v in values {
            let bin = ((v - min) / bin_width).floor() as isize;
            let bin = bin.clamp(0, max_bin as isize) as usize;
            counts[bin] += 1;
        }
    });

    counts
}

/// Compute Shannon entropy: `H = -sum(p * log2(p))` for p > 0.
///
/// Probabilities that are zero or negative are skipped.
/// Returns 0.0 for empty input or when all probabilities are zero.
///
/// Used by the action entropy metric to measure action diversity.
pub fn entropy_sum(probabilities: &[f64]) -> f64 {
    if probabilities.is_empty() {
        return 0.0;
    }

    let arch = Arch::new();
    arch.dispatch(|| {
        let mut entropy = 0.0_f64;
        for &p in probabilities {
            if p > 0.0 {
                entropy -= p * p.log2();
            }
        }
        entropy
    })
}

/// Compute the dot product of two slices: `sum(a[i] * b[i])`.
///
/// Uses the minimum length of the two slices.
/// Returns 0.0 for empty input.
///
/// Used by the smoothness metric and other linear algebra operations.
pub fn dot_product(a: &[f64], b: &[f64]) -> f64 {
    let len = a.len().min(b.len());
    if len == 0 {
        return 0.0;
    }

    let a = &a[..len];
    let b = &b[..len];

    let arch = Arch::new();
    arch.dispatch(|| {
        let mut sum = 0.0_f64;
        for i in 0..len {
            sum += a[i] * b[i];
        }
        sum
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- jerk_magnitude_sq ---

    #[test]
    fn jerk_3_4_returns_25() {
        let result = jerk_magnitude_sq(&[3.0, 4.0]);
        assert!((result - 25.0).abs() < 1e-10, "expected 25.0, got {result}");
    }

    #[test]
    fn jerk_empty_returns_zero() {
        assert_eq!(jerk_magnitude_sq(&[]), 0.0);
    }

    #[test]
    fn jerk_single_element() {
        let result = jerk_magnitude_sq(&[5.0]);
        assert!((result - 25.0).abs() < 1e-10);
    }

    #[test]
    fn jerk_all_zeros() {
        let result = jerk_magnitude_sq(&[0.0, 0.0, 0.0]);
        assert_eq!(result, 0.0);
    }

    #[test]
    fn jerk_negative_values() {
        // (-3)^2 + (-4)^2 = 9 + 16 = 25
        let result = jerk_magnitude_sq(&[-3.0, -4.0]);
        assert!((result - 25.0).abs() < 1e-10);
    }

    // --- histogram_bin_assign ---

    #[test]
    fn histogram_uniform_bins() {
        // Values 0.0, 1.0, 2.0, 3.0 into 4 bins: [0,1), [1,2), [2,3), [3,4)
        let counts = histogram_bin_assign(&[0.0, 1.0, 2.0, 3.0], 0.0, 1.0, 4);
        assert_eq!(counts, vec![1, 1, 1, 1]);
    }

    #[test]
    fn histogram_clamping() {
        // Values below min clamp to bin 0, values above max clamp to last bin.
        // -1.0 => floor((-1.0 - 0.0)/1.0) = -1, clamped to 0 => bin 0
        // 0.5 => floor(0.5/1.0) = 0 => bin 0
        // 1.5 => floor(1.5/1.0) = 1 => bin 1
        // 10.0 => floor(10.0/1.0) = 10, clamped to 2 => bin 2
        let counts = histogram_bin_assign(&[-1.0, 0.5, 1.5, 10.0], 0.0, 1.0, 3);
        assert_eq!(counts, vec![2, 1, 1]);
    }

    #[test]
    fn histogram_empty_values() {
        let counts = histogram_bin_assign(&[], 0.0, 1.0, 5);
        assert_eq!(counts, vec![0, 0, 0, 0, 0]);
    }

    #[test]
    fn histogram_single_value() {
        let counts = histogram_bin_assign(&[2.5], 0.0, 1.0, 5);
        assert_eq!(counts, vec![0, 0, 1, 0, 0]);
    }

    // --- entropy_sum ---

    #[test]
    fn entropy_uniform_distribution() {
        // Uniform over 4 outcomes: H = log2(4) = 2.0
        let result = entropy_sum(&[0.25, 0.25, 0.25, 0.25]);
        assert!(
            (result - 2.0).abs() < 1e-10,
            "expected 2.0 for uniform(4), got {result}"
        );
    }

    #[test]
    fn entropy_certain_outcome() {
        // All probability on one outcome: H = 0.0
        let result = entropy_sum(&[1.0, 0.0, 0.0, 0.0]);
        assert!(
            (result - 0.0).abs() < 1e-10,
            "expected 0.0 for certain outcome, got {result}"
        );
    }

    #[test]
    fn entropy_binary() {
        // Fair coin: H = 1.0
        let result = entropy_sum(&[0.5, 0.5]);
        assert!(
            (result - 1.0).abs() < 1e-10,
            "expected 1.0 for fair coin, got {result}"
        );
    }

    #[test]
    fn entropy_empty_returns_zero() {
        assert_eq!(entropy_sum(&[]), 0.0);
    }

    #[test]
    fn entropy_all_zeros() {
        assert_eq!(entropy_sum(&[0.0, 0.0, 0.0]), 0.0);
    }

    // --- dot_product ---

    #[test]
    fn dot_product_basic() {
        // 1*4 + 2*5 + 3*6 = 4 + 10 + 18 = 32
        let result = dot_product(&[1.0, 2.0, 3.0], &[4.0, 5.0, 6.0]);
        assert!(
            (result - 32.0).abs() < 1e-10,
            "expected 32.0, got {result}"
        );
    }

    #[test]
    fn dot_product_empty() {
        assert_eq!(dot_product(&[], &[]), 0.0);
    }

    #[test]
    fn dot_product_single() {
        let result = dot_product(&[3.0], &[4.0]);
        assert!((result - 12.0).abs() < 1e-10);
    }

    #[test]
    fn dot_product_mismatched_lengths() {
        // Uses minimum length
        let result = dot_product(&[1.0, 2.0, 3.0], &[4.0, 5.0]);
        assert!(
            (result - 14.0).abs() < 1e-10,
            "expected 14.0 (1*4 + 2*5), got {result}"
        );
    }

    #[test]
    fn dot_product_orthogonal() {
        // Orthogonal vectors: dot product = 0
        let result = dot_product(&[1.0, 0.0], &[0.0, 1.0]);
        assert!(
            result.abs() < 1e-10,
            "expected 0.0 for orthogonal, got {result}"
        );
    }
}
