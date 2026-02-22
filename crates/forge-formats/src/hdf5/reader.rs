//! HDF5 FormatReader and FormatDetector implementations.
//!
//! Provides `HDF5Reader` (implements `FormatReader`) and `HDF5Detector`
//! (implements `FormatDetector`) for reading and detecting HDF5 robotics
//! datasets in both robomimic and ALOHA layouts.
//!
//! Full implementation in Task 2.

use std::path::Path;

use forge_core::{DatasetInfo, Episode, ForgeResult, ReadOutput};

use crate::detection::{DetectionResult, FormatDetector};
use crate::traits::FormatReader;

/// Stateless reader for HDF5 robotics datasets.
pub struct HDF5Reader;

impl FormatReader for HDF5Reader {
    fn format_name(&self) -> &str {
        "hdf5"
    }

    fn can_read(&self, _path: &Path) -> bool {
        false // Stub -- implemented in Task 2
    }

    fn inspect(&self, _path: &Path) -> ForgeResult<DatasetInfo> {
        Err(forge_core::ForgeError::Config(
            "HDF5Reader.inspect() not yet implemented".to_string(),
        ))
    }

    fn read_episodes(&self, _path: &Path) -> ForgeResult<ReadOutput<Vec<Episode>>> {
        Err(forge_core::ForgeError::Config(
            "HDF5Reader.read_episodes() not yet implemented".to_string(),
        ))
    }
}

/// Scored format detector for HDF5 datasets.
pub struct HDF5Detector;

impl FormatDetector for HDF5Detector {
    fn detect(&self, _path: &Path) -> Option<DetectionResult> {
        None // Stub -- implemented in Task 2
    }
}
