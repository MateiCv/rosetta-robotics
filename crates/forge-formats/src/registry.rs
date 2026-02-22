//! Static format capability registry.
//!
//! Provides a compile-time list of all supported dataset formats and their
//! read/write capabilities. New formats are added here as they are implemented
//! in later phases.

use std::path::Path;

use forge_core::{ForgeError, ForgeResult};

/// Describes the capabilities of a single dataset format.
#[derive(Debug, Clone, Copy)]
pub struct FormatCapability {
    /// Machine-readable format identifier (e.g., "lerobot-v3").
    pub name: &'static str,
    /// Whether this format can be read (imported).
    pub can_read: bool,
    /// Whether this format can be written (exported).
    pub can_write: bool,
    /// Human-readable description of the format.
    pub description: &'static str,
}

/// All dataset formats known to Forge.
///
/// Returns a static slice of [`FormatCapability`] entries. New formats are
/// added to this array as their reader/writer implementations land.
pub fn all_formats() -> &'static [FormatCapability] {
    static FORMATS: &[FormatCapability] = &[FormatCapability {
        name: "lerobot-v3",
        can_read: true,
        can_write: true,
        description: "LeRobot v3 (Parquet + MP4 video)",
    }];
    FORMATS
}

/// Attempt to auto-detect the dataset format at `path`.
///
/// Iterates over registered format readers and returns the first format
/// whose `can_read()` returns `true`. Currently a stub -- full implementation
/// arrives with the LeRobot v3 reader in Plan 03.
pub fn detect_format(path: &Path) -> ForgeResult<&'static str> {
    // Stub: no readers registered yet to probe.
    // Full implementation will iterate FormatReader instances.
    Err(ForgeError::FormatDetection {
        path: path.to_path_buf(),
    })
}
