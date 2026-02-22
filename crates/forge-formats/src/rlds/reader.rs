//! RLDS FormatReader implementation.
//!
//! Placeholder for Task 2. This module will implement `FormatReader` for
//! RLDS datasets and `FormatDetector` for scored detection.

use std::path::Path;

use forge_core::{DatasetInfo, Episode, ForgeResult, ReadOutput};

use crate::detection::{DetectionResult, FormatDetector};
use crate::traits::FormatReader;

/// Stateless reader for RLDS/TFRecord datasets.
pub struct RLDSReader;

/// Scored detector for RLDS/TFRecord datasets.
pub struct RLDSDetector;

impl FormatReader for RLDSReader {
    fn format_name(&self) -> &str {
        "rlds"
    }

    fn can_read(&self, _path: &Path) -> bool {
        // Stub: will be implemented in Task 2.
        false
    }

    fn inspect(&self, _path: &Path) -> ForgeResult<DatasetInfo> {
        Err(forge_core::ForgeError::Config(
            "RLDS inspect not yet implemented".into(),
        ))
    }

    fn read_episodes(&self, _path: &Path) -> ForgeResult<ReadOutput<Vec<Episode>>> {
        Err(forge_core::ForgeError::Config(
            "RLDS read_episodes not yet implemented".into(),
        ))
    }
}

impl FormatDetector for RLDSDetector {
    fn detect(&self, _path: &Path) -> Option<DetectionResult> {
        // Stub: will be implemented in Task 2.
        None
    }
}
