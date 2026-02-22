//! GR00T (NVIDIA Isaac) format reader.
//!
//! GR00T datasets are structurally identical to LeRobot v2 but contain
//! NVIDIA-specific metadata markers (robot_type identifiers, annotation
//! fields). This module provides a thin wrapper around the v2 reader
//! with GR00T-specific detection and metadata handling.

pub mod reader;

pub use reader::GR00TDetector;
pub use reader::GR00TReader;
