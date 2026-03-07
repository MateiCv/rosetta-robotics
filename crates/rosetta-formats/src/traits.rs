//! Format reader and writer trait definitions.
//!
//! All format implementations (LeRobot v3, RLDS, HDF5, Zarr, ROS bags, etc.)
//! must implement [`FormatReader`] and/or [`FormatWriter`] to participate in
//! the Rosetta conversion pipeline.

use std::path::Path;

use rosetta_core::{DatasetInfo, Episode, RosettaResult, ReadOutput};

/// Reads episodes from a dataset in a specific format.
///
/// Implementors convert from an on-disk format into the canonical
/// [`Episode`] representation. The reader is `Send + Sync` to support
/// concurrent use in the pipeline.
pub trait FormatReader: Send + Sync {
    /// Human-readable name of the format (e.g., "lerobot-v3").
    fn format_name(&self) -> &str;

    /// Returns `true` if `path` appears to be a valid dataset in this format.
    ///
    /// Used by format auto-detection. Implementations should perform a cheap
    /// filesystem check (e.g., look for characteristic files or directory
    /// layout) without reading full data.
    fn can_read(&self, path: &Path) -> bool;

    /// Inspect the dataset at `path` and return structural metadata.
    ///
    /// Does not read episode data -- only schema, counts, camera info, etc.
    fn inspect(&self, path: &Path) -> RosettaResult<DatasetInfo>;

    /// Read all episodes from the dataset at `path`.
    ///
    /// Returns a [`ReadOutput`] containing the successfully read episodes and
    /// any warnings collected during reading (skipped episodes, truncated
    /// data, etc.). Per user decision: lenient by default -- skip broken
    /// episodes, collect warnings, show summary after reading completes.
    fn read_episodes(&self, path: &Path) -> RosettaResult<ReadOutput<Vec<Episode>>>;
}

/// Writes episodes to a dataset in a specific format.
///
/// Implementors convert from the canonical [`Episode`] representation into
/// an on-disk format. The writer is `Send + Sync` to support concurrent use
/// in the pipeline.
pub trait FormatWriter: Send + Sync {
    /// Human-readable name of the format (e.g., "lerobot-v3").
    fn format_name(&self) -> &str;

    /// Write a single episode to the output dataset.
    ///
    /// `episode_index` is the zero-based position of this episode in the
    /// output, which formats may use for file naming (e.g., episode_000042).
    fn write_episode(
        &mut self,
        episode: &Episode,
        output_path: &Path,
        episode_index: usize,
    ) -> RosettaResult<()>;

    /// Finalize the dataset after all episodes have been written.
    ///
    /// Writes any top-level metadata files, indices, or manifests required
    /// by the output format (e.g., `info.json`, `meta/info.json`).
    fn finalize(&mut self, output_path: &Path, dataset_info: &DatasetInfo) -> RosettaResult<()>;

    /// Set the FPS (frames per second) for the output dataset.
    ///
    /// Called by the conversion pipeline to propagate the source dataset's FPS
    /// to the writer. Default implementation is a no-op for formats that do not
    /// store FPS metadata.
    fn set_fps(&mut self, _fps: f64) {
        // Default: no-op. Formats that store FPS override this.
    }

    /// Set the robot type for the output dataset.
    ///
    /// Called by the conversion pipeline to propagate the source dataset's
    /// robot type to the writer. Default implementation is a no-op for formats
    /// that do not store robot type metadata.
    fn set_robot_type(&mut self, _robot_type: &str) {
        // Default: no-op. Formats that store robot type override this.
    }

    /// Enable deterministic mode for bit-identical output.
    ///
    /// When enabled, writers should use single-threaded encoding and sort
    /// all HashMap keys before serialization. Default implementation is a
    /// no-op for formats that do not support deterministic output.
    fn set_deterministic(&mut self, _deterministic: bool) {
        // Default: no-op. Formats that support deterministic mode override this.
    }

    /// Set the chunk size (episodes per chunk) for the output dataset.
    ///
    /// Called by the conversion pipeline to propagate the source dataset's
    /// chunk size. Default implementation is a no-op for formats that do not
    /// use chunked file layouts.
    fn set_chunks_size(&mut self, _chunks_size: usize) {
        // Default: no-op. Formats that use chunks override this.
    }
}
