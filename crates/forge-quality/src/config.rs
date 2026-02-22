//! Quality analysis configuration.
//!
//! [`QualityConfig`] controls which metrics are computed, their weights in the
//! composite score, and optional overrides for FPS and gripper dimension.
//! [`FilterConfig`] specifies episode filtering criteria.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use forge_core::ForgeResult;

use crate::metrics::MetricKind;

/// Configuration for quality analysis.
///
/// Controls per-metric weights, skip list, composite threshold, and
/// optional overrides for FPS and gripper dimension detection.
///
/// # Default Weights
///
/// The default weights are tuned for robotics datasets, emphasizing
/// trajectory smoothness and action liveness:
///
/// | Metric              | Weight |
/// |---------------------|--------|
/// | Smoothness          | 0.25   |
/// | Dead Actions        | 0.20   |
/// | Static Detection    | 0.15   |
/// | Gripper Chatter     | 0.10   |
/// | Timestamp Regularity| 0.10   |
/// | Action Saturation   | 0.10   |
/// | Action Entropy      | 0.10   |
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityConfig {
    /// Per-metric weights for the composite score. Must sum to 1.0
    /// (before redistribution from skipped metrics).
    pub weights: HashMap<MetricKind, f64>,

    /// Metrics to skip during analysis (default: empty).
    #[serde(default)]
    pub skip_metrics: Vec<MetricKind>,

    /// Composite score threshold for early exit. If set, analysis may
    /// stop early once the composite score is guaranteed to fall below
    /// this threshold.
    #[serde(default)]
    pub composite_threshold: Option<f64>,

    /// Override for which dimension index is the gripper (default: last dim).
    #[serde(default)]
    pub gripper_dim: Option<usize>,

    /// Override for dataset FPS. If `None`, derived from timestamps or
    /// dataset metadata.
    #[serde(default)]
    pub fps: Option<f64>,
}

impl Default for QualityConfig {
    fn default() -> Self {
        let mut weights = HashMap::new();
        weights.insert(MetricKind::Smoothness, 0.25);
        weights.insert(MetricKind::DeadActions, 0.20);
        weights.insert(MetricKind::StaticDetection, 0.15);
        weights.insert(MetricKind::GripperChatter, 0.10);
        weights.insert(MetricKind::TimestampRegularity, 0.10);
        weights.insert(MetricKind::ActionSaturation, 0.10);
        weights.insert(MetricKind::ActionEntropy, 0.10);

        Self {
            weights,
            skip_metrics: Vec::new(),
            composite_threshold: None,
            gripper_dim: None,
            fps: None,
        }
    }
}

impl QualityConfig {
    /// Load configuration from a YAML file.
    pub fn from_yaml(path: &Path) -> ForgeResult<Self> {
        let contents = std::fs::read_to_string(path)?;
        serde_yaml_ng::from_str(&contents).map_err(|e| {
            forge_core::ForgeError::Config(format!("invalid quality config YAML: {e}"))
        })
    }

    /// Returns weights excluding skipped metrics, with remaining weights
    /// redistributed proportionally so they sum to 1.0.
    ///
    /// If all metrics are skipped, returns an empty map.
    pub fn effective_weights(&self) -> HashMap<MetricKind, f64> {
        let active: HashMap<MetricKind, f64> = self
            .weights
            .iter()
            .filter(|(kind, _)| !self.skip_metrics.contains(kind))
            .map(|(&kind, &weight)| (kind, weight))
            .collect();

        let total: f64 = active.values().sum();
        if total == 0.0 {
            return active;
        }

        active
            .into_iter()
            .map(|(kind, weight)| (kind, weight / total))
            .collect()
    }
}

/// Configuration for episode filtering.
///
/// Specifies which episodes to keep or remove based on quality scores,
/// with optional manual overrides and dry-run mode.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FilterConfig {
    /// Minimum composite score to keep an episode.
    #[serde(default)]
    pub min_quality: Option<f64>,

    /// Per-metric minimum scores. An episode must meet ALL thresholds to pass.
    #[serde(default)]
    pub per_metric_thresholds: HashMap<MetricKind, f64>,

    /// Episode indices to always keep regardless of quality score.
    #[serde(default)]
    pub keep_episodes: Vec<usize>,

    /// Episode indices to always remove regardless of quality score.
    #[serde(default)]
    pub remove_episodes: Vec<usize>,

    /// If `true`, compute filtering decisions but do not write output.
    #[serde(default)]
    pub dry_run: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_weights_sum_to_one() {
        let config = QualityConfig::default();
        let sum: f64 = config.weights.values().sum();
        assert!(
            (sum - 1.0).abs() < 1e-10,
            "default weights should sum to 1.0, got {sum}"
        );
    }

    #[test]
    fn default_weights_have_all_metrics() {
        let config = QualityConfig::default();
        for kind in MetricKind::all() {
            assert!(
                config.weights.contains_key(kind),
                "missing default weight for {kind}"
            );
        }
    }

    #[test]
    fn effective_weights_excludes_skipped() {
        let mut config = QualityConfig::default();
        config.skip_metrics.push(MetricKind::GripperChatter);

        let effective = config.effective_weights();
        assert!(!effective.contains_key(&MetricKind::GripperChatter));
        assert_eq!(effective.len(), 6);

        let sum: f64 = effective.values().sum();
        assert!(
            (sum - 1.0).abs() < 1e-10,
            "effective weights should redistribute to sum to 1.0, got {sum}"
        );
    }

    #[test]
    fn effective_weights_empty_when_all_skipped() {
        let mut config = QualityConfig::default();
        for kind in MetricKind::all() {
            config.skip_metrics.push(*kind);
        }
        let effective = config.effective_weights();
        assert!(effective.is_empty());
    }

    #[test]
    fn from_yaml_loads_config() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("quality.yaml");
        let yaml = r#"
weights:
  Smoothness: 0.5
  DeadActions: 0.5
skip_metrics: []
composite_threshold: 0.7
gripper_dim: 6
fps: 30.0
"#;
        std::fs::write(&path, yaml).unwrap();
        let config = QualityConfig::from_yaml(&path).unwrap();
        assert_eq!(config.weights.len(), 2);
        assert_eq!(config.composite_threshold, Some(0.7));
        assert_eq!(config.gripper_dim, Some(6));
        assert_eq!(config.fps, Some(30.0));
    }
}
