//! Frame type representing a single timestep in an episode.
//!
//! A frame is the atomic unit of a robotics dataset: one timestep containing
//! camera images, robot state, robot action, and arbitrary metadata. Images
//! and arrays are lazily loaded to avoid reading unnecessary data.

use std::collections::HashMap;

use crate::lazy::{LazyArray, LazyImage};

/// A single timestep within an episode.
///
/// Contains lazily-loaded images and numeric arrays, plus arbitrary metadata.
/// The `timestep` field is the zero-based index within the episode.
#[derive(Debug)]
pub struct Frame {
    /// Zero-based timestep index within the episode.
    pub timestep: usize,
    /// Camera images keyed by camera name (e.g., "top", "wrist").
    pub images: HashMap<String, LazyImage>,
    /// Robot state observation (e.g., joint positions).
    pub state: Option<LazyArray>,
    /// Robot action (e.g., joint velocities or end-effector deltas).
    pub action: Option<LazyArray>,
    /// Arbitrary per-frame metadata.
    pub metadata: HashMap<String, serde_json::Value>,
}

impl Frame {
    /// Create a new empty frame at the given timestep.
    pub fn new(timestep: usize) -> Self {
        Self {
            timestep,
            images: HashMap::new(),
            state: None,
            action: None,
            metadata: HashMap::new(),
        }
    }

    /// Whether this frame has an image from the given camera.
    pub fn has_image(&self, camera_name: &str) -> bool {
        self.images.contains_key(camera_name)
    }

    /// Returns the names of all cameras that have images in this frame.
    pub fn image_names(&self) -> Vec<&str> {
        self.images.keys().map(|s| s.as_str()).collect()
    }
}
