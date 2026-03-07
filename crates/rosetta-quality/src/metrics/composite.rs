//! Composite quality score computation.
//!
//! Combines individual metric scores into a single weighted average,
//! with automatic weight redistribution when some metrics return `None`.

use std::collections::HashMap;

use crate::metrics::{MetricAccumulator, MetricKind};

/// Compute a composite quality score as a weighted average of individual scores.
///
/// Metrics that return `None` are skipped, and their weight is automatically
/// redistributed to the remaining metrics (by dividing by the sum of active
/// weights rather than the sum of all weights).
///
/// Returns `None` if all scores are `None` or all active weights are zero.
///
/// # Score Range
///
/// The composite score stays in `[0.0, 1.0]` when all individual scores are
/// in that range (which they should be -- all accumulators normalize to 0-1).
pub fn composite_score(
    scores: &[(MetricKind, Option<f64>)],
    weights: &HashMap<MetricKind, f64>,
) -> Option<f64> {
    let mut weighted_sum = 0.0;
    let mut total_weight = 0.0;

    for (kind, score) in scores {
        if let Some(s) = score {
            let w = weights.get(kind).copied().unwrap_or(0.0);
            weighted_sum += s * w;
            total_weight += w;
        }
    }

    if total_weight > 0.0 {
        Some(weighted_sum / total_weight)
    } else {
        None
    }
}

/// Compute optimistic and pessimistic bounds on the composite score.
///
/// For each accumulator, queries `score_bounds()` to get the `(min, max)`
/// range of possible final scores given frames seen so far. Returns
/// `(min_composite, max_composite)` as weighted averages of these bounds.
///
/// Used by early exit logic to determine if the composite score is
/// guaranteed to pass or fail a threshold.
pub fn composite_bounds(
    accumulators: &[Box<dyn MetricAccumulator>],
    weights: &HashMap<MetricKind, f64>,
) -> (f64, f64) {
    let mut min_weighted = 0.0;
    let mut max_weighted = 0.0;
    let mut total_weight = 0.0;

    for acc in accumulators {
        let kind = acc.kind();
        let w = weights.get(&kind).copied().unwrap_or(0.0);
        let (lo, hi) = acc.score_bounds();
        min_weighted += lo * w;
        max_weighted += hi * w;
        total_weight += w;
    }

    if total_weight > 0.0 {
        (min_weighted / total_weight, max_weighted / total_weight)
    } else {
        (0.0, 0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_scores_present_weighted_average() {
        let scores = vec![
            (MetricKind::Smoothness, Some(0.8)),
            (MetricKind::DeadActions, Some(0.6)),
            (MetricKind::ActionEntropy, Some(0.4)),
        ];
        let mut weights = HashMap::new();
        weights.insert(MetricKind::Smoothness, 0.5);
        weights.insert(MetricKind::DeadActions, 0.3);
        weights.insert(MetricKind::ActionEntropy, 0.2);

        let result = composite_score(&scores, &weights);
        // (0.8*0.5 + 0.6*0.3 + 0.4*0.2) / (0.5 + 0.3 + 0.2)
        // = (0.4 + 0.18 + 0.08) / 1.0 = 0.66
        let expected = (0.8 * 0.5 + 0.6 * 0.3 + 0.4 * 0.2) / (0.5 + 0.3 + 0.2);
        assert!(result.is_some());
        assert!(
            (result.unwrap() - expected).abs() < 1e-10,
            "expected {expected}, got {}",
            result.unwrap()
        );
    }

    #[test]
    fn some_none_scores_weight_redistributed() {
        let scores = vec![
            (MetricKind::Smoothness, Some(0.8)),
            (MetricKind::DeadActions, None), // skipped
            (MetricKind::ActionEntropy, Some(0.4)),
        ];
        let mut weights = HashMap::new();
        weights.insert(MetricKind::Smoothness, 0.5);
        weights.insert(MetricKind::DeadActions, 0.3);
        weights.insert(MetricKind::ActionEntropy, 0.2);

        let result = composite_score(&scores, &weights);
        // Only Smoothness(0.5) and ActionEntropy(0.2) are active.
        // (0.8*0.5 + 0.4*0.2) / (0.5 + 0.2) = (0.4 + 0.08) / 0.7 = 0.48/0.7
        let expected = (0.8 * 0.5 + 0.4 * 0.2) / (0.5 + 0.2);
        assert!(result.is_some());
        let actual = result.unwrap();
        assert!(
            (actual - expected).abs() < 1e-10,
            "expected {expected}, got {actual}"
        );
        assert!(actual >= 0.0 && actual <= 1.0, "result out of 0-1 range");
    }

    #[test]
    fn all_none_returns_none() {
        let scores = vec![
            (MetricKind::Smoothness, None),
            (MetricKind::DeadActions, None),
        ];
        let mut weights = HashMap::new();
        weights.insert(MetricKind::Smoothness, 0.5);
        weights.insert(MetricKind::DeadActions, 0.5);

        assert!(composite_score(&scores, &weights).is_none());
    }

    #[test]
    fn all_scores_one_gives_one() {
        let scores = vec![
            (MetricKind::Smoothness, Some(1.0)),
            (MetricKind::DeadActions, Some(1.0)),
            (MetricKind::ActionEntropy, Some(1.0)),
        ];
        let mut weights = HashMap::new();
        weights.insert(MetricKind::Smoothness, 0.5);
        weights.insert(MetricKind::DeadActions, 0.3);
        weights.insert(MetricKind::ActionEntropy, 0.2);

        let result = composite_score(&scores, &weights).unwrap();
        assert!(
            (result - 1.0).abs() < 1e-10,
            "all 1.0 scores should give 1.0 composite, got {result}"
        );
    }

    #[test]
    fn all_scores_zero_gives_zero() {
        let scores = vec![
            (MetricKind::Smoothness, Some(0.0)),
            (MetricKind::DeadActions, Some(0.0)),
            (MetricKind::ActionEntropy, Some(0.0)),
        ];
        let mut weights = HashMap::new();
        weights.insert(MetricKind::Smoothness, 0.5);
        weights.insert(MetricKind::DeadActions, 0.3);
        weights.insert(MetricKind::ActionEntropy, 0.2);

        let result = composite_score(&scores, &weights).unwrap();
        assert!(
            result.abs() < 1e-10,
            "all 0.0 scores should give 0.0 composite, got {result}"
        );
    }

    #[test]
    fn missing_weight_defaults_to_zero() {
        let scores = vec![
            (MetricKind::Smoothness, Some(0.8)),
            (MetricKind::DeadActions, Some(0.6)),
        ];
        // Only Smoothness has a weight; DeadActions is missing from map.
        let mut weights = HashMap::new();
        weights.insert(MetricKind::Smoothness, 1.0);

        let result = composite_score(&scores, &weights).unwrap();
        // Only Smoothness contributes: 0.8 * 1.0 / 1.0 = 0.8
        assert!(
            (result - 0.8).abs() < 1e-10,
            "missing weight should default to 0.0, got {result}"
        );
    }
}
