//! Episode type representing a single demonstration or trajectory.
//!
//! An episode is an ordered sequence of frames captured during one robot
//! interaction. Episode IDs are strings (not integers) to preserve
//! compatibility with all dataset formats -- some use numeric IDs
//! (e.g., "000042"), others use descriptive names (e.g., "episode_0").

use std::collections::HashMap;

use crate::frame::Frame;

/// A single episode (demonstration/trajectory) in a dataset.
///
/// Contains an ordered sequence of [`Frame`]s and episode-level metadata.
/// The `episode_id` is a string to accommodate all format conventions.
#[derive(Debug)]
pub struct Episode {
    /// String identifier for the episode. Preserves format-native IDs.
    pub episode_id: String,
    /// Ordered sequence of frames (timesteps) in this episode.
    pub frames: Vec<Frame>,
    /// Arbitrary episode-level metadata (e.g., task description, success flag).
    pub metadata: HashMap<String, serde_json::Value>,
}

impl Episode {
    /// Create a new empty episode with the given ID.
    pub fn new(episode_id: impl Into<String>) -> Self {
        Self {
            episode_id: episode_id.into(),
            frames: Vec::new(),
            metadata: HashMap::new(),
        }
    }

    /// Number of frames in this episode.
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    /// Whether this episode has no frames.
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// Alias for [`len`](Self::len) -- number of frames in this episode.
    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    /// Try to parse the episode ID as a numeric index.
    ///
    /// Returns `Some(n)` if the episode ID is purely numeric (e.g., "42")
    /// or follows common patterns like "episode_42". Returns `None` for
    /// non-numeric IDs.
    pub fn index(&self) -> Option<u64> {
        // Try direct parse first.
        if let Ok(n) = self.episode_id.parse::<u64>() {
            return Some(n);
        }
        // Try extracting trailing digits after common prefixes.
        let id = &self.episode_id;
        for prefix in &["episode_", "ep_", "ep"] {
            if let Some(suffix) = id.strip_prefix(prefix) {
                if let Ok(n) = suffix.parse::<u64>() {
                    return Some(n);
                }
            }
        }
        None
    }
}
