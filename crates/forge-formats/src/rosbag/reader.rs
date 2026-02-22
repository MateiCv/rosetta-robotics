//! Unified ROS bag format reader and detector.
//!
//! Provides [`RosbagReader`] implementing [`FormatReader`] for both ROS 1
//! `.bag` files and ROS 2 MCAP files, plus [`RosbagDetector`] implementing
//! [`FormatDetector`] for scored format detection.
//!
//! Sub-format is auto-detected from file extension and magic bytes:
//! - `.bag` -> ROS 1 (via `rosbag` crate)
//! - `.mcap` -> ROS 2 MCAP (via `mcap` crate)
//! - `.db3` -> ROS 2 SQLite (warning: conversion to MCAP suggested)

use std::path::Path;

use forge_core::{DatasetInfo, ForgeResult, ReadOutput};

use crate::detection::{DetectionResult, FormatDetector};
use crate::traits::FormatReader;

/// Stub: full implementation in Task 2.
pub struct RosbagReader;

/// Stub: full implementation in Task 2.
pub struct RosbagDetector;
