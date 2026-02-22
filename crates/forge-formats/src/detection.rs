//! Scored format detection engine.
//!
//! Provides a confidence-ranked detection system where each format detector
//! returns a score (0.0-1.0) indicating how likely a path is a dataset in
//! that format. The engine collects all scores, sorts by confidence, and
//! warns about ambiguous results.
//!
//! Per user decision: best-guess with warning when ambiguous. Proceed with
//! top pick. `--format` override flag always respected (handled upstream).

use std::path::Path;

use forge_core::{ForgeError, ForgeResult};

use crate::groot::GR00TDetector;
#[cfg(feature = "hdf5-format")]
use crate::hdf5::HDF5Detector;
use crate::lerobot_v2::LeRobotV2Detector;
use crate::lerobot_v3::reader::LeRobotV3Reader;
#[cfg(feature = "rlds-format")]
use crate::rlds::RLDSDetector;
#[cfg(feature = "rosbag-format")]
use crate::rosbag::RosbagDetector;
use crate::traits::FormatReader;
#[cfg(feature = "zarr-format")]
use crate::zarr::ZarrDetector;

/// Confidence gap below which two detection results are considered ambiguous.
pub const AMBIGUITY_THRESHOLD: f32 = 0.1;

/// Result from a single format detector, carrying a confidence score
/// and human-readable evidence for the detection decision.
#[derive(Debug, Clone)]
pub struct DetectionResult {
    /// Machine-readable format identifier (e.g., "lerobot-v3").
    pub format_name: &'static str,
    /// Confidence score from 0.0 (no match) to 1.0 (certain match).
    pub confidence: f32,
    /// Human-readable reasons supporting the detection (for diagnostics).
    pub evidence: Vec<String>,
}

/// Trait for scored format detection.
///
/// Implementors examine a filesystem path and return a confidence-scored
/// result if the path could be a dataset in their format. Returns `None`
/// if the path is definitely not this format.
///
/// This is separate from [`FormatReader::can_read`] (which returns a
/// boolean). Detectors can perform more thorough checks (file headers,
/// metadata parsing) and return graded confidence.
pub trait FormatDetector: Send + Sync {
    /// Score how likely `path` is a dataset in this format.
    ///
    /// Returns `None` if definitely not this format.
    /// Returns `Some(DetectionResult)` with a confidence score otherwise.
    fn detect(&self, path: &Path) -> Option<DetectionResult>;
}

/// Detect the dataset format at `path` using all registered detectors.
///
/// Returns a vector of detection results sorted by confidence (highest
/// first). Logs a warning if the top two results are within
/// [`AMBIGUITY_THRESHOLD`] of each other.
///
/// # Errors
///
/// Returns [`ForgeError::FormatDetection`] if no detectors match.
pub fn detect_format_scored(path: &Path) -> ForgeResult<Vec<DetectionResult>> {
    // Register all available detectors. Order: most specific first.
    // New detectors are added here as format readers are implemented.
    let detectors: Vec<Box<dyn FormatDetector>> = vec![
        // Phase 5 detectors: most specific first.
        Box::new(GR00TDetector), // Must be before LeRobotV2 (more specific)
        // Box::new(RoboDMDetector),
        #[cfg(feature = "rlds-format")]
        Box::new(RLDSDetector),
        #[cfg(feature = "zarr-format")]
        Box::new(ZarrDetector),
        #[cfg(feature = "hdf5-format")]
        Box::new(HDF5Detector),
        #[cfg(feature = "rosbag-format")]
        Box::new(RosbagDetector),
        Box::new(LeRobotV3Detector),
        Box::new(LeRobotV2Detector),
    ];

    let mut results: Vec<DetectionResult> = detectors
        .iter()
        .filter_map(|d| d.detect(path))
        .collect();

    results.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    if results.is_empty() {
        return Err(ForgeError::FormatDetection {
            path: path.to_path_buf(),
        });
    }

    // Warn if top two results are ambiguous (within AMBIGUITY_THRESHOLD).
    if results.len() >= 2
        && (results[0].confidence - results[1].confidence) < AMBIGUITY_THRESHOLD
    {
        eprintln!(
            "Warning: format detection ambiguous for '{}'. \
             Top match: {} ({:.0}%), runner-up: {} ({:.0}%). \
             Proceeding with top match.",
            path.display(),
            results[0].format_name,
            results[0].confidence * 100.0,
            results[1].format_name,
            results[1].confidence * 100.0,
        );
    }

    Ok(results)
}

/// Detector for LeRobot v3 datasets.
///
/// Wraps the existing `LeRobotV3Reader::can_read()` check with a
/// confidence score. When `can_read()` returns true, the score is high
/// because the check already verifies `meta/info.json` with
/// `codebase_version` starting with "v3" or "3".
struct LeRobotV3Detector;

impl FormatDetector for LeRobotV3Detector {
    fn detect(&self, path: &Path) -> Option<DetectionResult> {
        let reader = LeRobotV3Reader;
        if reader.can_read(path) {
            Some(DetectionResult {
                format_name: "lerobot-v3",
                confidence: 0.95,
                evidence: vec![
                    "meta/info.json found".to_string(),
                    "codebase_version indicates v3".to_string(),
                ],
            })
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn detect_format_scored_returns_error_for_nonexistent_path() {
        let result = detect_format_scored(&PathBuf::from("/nonexistent/path"));
        assert!(result.is_err());
    }

    #[test]
    fn detection_result_fields_accessible() {
        let result = DetectionResult {
            format_name: "test-format",
            confidence: 0.75,
            evidence: vec!["found test marker".to_string()],
        };
        assert_eq!(result.format_name, "test-format");
        assert!((result.confidence - 0.75).abs() < f32::EPSILON);
        assert_eq!(result.evidence.len(), 1);
    }

    #[test]
    fn ambiguity_threshold_is_reasonable() {
        assert!(AMBIGUITY_THRESHOLD > 0.0);
        assert!(AMBIGUITY_THRESHOLD < 0.5);
    }
}
