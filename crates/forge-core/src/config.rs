//! Conversion configuration types.
//!
//! [`ConversionConfig`] controls how datasets are converted between formats.
//! It is deserializable from YAML files via serde, with sensible defaults
//! matching the locked decisions (H.264 CRF 23, preset medium, codec h264).

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{ForgeError, ForgeResult};

/// Mapping rule for a single data field during conversion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldMapping {
    /// Source field name in the input dataset.
    pub source: String,
    /// Target field name in the output dataset.
    pub target: String,
    /// Optional transform to apply (e.g., "normalize", "clip").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transform: Option<String>,
}

/// Configuration for a dataset format conversion.
///
/// All fields have sensible defaults and can be overridden via YAML config
/// files. Video encoding defaults match the locked decisions from the
/// Python Forge implementation: libx264, CRF 23, preset medium.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversionConfig {
    /// Source format (auto-detected if not specified).
    pub source_format: Option<String>,
    /// Target format (required for conversion).
    pub target_format: Option<String>,

    /// Camera name remapping: input name -> output name.
    #[serde(default)]
    pub cameras: HashMap<String, String>,

    /// Field name remapping with optional transforms.
    #[serde(default)]
    pub fields: HashMap<String, FieldMapping>,

    /// Name of the action field in the source dataset.
    pub action_field: Option<String>,
    /// Name of the state/observation field in the source dataset.
    pub state_field: Option<String>,

    /// Frames per second (overrides source FPS if set).
    pub fps: Option<f64>,
    /// Robot type identifier (e.g., "panda", "ur5").
    pub robot_type: Option<String>,

    /// Video codec for output (default: "h264").
    #[serde(default = "default_codec")]
    pub video_codec: String,
    /// Constant Rate Factor for H.264 encoding (default: 23).
    #[serde(default = "default_crf")]
    pub video_crf: u8,
    /// Encoding speed preset (default: "medium").
    #[serde(default = "default_preset")]
    pub video_preset: String,

    /// If true, abort on first episode error instead of continuing.
    #[serde(default)]
    pub fail_on_error: bool,
    /// If true, skip episodes that already exist in the output (default: true).
    #[serde(default = "default_true")]
    pub skip_existing: bool,

    /// If true, encode videos with compression (default: true).
    #[serde(default = "default_true")]
    pub compress_videos: bool,
    /// If true, include depth camera channels in output (default: true).
    #[serde(default = "default_true")]
    pub include_depth: bool,

    /// Number of worker threads for parallel processing (default: 0 = auto-detect).
    #[serde(default = "default_workers")]
    pub num_workers: usize,

    /// If true, produce bit-for-bit identical output (opt-in, uses parallel processing with sorted merge).
    #[serde(default)]
    pub deterministic: bool,

    /// Suppress all progress output (only errors printed to stderr).
    #[serde(default)]
    pub quiet: bool,

    /// Output a single final JSON blob with all results instead of progress bar.
    #[serde(default)]
    pub json_output: bool,

    /// Enable per-episode verbose log lines.
    #[serde(default)]
    pub verbose: bool,
}

fn default_codec() -> String {
    "h264".into()
}
fn default_crf() -> u8 {
    23
}
fn default_preset() -> String {
    "medium".into()
}
fn default_true() -> bool {
    true
}
fn default_workers() -> usize {
    0
}

impl Default for ConversionConfig {
    fn default() -> Self {
        Self {
            source_format: None,
            target_format: None,
            cameras: HashMap::new(),
            fields: HashMap::new(),
            action_field: None,
            state_field: None,
            fps: None,
            robot_type: None,
            video_codec: default_codec(),
            video_crf: default_crf(),
            video_preset: default_preset(),
            fail_on_error: false,
            skip_existing: true,
            compress_videos: true,
            include_depth: true,
            num_workers: default_workers(),
            deterministic: false,
            quiet: false,
            json_output: false,
            verbose: false,
        }
    }
}

impl ConversionConfig {
    /// Load a conversion config from a YAML file.
    pub fn from_yaml(path: &Path) -> ForgeResult<Self> {
        let contents = std::fs::read_to_string(path)?;
        serde_yaml_ng::from_str(&contents).map_err(|e| ForgeError::Config(e.to_string()))
    }
}
