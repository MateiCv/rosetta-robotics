//! Static format capability registry.
//!
//! Provides a compile-time list of all supported dataset formats and their
//! read/write capabilities. New formats are added here as they are implemented
//! in later phases.

use std::path::Path;

use forge_core::{ForgeError, ForgeResult};

use crate::lerobot_v2::reader::LeRobotV2Reader;
use crate::lerobot_v3::reader::LeRobotV3Reader;
use crate::lerobot_v3::writer::LeRobotV3Writer;
use crate::traits::{FormatReader, FormatWriter};

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
    static FORMATS: &[FormatCapability] = &[
        FormatCapability {
            name: "lerobot-v3",
            can_read: true,
            can_write: true,
            description: "LeRobot v3 (Parquet + MP4 video)",
        },
        FormatCapability {
            name: "lerobot-v2",
            can_read: true,
            can_write: false,
            description: "LeRobot v2 (per-episode Parquet + MP4 video)",
        },
    ];
    FORMATS
}

/// Attempt to auto-detect the dataset format at `path`.
///
/// Iterates over registered format readers and returns the first format
/// whose `can_read()` returns `true`.
pub fn detect_format(path: &Path) -> ForgeResult<&'static str> {
    // Check each registered reader in priority order (most specific first).
    let v3_reader = LeRobotV3Reader;
    if v3_reader.can_read(path) {
        return Ok("lerobot-v3");
    }

    let v2_reader = LeRobotV2Reader;
    if v2_reader.can_read(path) {
        return Ok("lerobot-v2");
    }

    Err(ForgeError::FormatDetection {
        path: path.to_path_buf(),
    })
}

/// Get a boxed format reader for the given format name.
pub fn get_reader(format_name: &str) -> ForgeResult<Box<dyn FormatReader>> {
    match format_name {
        "lerobot-v3" => Ok(Box::new(LeRobotV3Reader)),
        "lerobot-v2" => Ok(Box::new(LeRobotV2Reader)),
        _ => Err(ForgeError::UnsupportedFormat {
            format_name: format_name.to_string(),
            available: all_formats().iter().map(|f| f.name.to_string()).collect(),
        }),
    }
}

/// Get a boxed format writer for the given format name.
///
/// Returns a writer with sensible defaults (30 fps, "unknown" robot type).
/// Callers should configure the writer further via the conversion config.
pub fn get_writer(format_name: &str) -> ForgeResult<Box<dyn FormatWriter>> {
    match format_name {
        "lerobot-v3" => Ok(Box::new(LeRobotV3Writer::new(30, "unknown"))),
        _ => Err(ForgeError::UnsupportedFormat {
            format_name: format_name.to_string(),
            available: all_formats()
                .iter()
                .filter(|f| f.can_write)
                .map(|f| f.name.to_string())
                .collect(),
        }),
    }
}
