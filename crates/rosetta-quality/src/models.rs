//! Serializable data models for quality reports and filter results.
//!
//! These types are the output of quality analysis and episode filtering.
//! All derive `Serialize`/`Deserialize` for JSON export.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Score for a single metric on a single episode.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricScore {
    /// Normalized score in `[0.0, 1.0]` (higher = better).
    /// `None` if the metric was skipped or could not be computed.
    pub normalized: Option<f64>,
    /// Raw metric value before normalization.
    pub raw: Option<f64>,
    /// Warning message if the metric was degraded or skipped
    /// (e.g., "skipped, no gripper data").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

/// Quality assessment for a single episode.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpisodeQuality {
    /// Episode identifier string.
    pub episode_id: String,
    /// Number of frames in this episode.
    pub num_frames: usize,
    /// Episode duration in seconds (if timestamps available).
    pub duration_secs: Option<f64>,
    /// Camera names present in this episode.
    pub cameras: Vec<String>,
    /// Weighted composite quality score in `[0.0, 1.0]`.
    /// `None` if no metrics could be computed.
    pub composite_score: Option<f64>,
    /// Per-metric scores keyed by metric kind name (e.g., "smoothness").
    pub metric_scores: HashMap<String, MetricScore>,
    /// Human-readable quality flags (e.g., "dead actions detected").
    #[serde(default)]
    pub flags: Vec<String>,
}

/// Quality report for an entire dataset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityReport {
    /// Path to the dataset that was analyzed.
    pub dataset_path: String,
    /// Number of episodes analyzed.
    pub num_episodes: usize,
    /// Mean composite score across all episodes.
    pub overall_score: f64,
    /// ISO 8601 timestamp of when the analysis was computed.
    pub computed_at: String,
    /// Per-episode quality assessments.
    pub per_episode: Vec<EpisodeQuality>,
    /// Dataset-level quality flags.
    #[serde(default)]
    pub flags: Vec<String>,
    /// Actionable recommendations based on the analysis.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recommendations: Vec<String>,
}

/// Result of episode filtering.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterResult {
    /// Total number of episodes in the source dataset.
    pub total_episodes: usize,
    /// Number of episodes that passed filtering.
    pub episodes_kept: usize,
    /// Number of episodes excluded by filtering.
    pub episodes_excluded: usize,
    /// IDs of episodes that were kept.
    pub kept_ids: Vec<String>,
    /// IDs of episodes that were excluded.
    pub excluded_ids: Vec<String>,
    /// Per-episode exclusion reasons (episode_id -> list of reasons).
    #[serde(default)]
    pub exclusion_reasons: HashMap<String, Vec<String>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quality_report_serde_roundtrip() {
        let mut metric_scores = HashMap::new();
        metric_scores.insert(
            "smoothness".to_string(),
            MetricScore {
                normalized: Some(0.85),
                raw: Some(-4.2),
                warning: None,
            },
        );
        metric_scores.insert(
            "dead_actions".to_string(),
            MetricScore {
                normalized: Some(0.92),
                raw: Some(0.08),
                warning: None,
            },
        );
        metric_scores.insert(
            "gripper_chatter".to_string(),
            MetricScore {
                normalized: None,
                raw: None,
                warning: Some("skipped, no gripper data".to_string()),
            },
        );

        let episode = EpisodeQuality {
            episode_id: "episode_0".to_string(),
            num_frames: 100,
            duration_secs: Some(3.33),
            cameras: vec!["top".to_string(), "wrist".to_string()],
            composite_score: Some(0.88),
            metric_scores,
            flags: vec!["dead actions detected".to_string()],
        };

        let report = QualityReport {
            dataset_path: "/data/test_dataset".to_string(),
            num_episodes: 1,
            overall_score: 0.88,
            computed_at: "2026-02-22T00:00:00Z".to_string(),
            per_episode: vec![episode],
            flags: vec![],
            recommendations: vec!["Consider re-recording episodes with low smoothness".to_string()],
        };

        // Serialize to JSON
        let json = serde_json::to_string_pretty(&report).expect("serialize");

        // Deserialize back
        let roundtripped: QualityReport =
            serde_json::from_str(&json).expect("deserialize");

        assert_eq!(roundtripped.dataset_path, report.dataset_path);
        assert_eq!(roundtripped.num_episodes, report.num_episodes);
        assert!((roundtripped.overall_score - report.overall_score).abs() < 1e-10);
        assert_eq!(roundtripped.per_episode.len(), 1);

        let ep = &roundtripped.per_episode[0];
        assert_eq!(ep.episode_id, "episode_0");
        assert_eq!(ep.num_frames, 100);
        assert_eq!(ep.composite_score, Some(0.88));
        assert_eq!(ep.metric_scores.len(), 3);
        assert_eq!(
            ep.metric_scores["gripper_chatter"].warning,
            Some("skipped, no gripper data".to_string())
        );
        assert_eq!(roundtripped.recommendations.len(), 1);
    }

    #[test]
    fn filter_result_serde_roundtrip() {
        let mut reasons = HashMap::new();
        reasons.insert(
            "episode_2".to_string(),
            vec!["smoothness below 0.5".to_string()],
        );

        let result = FilterResult {
            total_episodes: 10,
            episodes_kept: 8,
            episodes_excluded: 2,
            kept_ids: vec!["episode_0".to_string(), "episode_1".to_string()],
            excluded_ids: vec!["episode_2".to_string()],
            exclusion_reasons: reasons,
        };

        let json = serde_json::to_string(&result).expect("serialize");
        let roundtripped: FilterResult = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(roundtripped.total_episodes, 10);
        assert_eq!(roundtripped.episodes_kept, 8);
        assert_eq!(roundtripped.exclusion_reasons.len(), 1);
    }

    #[test]
    fn empty_recommendations_omitted_in_json() {
        let report = QualityReport {
            dataset_path: "/data/test".to_string(),
            num_episodes: 0,
            overall_score: 0.0,
            computed_at: "2026-01-01T00:00:00Z".to_string(),
            per_episode: vec![],
            flags: vec![],
            recommendations: vec![],
        };

        let json = serde_json::to_string(&report).expect("serialize");
        assert!(
            !json.contains("recommendations"),
            "empty recommendations should be omitted from JSON"
        );
    }
}
