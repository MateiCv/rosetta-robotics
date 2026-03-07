//! Static format capability registry.
//!
//! Provides a compile-time list of all supported dataset formats and their
//! read/write capabilities. New formats are added here as they are implemented
//! in later phases.

use std::path::Path;

use rosetta_core::{RosettaError, RosettaResult};

use crate::groot::reader::GR00TReader;
#[cfg(feature = "hdf5-format")]
use crate::hdf5::HDF5Reader;
#[cfg(feature = "hdf5-format")]
use crate::hdf5::HDF5Writer;
use crate::lerobot_v2::reader::LeRobotV2Reader;
use crate::lerobot_v2::LeRobotV2Writer;
use crate::lerobot_v3::reader::LeRobotV3Reader;
use crate::lerobot_v3::writer::LeRobotV3Writer;
#[cfg(feature = "rlds-format")]
use crate::rlds::RLDSReader;
#[cfg(feature = "rlds-format")]
use crate::rlds::RLDSWriter;
#[cfg(feature = "robodm-format")]
use crate::robodm::RoboDMReader;
#[cfg(feature = "robodm-format")]
use crate::robodm::RoboDMWriter;
#[cfg(feature = "rosbag-format")]
use crate::rosbag::RosbagReader;
#[cfg(feature = "rosbag-format")]
use crate::rosbag::RosbagWriter;
use crate::traits::{FormatReader, FormatWriter};
#[cfg(feature = "zarr-format")]
use crate::zarr::ZarrReader;
#[cfg(feature = "zarr-format")]
use crate::zarr::ZarrWriter;

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

/// All dataset formats known to Rosetta.
///
/// Returns a static slice of [`FormatCapability`] entries covering all 8
/// supported robotics dataset formats. Formats behind optional feature
/// flags are always listed (for user-facing display) but require the
/// corresponding feature to be enabled for actual reading.
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
            can_write: true,
            description: "LeRobot v2 (per-episode Parquet + MP4 video)",
        },
        FormatCapability {
            name: "groot",
            can_read: true,
            can_write: false,
            description: "GR00T / NVIDIA Isaac (LeRobot v2 with NVIDIA markers, read-only)",
        },
        FormatCapability {
            name: "hdf5",
            can_read: true,
            can_write: true,
            description: "HDF5 (robomimic, ACT/ALOHA)",
        },
        FormatCapability {
            name: "zarr",
            can_read: true,
            can_write: true,
            description: "Zarr v2 (Diffusion Policy, UMI)",
        },
        FormatCapability {
            name: "rlds",
            can_read: true,
            can_write: true,
            description: "RLDS/TFRecord (Open-X Embodiment)",
        },
        FormatCapability {
            name: "rosbag",
            can_read: true,
            can_write: true,
            description: "ROS bags (ROS 1 .bag read, ROS 2 MCAP read + write)",
        },
        FormatCapability {
            name: "robodm",
            can_read: true,
            can_write: true,
            description: "RoboDM (Berkeley .vla)",
        },
    ];
    FORMATS
}

/// Attempt to auto-detect the dataset format at `path`.
///
/// Delegates to the scored detection engine ([`detect_format_scored`]) and
/// returns the format name of the highest-confidence match. The scored
/// system handles ambiguity warnings automatically.
pub fn detect_format(path: &Path) -> RosettaResult<&'static str> {
    let results = crate::detection::detect_format_scored(path)?;
    Ok(results[0].format_name)
}

/// Get a boxed format reader for the given format name.
pub fn get_reader(format_name: &str) -> RosettaResult<Box<dyn FormatReader>> {
    match format_name {
        "lerobot-v3" => Ok(Box::new(LeRobotV3Reader)),
        "lerobot-v2" => Ok(Box::new(LeRobotV2Reader)),
        "groot" => Ok(Box::new(GR00TReader::new())),
        #[cfg(feature = "hdf5-format")]
        "hdf5" => Ok(Box::new(HDF5Reader)),
        #[cfg(feature = "zarr-format")]
        "zarr" => Ok(Box::new(ZarrReader)),
        #[cfg(feature = "rlds-format")]
        "rlds" => Ok(Box::new(RLDSReader)),
        #[cfg(feature = "rosbag-format")]
        "rosbag" => Ok(Box::new(RosbagReader)),
        #[cfg(feature = "robodm-format")]
        "robodm" => Ok(Box::new(RoboDMReader)),
        _ => Err(RosettaError::UnsupportedFormat {
            format_name: format_name.to_string(),
            available: all_formats().iter().map(|f| f.name.to_string()).collect(),
        }),
    }
}

/// Get a boxed format writer for the given format name.
///
/// Returns a writer with sensible defaults (30 fps, "unknown" robot type).
/// Callers should configure the writer further via the conversion config.
pub fn get_writer(format_name: &str) -> RosettaResult<Box<dyn FormatWriter>> {
    match format_name {
        "lerobot-v3" => Ok(Box::new(LeRobotV3Writer::new(30, "unknown"))),
        "lerobot-v2" => Ok(Box::new(LeRobotV2Writer::new(30, "unknown"))),
        #[cfg(feature = "rlds-format")]
        "rlds" => Ok(Box::new(RLDSWriter::new())),
        #[cfg(feature = "hdf5-format")]
        "hdf5" => Ok(Box::new(HDF5Writer::new())),
        #[cfg(feature = "zarr-format")]
        "zarr" => Ok(Box::new(ZarrWriter::new())),
        #[cfg(feature = "rosbag-format")]
        "rosbag" => Ok(Box::new(RosbagWriter::new())),
        #[cfg(feature = "robodm-format")]
        "robodm" => Ok(Box::new(RoboDMWriter::new())),
        _ => Err(RosettaError::UnsupportedFormat {
            format_name: format_name.to_string(),
            available: all_formats()
                .iter()
                .filter(|f| f.can_write)
                .map(|f| f.name.to_string())
                .collect(),
        }),
    }
}
