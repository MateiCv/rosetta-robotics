//! Quality analyzer orchestrator.
//!
//! [`QualityAnalyzer`] ties all metrics together: it reads episodes from a
//! [`FormatReader`], feeds frames through all configured accumulators, computes
//! composite scores with weight redistribution, supports early exit
//! optimization, and produces exportable [`QualityReport`] JSON.
//!
//! # Critical Invariant
//!
//! Quality analysis does NOT load images -- only action, state, and timestamp
//! data from frames. This keeps memory usage minimal.

use std::collections::HashMap;
use std::path::Path;

use forge_core::{Episode, ForgeError, ForgeResult};
use forge_formats::FormatReader;

use crate::config::QualityConfig;
use crate::early_exit::{check_early_exit, EarlyExitDecision};
use crate::metrics::composite::composite_score;
use crate::metrics::{create_accumulators, FrameContext, MetricDetails, MetricKind};
use crate::models::{EpisodeQuality, MetricScore, QualityReport};

/// Orchestrates quality analysis across episodes.
///
/// Given a [`QualityConfig`], analyzes individual episodes or entire datasets,
/// producing [`EpisodeQuality`] and [`QualityReport`] results with JSON export.
pub struct QualityAnalyzer {
    config: QualityConfig,
}

impl QualityAnalyzer {
    /// Create a new analyzer with the given configuration.
    pub fn new(config: QualityConfig) -> Self {
        Self { config }
    }

    /// Analyze a single episode and produce quality scores.
    ///
    /// Creates accumulators, streams frames through them, optionally applies
    /// early exit, and returns the composite + per-metric scores.
    ///
    /// `dataset_fps` is the FPS from dataset metadata, used as fallback when
    /// `config.fps` is `None`.
    pub fn analyze_episode(&self, episode: &Episode, dataset_fps: Option<f64>) -> EpisodeQuality {
        let mut accumulators = create_accumulators(&self.config);
        let fps = self.config.fps.or(dataset_fps).unwrap_or(30.0);
        let dt = 1.0 / fps;
        let weights = self.config.effective_weights();
        let threshold = self.config.composite_threshold;

        let mut early_exited = false;

        for (i, frame) in episode.frames.iter().enumerate() {
            // Load action and state (NO images).
            let action_data = frame
                .action
                .as_ref()
                .and_then(|a| a.load().ok());
            let state_data = frame
                .state
                .as_ref()
                .and_then(|s| s.load().ok());

            // Extract timestamp from metadata, fall back to index * dt.
            let ts = frame
                .metadata
                .get("timestamp")
                .and_then(|v| v.as_f64())
                .unwrap_or(frame.timestep as f64 * dt);

            let ctx = FrameContext {
                frame_index: frame.timestep,
                action: action_data.map(|v| v.as_slice()),
                state: state_data.map(|v| v.as_slice()),
                timestamp: Some(ts),
                dt,
            };

            for acc in accumulators.iter_mut() {
                acc.push_frame(&ctx);
            }

            // Check early exit every 10 frames (not every frame -- reduce overhead).
            if let Some(thresh) = threshold {
                if i > 0 && i % 10 == 0 {
                    match check_early_exit(&accumulators, &weights, thresh) {
                        EarlyExitDecision::Pass | EarlyExitDecision::Fail => {
                            early_exited = true;
                            break;
                        }
                        EarlyExitDecision::Continue => {}
                    }
                }
            }
        }

        // Finalize all accumulators.
        let results: Vec<_> = accumulators.iter_mut().map(|a| a.finalize()).collect();

        // Compute composite score.
        let score_pairs: Vec<(MetricKind, Option<f64>)> = results
            .iter()
            .map(|r| (r.kind, r.normalized_score))
            .collect();
        let composite = composite_score(&score_pairs, &weights);

        // Build per-metric scores map.
        let mut metric_scores = HashMap::new();
        for r in &results {
            metric_scores.insert(
                r.kind.to_string(),
                MetricScore {
                    normalized: r.normalized_score,
                    raw: r.raw_value,
                    warning: r.warning.clone(),
                },
            );
        }

        // Collect flags from metric details.
        let mut flags = Vec::new();
        for r in &results {
            match &r.details {
                MetricDetails::DeadActions { dead_ratio, .. } if *dead_ratio > 0.5 => {
                    flags.push("dead actions detected".to_string());
                }
                MetricDetails::StaticDetection { static_ratio } if *static_ratio > 0.5 => {
                    flags.push("static episode detected".to_string());
                }
                MetricDetails::GripperChatter { rate, .. } if *rate > 5.0 => {
                    flags.push("excessive gripper chatter".to_string());
                }
                MetricDetails::TimestampRegularity { gap_count, .. } if *gap_count > 0 => {
                    flags.push(format!("{gap_count} timestamp gap(s) detected"));
                }
                MetricDetails::ActionSaturation { saturated_ratio } if *saturated_ratio > 0.3 => {
                    flags.push("high action saturation".to_string());
                }
                _ => {}
            }
        }

        if early_exited {
            flags.push("early exit triggered".to_string());
        }

        // Compute duration.
        let duration_secs = if episode.frames.is_empty() {
            None
        } else {
            // Try to get duration from first and last frame timestamps.
            let first_ts = episode
                .frames
                .first()
                .and_then(|f| f.metadata.get("timestamp"))
                .and_then(|v| v.as_f64());
            let last_ts = episode
                .frames
                .last()
                .and_then(|f| f.metadata.get("timestamp"))
                .and_then(|v| v.as_f64());

            match (first_ts, last_ts) {
                (Some(t0), Some(t1)) if t1 > t0 => Some(t1 - t0),
                _ => Some(episode.frames.len() as f64 * dt),
            }
        };

        // Get camera names from first frame.
        let cameras = episode
            .frames
            .first()
            .map(|f| f.image_names().iter().map(|s| s.to_string()).collect())
            .unwrap_or_default();

        EpisodeQuality {
            episode_id: episode.episode_id.clone(),
            num_frames: episode.frames.len(),
            duration_secs,
            cameras,
            composite_score: composite,
            metric_scores,
            flags,
        }
    }

    /// Analyze an entire dataset using a format reader.
    ///
    /// Inspects the dataset for metadata (FPS, etc.), reads all episodes,
    /// and produces a complete [`QualityReport`].
    pub fn analyze_dataset(
        &self,
        reader: &dyn FormatReader,
        path: &Path,
    ) -> ForgeResult<QualityReport> {
        let info = reader.inspect(path)?;
        let dataset_fps = info.fps;

        let episodes_iter = reader.read_episodes(path)?;
        let mut per_episode = Vec::new();

        for episode_result in episodes_iter {
            match episode_result {
                Ok(episode) => {
                    let eq = self.analyze_episode(&episode, dataset_fps);
                    per_episode.push(eq);
                }
                Err(e) => {
                    // Log but continue -- partial analysis is better than no analysis.
                    eprintln!("Warning: skipping episode due to error: {e}");
                }
            }
        }

        // Compute overall score as mean of non-None composite scores.
        let valid_scores: Vec<f64> = per_episode
            .iter()
            .filter_map(|eq| eq.composite_score)
            .collect();
        let overall_score = if valid_scores.is_empty() {
            0.0
        } else {
            valid_scores.iter().sum::<f64>() / valid_scores.len() as f64
        };

        // Dataset-level flags.
        let mut flags = Vec::new();
        let below_half = per_episode
            .iter()
            .filter(|eq| eq.composite_score.is_some_and(|s| s < 0.5))
            .count();
        if below_half > 0 {
            flags.push(format!(
                "{below_half} episode(s) have quality below 0.5"
            ));
        }

        // Compute ISO 8601 timestamp.
        let computed_at = {
            let now = std::time::SystemTime::now();
            let secs = now
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            // Simple manual ISO 8601 formatting (no chrono dependency).
            format_utc_timestamp(secs)
        };

        Ok(QualityReport {
            dataset_path: path.display().to_string(),
            num_episodes: per_episode.len(),
            overall_score,
            computed_at,
            per_episode,
            flags,
            recommendations: Vec::new(),
        })
    }

    /// Export a quality report as pretty-printed JSON.
    pub fn export_json(&self, report: &QualityReport) -> ForgeResult<String> {
        serde_json::to_string_pretty(report)
            .map_err(|e| ForgeError::Config(format!("JSON export failed: {e}")))
    }

    /// Save a quality report as JSON to a file.
    pub fn save_json(&self, report: &QualityReport, path: &Path) -> ForgeResult<()> {
        let json = self.export_json(report)?;
        std::fs::write(path, json)?;
        Ok(())
    }
}

/// Format a Unix timestamp as ISO 8601 UTC string.
///
/// Manual implementation to avoid pulling in chrono as a dependency.
fn format_utc_timestamp(epoch_secs: u64) -> String {
    // Days per month (non-leap).
    const DAYS_IN_MONTH: [u64; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

    let secs_in_day = 86400u64;
    let secs_in_hour = 3600u64;
    let secs_in_min = 60u64;

    let mut remaining = epoch_secs;
    let time_of_day = remaining % secs_in_day;
    let mut days = remaining / secs_in_day;

    let hours = time_of_day / secs_in_hour;
    remaining = time_of_day % secs_in_hour;
    let minutes = remaining / secs_in_min;
    let seconds = remaining % secs_in_min;

    // Compute year/month/day from days since epoch (1970-01-01).
    let mut year = 1970u64;
    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }

    let leap = is_leap_year(year);
    let mut month = 0usize;
    loop {
        let dim = if month == 1 && leap {
            29
        } else {
            DAYS_IN_MONTH[month]
        };
        if days < dim {
            break;
        }
        days -= dim;
        month += 1;
    }

    format!(
        "{year:04}-{:02}-{:02}T{hours:02}:{minutes:02}:{seconds:02}Z",
        month + 1,
        days + 1
    )
}

fn is_leap_year(y: u64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use forge_core::schema::Dtype;
    use forge_core::{Episode, Frame, LazyArray};

    /// Create a synthetic episode with `n` frames, each having action and state data.
    fn make_test_episode(n: usize) -> Episode {
        let mut episode = Episode::new("test_episode_0");
        let dt = 1.0 / 30.0;

        for i in 0..n {
            let mut frame = Frame::new(i);

            // 6-DOF action: linearly increasing values.
            let action = (0..6).map(|d| (i as f64 * 0.1) + d as f64 * 0.01).collect::<Vec<f64>>();
            frame.action = Some(LazyArray::from_vec(action, vec![6], Dtype::Float64));

            // 6-DOF state: slowly varying.
            let state = (0..6).map(|d| (i as f64 * 0.05) + d as f64 * 0.02).collect::<Vec<f64>>();
            frame.state = Some(LazyArray::from_vec(state, vec![6], Dtype::Float64));

            // Timestamp metadata.
            frame.metadata.insert(
                "timestamp".to_string(),
                serde_json::Value::from(i as f64 * dt),
            );

            episode.frames.push(frame);
        }

        episode
    }

    #[test]
    fn analyze_episode_produces_all_seven_metrics() {
        let config = QualityConfig::default();
        let analyzer = QualityAnalyzer::new(config);
        let episode = make_test_episode(20);

        let result = analyzer.analyze_episode(&episode, None);

        assert_eq!(result.episode_id, "test_episode_0");
        assert_eq!(result.num_frames, 20);
        assert_eq!(
            result.metric_scores.len(),
            7,
            "expected 7 metric scores, got {}",
            result.metric_scores.len()
        );
        assert!(
            result.composite_score.is_some(),
            "composite score should be present"
        );
        let cs = result.composite_score.unwrap();
        assert!(
            (0.0..=1.0).contains(&cs),
            "composite score {cs} not in 0-1 range"
        );
    }

    #[test]
    fn analyze_episode_skipped_metric_reduces_count() {
        let mut config = QualityConfig::default();
        config.skip_metrics.push(MetricKind::GripperChatter);
        let analyzer = QualityAnalyzer::new(config);
        let episode = make_test_episode(20);

        let result = analyzer.analyze_episode(&episode, None);
        assert_eq!(
            result.metric_scores.len(),
            6,
            "expected 6 metric scores when chatter is skipped"
        );
        assert!(
            !result.metric_scores.contains_key("gripper_chatter"),
            "gripper_chatter should be absent"
        );
    }

    #[test]
    fn json_export_roundtrip() {
        let config = QualityConfig::default();
        let analyzer = QualityAnalyzer::new(config);
        let episode = make_test_episode(20);

        let eq = analyzer.analyze_episode(&episode, None);
        let report = QualityReport {
            dataset_path: "/test/dataset".to_string(),
            num_episodes: 1,
            overall_score: eq.composite_score.unwrap_or(0.0),
            computed_at: "2026-02-22T00:00:00Z".to_string(),
            per_episode: vec![eq],
            flags: vec![],
            recommendations: vec![],
        };

        let json = analyzer.export_json(&report).unwrap();
        assert!(!json.is_empty());

        // Round-trip: deserialize back.
        let rt: QualityReport = serde_json::from_str(&json).unwrap();
        assert_eq!(rt.dataset_path, "/test/dataset");
        assert_eq!(rt.num_episodes, 1);
        assert_eq!(rt.per_episode.len(), 1);
        assert_eq!(rt.per_episode[0].metric_scores.len(), 7);
    }

    #[test]
    fn early_exit_with_threshold_zero_passes_early() {
        let mut config = QualityConfig::default();
        // threshold=0.0 means any composite >= 0 passes immediately.
        config.composite_threshold = Some(0.0);
        let analyzer = QualityAnalyzer::new(config);
        let episode = make_test_episode(100);

        let result = analyzer.analyze_episode(&episode, None);
        // Early exit should trigger since all score bounds start at (0.0, 1.0)
        // meaning min_composite >= 0.0 is true from the start.
        assert!(
            result.flags.contains(&"early exit triggered".to_string()),
            "expected early exit flag, got flags: {:?}",
            result.flags
        );
    }

    #[test]
    fn analyze_episode_empty_episode() {
        let config = QualityConfig::default();
        let analyzer = QualityAnalyzer::new(config);
        let episode = Episode::new("empty");

        let result = analyzer.analyze_episode(&episode, None);
        assert_eq!(result.num_frames, 0);
        assert_eq!(result.episode_id, "empty");
        assert!(result.duration_secs.is_none());
    }

    #[test]
    fn format_utc_timestamp_known_date() {
        // 2026-02-22T00:00:00Z = 1771718400
        let ts = format_utc_timestamp(1771718400);
        assert_eq!(ts, "2026-02-22T00:00:00Z");
    }

    #[test]
    fn format_utc_timestamp_epoch() {
        assert_eq!(format_utc_timestamp(0), "1970-01-01T00:00:00Z");
    }
}
