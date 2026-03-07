//! Dataset schema types: field descriptors, camera info, and dataset metadata.
//!
//! These types describe the structure of a dataset without holding the actual
//! data. They are used for inspection, validation, and format conversion
//! planning.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Data types supported by Rosetta.
///
/// Maps directly to Python Rosetta's Dtype enum. Used in [`FieldSchema`] to
/// describe the element type of array fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Dtype {
    Float32,
    Float64,
    Int32,
    Int64,
    Uint8,
    Bool,
    String,
}

/// Schema for a single field in the dataset.
///
/// Describes the name, shape, element type, and optional description of
/// a data field (e.g., "action", "observation/joint_positions").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldSchema {
    pub name: String,
    pub shape: Vec<usize>,
    pub dtype: Dtype,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Metadata about a camera stream in the dataset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CameraInfo {
    pub name: String,
    pub height: u32,
    pub width: u32,
    #[serde(default = "default_channels")]
    pub channels: u8,
    #[serde(default = "default_encoding")]
    pub encoding: String,
}

fn default_channels() -> u8 {
    3
}

fn default_encoding() -> String {
    "rgb".into()
}

/// Top-level metadata describing an entire dataset.
///
/// Produced by format readers during inspection and used for conversion
/// planning. Contains structural information (episode/frame counts, schema)
/// but not the actual data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatasetInfo {
    pub format: String,
    pub path: PathBuf,
    pub total_episodes: usize,
    pub total_frames: usize,
    pub fps: Option<f64>,
    pub robot_type: Option<String>,
    pub fields: Vec<FieldSchema>,
    pub cameras: Vec<CameraInfo>,
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,
}
