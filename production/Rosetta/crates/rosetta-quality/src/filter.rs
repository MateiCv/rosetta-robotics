//! Episode filtering engine for quality-based dataset cleaning.
//!
//! [`FilterEngine`] evaluates episodes against quality criteria (composite score
//! threshold, per-metric thresholds, manual overrides) and writes passing
//! episodes to a new output dataset. Supports dry-run mode, lazy metadata
//! pre-filtering, and copy-only semantics (source is never modified).

use std::collections::HashMap;
use std::path::Path;

use rosetta_core::{Episode, RosettaError, RosettaResult};
use rosetta_formats::{FormatReader, FormatWriter};

use crate::analyzer::QualityAnalyzer;
use crate::config::{FilterConfig, QualityConfig};
use crate::models::{EpisodeQuality, FilterResult};

/// Decision for whether to keep or remove an episode, with a reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilterDecision {
    /// Keep the episode, with a reason string.
    Keep(&'static str),
    /// Remove the episode, with a reason string.
    Remove(&'static str),
}

/// Evaluates episodes against quality criteria and writes passing episodes
/// to a new output dataset.
///
/// The filter engine uses copy-only semantics: the source dataset is never
/// modified. Output must be written to a different path. Supports dry-run
/// mode (output=None) which computes filtering decisions without writing,
/// and lazy metadata pre-filtering to skip obviously bad episodes before
/// full quality analysis.
pub struct FilterEngine {
    #[allow(dead_code)]
    quality_config: QualityConfig,
    filter_config: FilterConfig,
    analyzer: QualityAnalyzer,
}

impl FilterEngine {
    /// Create a new filter engine with the given quality and filter configurations.
    pub fn new(quality_config: QualityConfig, filter_config: FilterConfig) -> Self {
        let analyzer = QualityAnalyzer::new(quality_config.clone());
        Self {
            quality_config,
            filter_config,
            analyzer,
        }
    }

    /// Filter episodes from a dataset, writing passing episodes to `output`.
    ///
    /// If `output` is `None`, performs a dry run: computes filtering decisions
    /// and returns a [`FilterResult`] without writing any data.
    ///
    /// If `output` is `Some`, writes passing episodes via the provided writer
    /// and finalizes the output dataset with updated metadata.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Path validation fails (output overlaps source)
    /// - The reader cannot inspect or read the source dataset
    /// - The writer fails to write an episode or finalize
    pub fn filter(
        &self,
        reader: &dyn FormatReader,
        writer: &mut dyn FormatWriter,
        source: &Path,
        output: Option<&Path>,
    ) -> RosettaResult<FilterResult> {
        if let Some(out) = output {
            validate_paths(source, out)?;
        }

        let info = reader.inspect(source)?;
        let dataset_fps = info.fps;
        let read_output = reader.read_episodes(source)?;

        let mut kept_ids = Vec::new();
        let mut excluded_ids = Vec::new();
        let mut exclusion_reasons: HashMap<String, Vec<String>> = HashMap::new();
        let mut kept_index = 0usize;
        let mut total_kept_frames = 0usize;

        for episode in read_output.data {

            let episode_idx = episode.index().unwrap_or(0) as usize;

            // Lazy metadata pre-filter (QUAL-15).
            if let Some(decision) = self.should_skip_by_metadata(&episode) {
                match decision {
                    FilterDecision::Keep(reason) => {
                        if let Some(out) = output {
                            writer.write_episode(&episode, out, kept_index)?;
                        }
                        total_kept_frames += episode.frames.len();
                        kept_ids.push(episode.episode_id.clone());
                        kept_index += 1;
                        // Log that it was a manual keep.
                        let _ = reason;
                    }
                    FilterDecision::Remove(reason) => {
                        excluded_ids.push(episode.episode_id.clone());
                        exclusion_reasons
                            .entry(episode.episode_id.clone())
                            .or_default()
                            .push(reason.to_string());
                    }
                }
                continue;
            }

            // Full quality analysis.
            let quality = self.analyzer.analyze_episode(&episode, dataset_fps);

            // Evaluate against thresholds.
            let (passes, fail_reasons) = self.evaluate_episode(&quality);

            // Manual overrides take precedence over quality-based decisions.
            let final_keep = if self.filter_config.keep_episodes.contains(&episode_idx) {
                true
            } else if self.filter_config.remove_episodes.contains(&episode_idx) {
                false
            } else {
                passes
            };

            if final_keep {
                if let Some(out) = output {
                    writer.write_episode(&episode, out, kept_index)?;
                }
                total_kept_frames += episode.frames.len();
                kept_ids.push(episode.episode_id.clone());
                kept_index += 1;
            } else {
                excluded_ids.push(episode.episode_id.clone());
                let reasons = if self.filter_config.remove_episodes.contains(&episode_idx) {
                    vec!["manual remove override".to_string()]
                } else {
                    fail_reasons
                };
                exclusion_reasons
                    .entry(episode.episode_id.clone())
                    .or_default()
                    .extend(reasons);
            }
        }

        // Finalize output dataset if writing.
        if let Some(out) = output {
            let mut filtered_info = info.clone();
            filtered_info.total_episodes = kept_ids.len();
            filtered_info.total_frames = total_kept_frames;
            writer.finalize(out, &filtered_info)?;
        }

        let total = kept_ids.len() + excluded_ids.len();
        Ok(FilterResult {
            total_episodes: total,
            episodes_kept: kept_ids.len(),
            episodes_excluded: excluded_ids.len(),
            kept_ids,
            excluded_ids,
            exclusion_reasons,
        })
    }

    /// Lazy metadata pre-filtering (QUAL-15).
    ///
    /// Checks manual overrides and basic metadata heuristics before loading
    /// full frame data for quality analysis. Returns `Some(decision)` if a
    /// determination can be made from metadata alone, `None` if full analysis
    /// is needed.
    fn should_skip_by_metadata(&self, episode: &Episode) -> Option<FilterDecision> {
        let episode_idx = episode.index().unwrap_or(0) as usize;

        // Manual overrides checked first.
        if self.filter_config.keep_episodes.contains(&episode_idx) {
            return Some(FilterDecision::Keep("manual keep override"));
        }
        if self.filter_config.remove_episodes.contains(&episode_idx) {
            return Some(FilterDecision::Remove("manual remove override"));
        }

        // Empty episode: no frames to analyze.
        if episode.frames.is_empty() {
            return Some(FilterDecision::Remove("empty episode"));
        }

        // Action data on first frame: if absent, we still proceed to full
        // analysis since metrics handle missing data gracefully.
        None
    }

    /// Evaluate an episode's quality against configured thresholds.
    ///
    /// Uses AND logic: the episode must pass both the composite threshold
    /// and ALL per-metric thresholds to be kept.
    ///
    /// Returns `(passes, reasons)` where `reasons` lists why the episode
    /// failed (empty if it passed).
    fn evaluate_episode(&self, quality: &EpisodeQuality) -> (bool, Vec<String>) {
        let mut reasons = Vec::new();

        // Check composite threshold.
        if let Some(threshold) = self.filter_config.min_quality {
            match quality.composite_score {
                Some(score) if score < threshold => {
                    reasons.push(format!(
                        "composite score {score:.2} below threshold {threshold:.2}"
                    ));
                }
                None => {
                    reasons.push("composite score unavailable".to_string());
                }
                _ => {}
            }
        }

        // Check per-metric thresholds (AND logic).
        for (kind, threshold) in &self.filter_config.per_metric_thresholds {
            let metric_name = kind.to_string();
            if let Some(metric_score) = quality.metric_scores.get(&metric_name) {
                match metric_score.normalized {
                    Some(score) if score < *threshold => {
                        reasons.push(format!(
                            "{metric_name} score {score:.2} below threshold {threshold:.2}"
                        ));
                    }
                    None => {
                        reasons.push(format!("{metric_name} score unavailable"));
                    }
                    _ => {}
                }
            } else {
                reasons.push(format!("{metric_name} metric not computed"));
            }
        }

        (reasons.is_empty(), reasons)
    }

    /// Format a dry-run result as a human-readable summary string.
    ///
    /// Output format: "Would keep X/Y episodes (Z%). N removed by: reason (count), ..."
    pub fn format_dry_run(result: &FilterResult) -> String {
        let total = result.total_episodes;
        let kept = result.episodes_kept;
        let removed = result.episodes_excluded;

        if removed == 0 {
            return format!("Would keep {total}/{total} episodes (100%). None removed.");
        }

        let pct = if total > 0 {
            (kept as f64 / total as f64) * 100.0
        } else {
            0.0
        };

        // Count exclusion reasons across all excluded episodes.
        let mut reason_counts: HashMap<String, usize> = HashMap::new();
        for reasons in result.exclusion_reasons.values() {
            for reason in reasons {
                // Extract the metric/category name from the reason string.
                let category = extract_reason_category(reason);
                *reason_counts.entry(category).or_insert(0) += 1;
            }
        }

        // Sort by count descending for consistent output.
        let mut sorted_reasons: Vec<_> = reason_counts.into_iter().collect();
        sorted_reasons.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

        let reasons_str = sorted_reasons
            .iter()
            .map(|(name, count)| format!("{name} ({count})"))
            .collect::<Vec<_>>()
            .join(", ");

        format!("Would keep {kept}/{total} episodes ({pct:.0}%). {removed} removed by: {reasons_str}")
    }
}

/// Extract a short category name from a full exclusion reason string.
///
/// Converts "smoothness score 0.30 below threshold 0.50" to "smoothness"
/// and "composite score 0.30 below threshold 0.50" to "composite".
fn extract_reason_category(reason: &str) -> String {
    // Check for metric-specific reasons.
    if let Some(name) = reason.strip_suffix(" score unavailable") {
        return name.to_string();
    }
    if let Some(rest) = reason.strip_suffix(" metric not computed") {
        return rest.to_string();
    }
    // "X score Y below threshold Z" -> X
    if let Some(idx) = reason.find(" score ") {
        return reason[..idx].to_string();
    }
    // Manual overrides and other reasons.
    if reason.contains("manual") {
        return "manual override".to_string();
    }
    if reason.contains("empty") {
        return "empty episode".to_string();
    }
    reason.to_string()
}

/// Validate that source and output paths do not overlap.
///
/// Canonicalizes both paths and checks for identity or ancestor relationships.
/// For the output path, if it does not exist yet, canonicalizes the parent
/// directory and appends the filename.
///
/// # Errors
///
/// Returns `RosettaError::Config` if:
/// - Output path is identical to source path
/// - Output is an ancestor of source or source is an ancestor of output
fn validate_paths(source: &Path, output: &Path) -> RosettaResult<()> {
    // Canonicalize source (must exist).
    let canon_source = std::fs::canonicalize(source).map_err(|e| {
        RosettaError::Config(format!(
            "cannot canonicalize source path '{}': {e}",
            source.display()
        ))
    })?;

    // Canonicalize output: may not exist yet, so use parent + filename.
    let canon_output = if output.exists() {
        std::fs::canonicalize(output).map_err(|e| {
            RosettaError::Config(format!(
                "cannot canonicalize output path '{}': {e}",
                output.display()
            ))
        })?
    } else {
        let parent = output.parent().unwrap_or(Path::new("."));
        let parent_canon = std::fs::canonicalize(parent).map_err(|e| {
            RosettaError::Config(format!(
                "cannot canonicalize output parent '{}': {e}",
                parent.display()
            ))
        })?;
        let filename = output
            .file_name()
            .ok_or_else(|| RosettaError::Config("output path has no filename".to_string()))?;
        parent_canon.join(filename)
    };

    // Check identity.
    if canon_source == canon_output {
        return Err(RosettaError::Config(
            "output path must differ from source path".to_string(),
        ));
    }

    // Check ancestor relationships.
    if canon_output.starts_with(&canon_source) {
        return Err(RosettaError::Config(
            "output path must not overlap with source path".to_string(),
        ));
    }
    if canon_source.starts_with(&canon_output) {
        return Err(RosettaError::Config(
            "output path must not overlap with source path".to_string(),
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rosetta_core::schema::Dtype;
    use rosetta_core::{DatasetInfo, Frame, LazyArray, ReadOutput};
    use crate::metrics::MetricKind;
    use std::path::PathBuf;
    use std::sync::Mutex;

    // ---- Helpers ----

    /// Create a test episode with the given ID, number of frames, and action
    /// values that influence quality scoring.
    fn make_episode(id: &str, num_frames: usize, action_base: f64) -> Episode {
        let mut episode = Episode::new(id);
        let dt = 1.0 / 30.0;

        for i in 0..num_frames {
            let mut frame = Frame::new(i);

            // 6-DOF action: base value + small linear ramp.
            let action: Vec<f64> = (0..6)
                .map(|d| action_base + (i as f64 * 0.1) + d as f64 * 0.01)
                .collect();
            frame.action = Some(LazyArray::from_vec(action, vec![6], Dtype::Float64));

            // 6-DOF state: slowly varying.
            let state: Vec<f64> = (0..6)
                .map(|d| (i as f64 * 0.05) + d as f64 * 0.02)
                .collect();
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

    /// Create an episode with dead (zero) actions that will score poorly.
    fn make_dead_episode(id: &str, num_frames: usize) -> Episode {
        let mut episode = Episode::new(id);
        let dt = 1.0 / 30.0;

        for i in 0..num_frames {
            let mut frame = Frame::new(i);

            // All zeros -- dead actions.
            let action = vec![0.0; 6];
            frame.action = Some(LazyArray::from_vec(action, vec![6], Dtype::Float64));

            let state = vec![0.0; 6];
            frame.state = Some(LazyArray::from_vec(state, vec![6], Dtype::Float64));

            frame.metadata.insert(
                "timestamp".to_string(),
                serde_json::Value::from(i as f64 * dt),
            );

            episode.frames.push(frame);
        }

        episode
    }

    // ---- Mock FormatReader/Writer ----

    /// A mock reader that builds episodes from stored parameters.
    struct MockReaderWithEpisodes {
        /// (episode_id, num_frames, action_base, is_dead)
        episode_params: Vec<(String, usize, f64, bool)>,
        info: DatasetInfo,
    }

    impl FormatReader for MockReaderWithEpisodes {
        fn format_name(&self) -> &str {
            "mock"
        }

        fn can_read(&self, _path: &Path) -> bool {
            true
        }

        fn inspect(&self, _path: &Path) -> RosettaResult<DatasetInfo> {
            Ok(self.info.clone())
        }

        fn read_episodes(
            &self,
            _path: &Path,
        ) -> RosettaResult<ReadOutput<Vec<Episode>>> {
            let episodes: Vec<Episode> = self
                .episode_params
                .iter()
                .map(|(id, frames, base, is_dead)| {
                    if *is_dead {
                        make_dead_episode(id, *frames)
                    } else {
                        make_episode(id, *frames, *base)
                    }
                })
                .collect();
            Ok(ReadOutput::new(episodes))
        }
    }

    struct MockWriter {
        written_episodes: Mutex<Vec<(String, usize)>>,
        finalized: Mutex<bool>,
    }

    impl MockWriter {
        fn new() -> Self {
            Self {
                written_episodes: Mutex::new(Vec::new()),
                finalized: Mutex::new(false),
            }
        }

        fn written_count(&self) -> usize {
            self.written_episodes.lock().unwrap().len()
        }

        fn is_finalized(&self) -> bool {
            *self.finalized.lock().unwrap()
        }
    }

    impl FormatWriter for MockWriter {
        fn format_name(&self) -> &str {
            "mock"
        }

        fn write_episode(
            &mut self,
            episode: &Episode,
            _output_path: &Path,
            episode_index: usize,
        ) -> RosettaResult<()> {
            self.written_episodes
                .lock()
                .unwrap()
                .push((episode.episode_id.clone(), episode_index));
            Ok(())
        }

        fn finalize(&mut self, _output_path: &Path, _dataset_info: &DatasetInfo) -> RosettaResult<()> {
            *self.finalized.lock().unwrap() = true;
            Ok(())
        }
    }

    fn mock_dataset_info() -> DatasetInfo {
        DatasetInfo {
            format: "mock".to_string(),
            path: PathBuf::from("/test/dataset"),
            total_episodes: 3,
            total_frames: 60,
            fps: Some(30.0),
            robot_type: None,
            fields: vec![],
            cameras: vec![],
            metadata: HashMap::new(),
        }
    }

    // ---- Path Validation Tests ----

    #[test]
    fn path_validation_same_path_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dataset");
        std::fs::create_dir_all(&path).unwrap();

        let result = validate_paths(&path, &path);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("must differ"),
            "expected 'must differ' error, got: {err}"
        );
    }

    #[test]
    fn path_validation_overlapping_paths_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("dataset");
        let output = dir.path().join("dataset").join("filtered");
        std::fs::create_dir_all(&output).unwrap();

        let result = validate_paths(&source, &output);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("must not overlap"),
            "expected overlap error, got: {err}"
        );
    }

    #[test]
    fn path_validation_different_paths_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        let output = dir.path().join("output");
        std::fs::create_dir_all(&source).unwrap();
        // Output doesn't need to exist -- parent must exist.

        let result = validate_paths(&source, &output);
        assert!(result.is_ok(), "different paths should succeed: {result:?}");
    }

    // ---- Lazy Pre-filter Tests ----

    #[test]
    fn lazy_prefilter_empty_episode_returns_remove() {
        let engine = FilterEngine::new(QualityConfig::default(), FilterConfig::default());
        let episode = Episode::new("episode_0");

        let decision = engine.should_skip_by_metadata(&episode);
        assert_eq!(decision, Some(FilterDecision::Remove("empty episode")));
    }

    #[test]
    fn lazy_prefilter_manual_keep_returns_keep() {
        let filter_config = FilterConfig {
            keep_episodes: vec![0],
            ..FilterConfig::default()
        };
        let engine = FilterEngine::new(QualityConfig::default(), filter_config);
        let episode = make_episode("episode_0", 10, 0.5);

        let decision = engine.should_skip_by_metadata(&episode);
        assert_eq!(decision, Some(FilterDecision::Keep("manual keep override")));
    }

    #[test]
    fn lazy_prefilter_manual_remove_returns_remove() {
        let filter_config = FilterConfig {
            remove_episodes: vec![0],
            ..FilterConfig::default()
        };
        let engine = FilterEngine::new(QualityConfig::default(), filter_config);
        let episode = make_episode("episode_0", 10, 0.5);

        let decision = engine.should_skip_by_metadata(&episode);
        assert_eq!(
            decision,
            Some(FilterDecision::Remove("manual remove override"))
        );
    }

    #[test]
    fn lazy_prefilter_normal_episode_returns_none() {
        let engine = FilterEngine::new(QualityConfig::default(), FilterConfig::default());
        let episode = make_episode("episode_0", 10, 0.5);

        let decision = engine.should_skip_by_metadata(&episode);
        assert!(
            decision.is_none(),
            "normal episode should proceed to full analysis"
        );
    }

    // ---- Threshold Evaluation Tests ----

    #[test]
    fn threshold_episode_above_composite_passes() {
        let filter_config = FilterConfig {
            min_quality: Some(0.5),
            ..FilterConfig::default()
        };
        let engine = FilterEngine::new(QualityConfig::default(), filter_config);

        let quality = EpisodeQuality {
            episode_id: "ep0".to_string(),
            num_frames: 20,
            duration_secs: Some(0.66),
            cameras: vec![],
            composite_score: Some(0.8),
            metric_scores: HashMap::new(),
            flags: vec![],
        };

        let (passes, reasons) = engine.evaluate_episode(&quality);
        assert!(passes, "episode with 0.8 composite should pass 0.5 threshold");
        assert!(reasons.is_empty());
    }

    #[test]
    fn threshold_episode_below_composite_fails() {
        let filter_config = FilterConfig {
            min_quality: Some(0.5),
            ..FilterConfig::default()
        };
        let engine = FilterEngine::new(QualityConfig::default(), filter_config);

        let quality = EpisodeQuality {
            episode_id: "ep0".to_string(),
            num_frames: 20,
            duration_secs: Some(0.66),
            cameras: vec![],
            composite_score: Some(0.3),
            metric_scores: HashMap::new(),
            flags: vec![],
        };

        let (passes, reasons) = engine.evaluate_episode(&quality);
        assert!(!passes, "episode with 0.3 composite should fail 0.5 threshold");
        assert!(!reasons.is_empty());
        assert!(reasons[0].contains("composite"));
    }

    #[test]
    fn threshold_per_metric_failure_adds_correct_reason() {
        let mut per_metric = HashMap::new();
        per_metric.insert(MetricKind::Smoothness, 0.7);

        let filter_config = FilterConfig {
            per_metric_thresholds: per_metric,
            ..FilterConfig::default()
        };
        let engine = FilterEngine::new(QualityConfig::default(), filter_config);

        let mut metric_scores = HashMap::new();
        metric_scores.insert(
            "smoothness".to_string(),
            crate::models::MetricScore {
                normalized: Some(0.4),
                raw: Some(-5.0),
                warning: None,
            },
        );

        let quality = EpisodeQuality {
            episode_id: "ep0".to_string(),
            num_frames: 20,
            duration_secs: Some(0.66),
            cameras: vec![],
            composite_score: Some(0.8),
            metric_scores,
            flags: vec![],
        };

        let (passes, reasons) = engine.evaluate_episode(&quality);
        assert!(!passes, "should fail smoothness per-metric threshold");
        assert!(
            reasons.iter().any(|r| r.contains("smoothness")),
            "reason should mention smoothness: {reasons:?}"
        );
    }

    #[test]
    fn threshold_no_thresholds_always_passes() {
        let engine = FilterEngine::new(QualityConfig::default(), FilterConfig::default());

        let quality = EpisodeQuality {
            episode_id: "ep0".to_string(),
            num_frames: 20,
            duration_secs: Some(0.66),
            cameras: vec![],
            composite_score: Some(0.1),
            metric_scores: HashMap::new(),
            flags: vec![],
        };

        let (passes, _) = engine.evaluate_episode(&quality);
        assert!(passes, "no thresholds should always pass");
    }

    // ---- Dry-run Format Tests ----

    #[test]
    fn dry_run_format_no_exclusions() {
        let result = FilterResult {
            total_episodes: 10,
            episodes_kept: 10,
            episodes_excluded: 0,
            kept_ids: (0..10).map(|i| format!("episode_{i}")).collect(),
            excluded_ids: vec![],
            exclusion_reasons: HashMap::new(),
        };

        let output = FilterEngine::format_dry_run(&result);
        assert_eq!(
            output,
            "Would keep 10/10 episodes (100%). None removed."
        );
    }

    #[test]
    fn dry_run_format_with_exclusions() {
        let mut reasons = HashMap::new();
        reasons.insert(
            "episode_2".to_string(),
            vec!["smoothness score 0.30 below threshold 0.50".to_string()],
        );
        reasons.insert(
            "episode_5".to_string(),
            vec!["smoothness score 0.20 below threshold 0.50".to_string()],
        );
        reasons.insert(
            "episode_7".to_string(),
            vec![
                "smoothness score 0.10 below threshold 0.50".to_string(),
                "dead_actions score 0.25 below threshold 0.50".to_string(),
            ],
        );

        let result = FilterResult {
            total_episodes: 10,
            episodes_kept: 7,
            episodes_excluded: 3,
            kept_ids: vec![],
            excluded_ids: vec![
                "episode_2".to_string(),
                "episode_5".to_string(),
                "episode_7".to_string(),
            ],
            exclusion_reasons: reasons,
        };

        let output = FilterEngine::format_dry_run(&result);
        assert!(
            output.starts_with("Would keep 7/10 episodes (70%)."),
            "unexpected format: {output}"
        );
        assert!(output.contains("3 removed by:"), "should show removed count: {output}");
        assert!(output.contains("smoothness"), "should mention smoothness: {output}");
    }

    // ---- Manual Override Tests ----

    #[test]
    fn manual_keep_overrides_low_quality() {
        let filter_config = FilterConfig {
            min_quality: Some(0.9),
            keep_episodes: vec![0],
            ..FilterConfig::default()
        };
        let engine = FilterEngine::new(QualityConfig::default(), filter_config);

        let info = mock_dataset_info();
        let reader = MockReaderWithEpisodes {
            episode_params: vec![("episode_0".to_string(), 20, 0.0, true)],
            info: info.clone(),
        };

        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        let output = dir.path().join("output");
        std::fs::create_dir_all(&source).unwrap();

        let mut writer = MockWriter::new();
        let result = engine
            .filter(&reader, &mut writer, &source, Some(&output))
            .unwrap();

        assert_eq!(result.episodes_kept, 1, "manual keep should override low quality");
        assert!(result.kept_ids.contains(&"episode_0".to_string()));
    }

    #[test]
    fn manual_remove_overrides_high_quality() {
        let filter_config = FilterConfig {
            min_quality: Some(0.0),
            remove_episodes: vec![0],
            ..FilterConfig::default()
        };
        let engine = FilterEngine::new(QualityConfig::default(), filter_config);

        let info = mock_dataset_info();
        let reader = MockReaderWithEpisodes {
            episode_params: vec![("episode_0".to_string(), 20, 0.5, false)],
            info: info.clone(),
        };

        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        let output = dir.path().join("output");
        std::fs::create_dir_all(&source).unwrap();

        let mut writer = MockWriter::new();
        let result = engine
            .filter(&reader, &mut writer, &source, Some(&output))
            .unwrap();

        assert_eq!(
            result.episodes_excluded, 1,
            "manual remove should override high quality"
        );
        assert!(result.excluded_ids.contains(&"episode_0".to_string()));
    }

    // ---- Integration Test: Mock Reader/Writer ----

    #[test]
    fn integration_filter_with_mock_reader_writer() {
        let filter_config = FilterConfig {
            min_quality: Some(0.3),
            ..FilterConfig::default()
        };
        let engine = FilterEngine::new(QualityConfig::default(), filter_config);

        let info = mock_dataset_info();
        let reader = MockReaderWithEpisodes {
            episode_params: vec![
                // Good episode: smooth ramp actions.
                ("episode_0".to_string(), 30, 0.5, false),
                // Bad episode: all dead/zero actions.
                ("episode_1".to_string(), 30, 0.0, true),
                // Mediocre episode: small actions.
                ("episode_2".to_string(), 30, 0.1, false),
            ],
            info: info.clone(),
        };

        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        let output = dir.path().join("output");
        std::fs::create_dir_all(&source).unwrap();

        let mut writer = MockWriter::new();
        let result = engine
            .filter(&reader, &mut writer, &source, Some(&output))
            .unwrap();

        assert_eq!(result.total_episodes, 3);
        assert!(
            result.episodes_kept >= 1,
            "at least the good episode should be kept"
        );
        assert!(writer.is_finalized(), "writer should be finalized");
        assert_eq!(
            writer.written_count(),
            result.episodes_kept,
            "writer should have written exactly the kept episodes"
        );
    }

    #[test]
    fn integration_dry_run_no_writes() {
        let filter_config = FilterConfig {
            min_quality: Some(0.3),
            ..FilterConfig::default()
        };
        let engine = FilterEngine::new(QualityConfig::default(), filter_config);

        let info = mock_dataset_info();
        let reader = MockReaderWithEpisodes {
            episode_params: vec![
                ("episode_0".to_string(), 30, 0.5, false),
                ("episode_1".to_string(), 30, 0.0, true),
                ("episode_2".to_string(), 30, 0.1, false),
            ],
            info: info.clone(),
        };

        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        std::fs::create_dir_all(&source).unwrap();

        let mut writer = MockWriter::new();
        // output=None means dry run.
        let result = engine
            .filter(&reader, &mut writer, &source, None)
            .unwrap();

        assert_eq!(result.total_episodes, 3);
        assert_eq!(writer.written_count(), 0, "dry run should not write");
        assert!(!writer.is_finalized(), "dry run should not finalize");
    }

    // ---- extract_reason_category Tests ----

    #[test]
    fn reason_category_extraction() {
        assert_eq!(
            extract_reason_category("smoothness score 0.30 below threshold 0.50"),
            "smoothness"
        );
        assert_eq!(
            extract_reason_category("composite score 0.30 below threshold 0.50"),
            "composite"
        );
        assert_eq!(
            extract_reason_category("dead_actions score unavailable"),
            "dead_actions"
        );
        assert_eq!(
            extract_reason_category("manual remove override"),
            "manual override"
        );
        assert_eq!(
            extract_reason_category("empty episode"),
            "empty episode"
        );
    }
}
