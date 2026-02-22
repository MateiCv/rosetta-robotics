//! Parquet I/O helpers for LeRobot v3 datasets.
//!
//! Reads and writes Parquet data files and episode metadata using Arrow
//! RecordBatches, with proper type handling for FixedSizeList, Int64,
//! and Float32 columns.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use arrow::array::{
    Array, AsArray, Float32Array, Int64Array, RecordBatch,
};
use arrow::datatypes::{DataType, Field, Schema};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::arrow::ArrowWriter;
use parquet::file::properties::WriterProperties;

use forge_core::{ForgeError, ForgeResult};

use super::info::InfoJson;

/// Metadata for a single episode (from `meta/episodes/` Parquet files).
#[derive(Debug, Clone)]
pub struct EpisodeMetadata {
    pub episode_index: i64,
    pub length: i64,
    pub task_index: i64,
}

/// Extracted per-episode data from the main data Parquet files.
#[derive(Debug)]
pub struct EpisodeData {
    /// Zero-based frame index within the episode.
    pub frame_indices: Vec<i64>,
    /// Global frame index across the entire dataset.
    pub global_indices: Vec<i64>,
    /// Timestamps in seconds.
    pub timestamps: Vec<f32>,
    /// Task index for each frame.
    pub task_indices: Vec<i64>,
    /// Named data fields (action, observation.state, etc.) as f32 vectors.
    /// Each entry maps feature_name -> Vec of per-frame vectors.
    pub fields: HashMap<String, Vec<Vec<f32>>>,
}

/// Read a Parquet file into a vector of Arrow RecordBatches.
pub fn read_parquet_file(path: &Path) -> ForgeResult<Vec<RecordBatch>> {
    let file = std::fs::File::open(path).map_err(ForgeError::Io)?;

    let builder = ParquetRecordBatchReaderBuilder::try_new(file).map_err(|e| {
        ForgeError::Inspection {
            path: path.to_path_buf(),
            reason: format!("failed to create Parquet reader: {e}"),
        }
    })?;

    let reader = builder.build().map_err(|e| ForgeError::Inspection {
        path: path.to_path_buf(),
        reason: format!("failed to build Parquet reader: {e}"),
    })?;

    let mut batches = Vec::new();
    for batch_result in reader {
        let batch = batch_result.map_err(|e| ForgeError::Inspection {
            path: path.to_path_buf(),
            reason: format!("failed to read Parquet batch: {e}"),
        })?;
        batches.push(batch);
    }

    Ok(batches)
}

/// Read episode metadata from the `meta/episodes/` Parquet files.
///
/// Scans `meta/episodes/chunk-XXX/file-XXX.parquet` for all chunks
/// and returns an `EpisodeMetadata` for each episode found.
pub fn read_episode_metadata(
    dataset_path: &Path,
    info: &InfoJson,
) -> ForgeResult<Vec<EpisodeMetadata>> {
    let mut all_meta = Vec::new();

    // Iterate over chunk directories that should exist based on total_episodes.
    let num_chunks = if info.total_episodes == 0 {
        0
    } else {
        info.chunk_for_episode(info.total_episodes - 1) + 1
    };

    for chunk_idx in 0..num_chunks {
        let meta_path = dataset_path
            .join("meta")
            .join("episodes")
            .join(format!("chunk-{chunk_idx:03}"))
            .join("file-000.parquet");

        if !meta_path.exists() {
            continue;
        }

        let batches = read_parquet_file(&meta_path)?;

        for batch in &batches {
            let ep_col = get_i64_column(batch, "episode_index")?;
            let len_col = get_i64_column(batch, "length")?;
            let task_col = get_i64_column(batch, "task_index")?;

            for row in 0..batch.num_rows() {
                all_meta.push(EpisodeMetadata {
                    episode_index: ep_col.value(row),
                    length: len_col.value(row),
                    task_index: task_col.value(row),
                });
            }
        }
    }

    // Sort by episode index for deterministic iteration.
    all_meta.sort_by_key(|m| m.episode_index);

    Ok(all_meta)
}

/// Extract per-episode data from RecordBatches, filtering rows where
/// `episode_index` matches the requested episode.
pub fn extract_episode_data(
    batches: &[RecordBatch],
    episode_index: i64,
    info: &InfoJson,
) -> ForgeResult<EpisodeData> {
    let mut frame_indices = Vec::new();
    let mut global_indices = Vec::new();
    let mut timestamps = Vec::new();
    let mut task_indices = Vec::new();
    let mut fields: HashMap<String, Vec<Vec<f32>>> = HashMap::new();

    // Identify non-video, non-index features that are stored as FixedSizeList.
    let data_feature_names: Vec<String> = info
        .data_features()
        .iter()
        .filter(|(name, spec)| {
            // Skip scalar index/control columns -- those are handled directly.
            !matches!(
                *name,
                "frame_index" | "episode_index" | "index" | "timestamp" | "task_index"
            ) && spec.dtype == "float32"
        })
        .map(|(name, _)| name.to_string())
        .collect();

    for batch in batches {
        let ep_col = get_i64_column(batch, "episode_index")?;

        // Build row mask for the target episode.
        let matching_rows: Vec<usize> = (0..batch.num_rows())
            .filter(|&row| ep_col.value(row) == episode_index)
            .collect();

        if matching_rows.is_empty() {
            continue;
        }

        // Extract scalar columns.
        let fi_col = get_i64_column(batch, "frame_index")?;
        let idx_col = get_i64_column(batch, "index")?;
        let ts_col = get_f32_column(batch, "timestamp")?;
        let ti_col = get_i64_column(batch, "task_index")?;

        for &row in &matching_rows {
            frame_indices.push(fi_col.value(row));
            global_indices.push(idx_col.value(row));
            timestamps.push(ts_col.value(row));
            task_indices.push(ti_col.value(row));
        }

        // Extract FixedSizeList data features.
        for feat_name in &data_feature_names {
            let col_idx = match batch.schema().index_of(feat_name) {
                Ok(idx) => idx,
                Err(_) => continue,
            };
            let col = batch.column(col_idx);
            let feat_data = fields.entry(feat_name.clone()).or_default();

            match col.data_type() {
                DataType::FixedSizeList(_, list_size) => {
                    let list_size = *list_size as usize;
                    let list_array = col.as_fixed_size_list();
                    let values = list_array.values();
                    let float_values = values
                        .as_any()
                        .downcast_ref::<Float32Array>()
                        .ok_or_else(|| ForgeError::Schema {
                            field_name: feat_name.clone(),
                            expected: "Float32".to_string(),
                            actual: format!("{:?}", values.data_type()),
                        })?;

                    for &row in &matching_rows {
                        let offset = row * list_size;
                        let vec: Vec<f32> = (0..list_size)
                            .map(|i| float_values.value(offset + i))
                            .collect();
                        feat_data.push(vec);
                    }
                }
                other => {
                    return Err(ForgeError::Schema {
                        field_name: feat_name.clone(),
                        expected: "FixedSizeList<Float32>".to_string(),
                        actual: format!("{other:?}"),
                    });
                }
            }
        }
    }

    Ok(EpisodeData {
        frame_indices,
        global_indices,
        timestamps,
        task_indices,
        fields,
    })
}

/// Helper: get an Int64 column by name from a RecordBatch.
fn get_i64_column<'a>(batch: &'a RecordBatch, name: &str) -> ForgeResult<&'a Int64Array> {
    let col_idx = batch.schema().index_of(name).map_err(|_| ForgeError::Schema {
        field_name: name.to_string(),
        expected: "column present".to_string(),
        actual: "column missing".to_string(),
    })?;

    batch
        .column(col_idx)
        .as_any()
        .downcast_ref::<Int64Array>()
        .ok_or_else(|| ForgeError::Schema {
            field_name: name.to_string(),
            expected: "Int64".to_string(),
            actual: format!("{:?}", batch.column(col_idx).data_type()),
        })
}

/// Helper: get a Float32 column by name from a RecordBatch.
fn get_f32_column<'a>(batch: &'a RecordBatch, name: &str) -> ForgeResult<&'a Float32Array> {
    let col_idx = batch.schema().index_of(name).map_err(|_| ForgeError::Schema {
        field_name: name.to_string(),
        expected: "column present".to_string(),
        actual: "column missing".to_string(),
    })?;

    batch
        .column(col_idx)
        .as_any()
        .downcast_ref::<Float32Array>()
        .ok_or_else(|| ForgeError::Schema {
            field_name: name.to_string(),
            expected: "Float32".to_string(),
            actual: format!("{:?}", batch.column(col_idx).data_type()),
        })
}

// ---- Write helpers ----

/// Write an Arrow RecordBatch to a Parquet file, creating parent directories.
pub fn write_parquet_file(path: &Path, batch: &RecordBatch) -> ForgeResult<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(ForgeError::Io)?;
    }

    let file = std::fs::File::create(path).map_err(ForgeError::Io)?;
    let props = WriterProperties::builder().build();
    let mut writer =
        ArrowWriter::try_new(file, batch.schema(), Some(props)).map_err(|e| {
            ForgeError::Inspection {
                path: path.to_path_buf(),
                reason: format!("failed to create Parquet writer: {e}"),
            }
        })?;

    writer.write(batch).map_err(|e| ForgeError::Inspection {
        path: path.to_path_buf(),
        reason: format!("failed to write Parquet batch: {e}"),
    })?;

    writer.close().map_err(|e| ForgeError::Inspection {
        path: path.to_path_buf(),
        reason: format!("failed to close Parquet writer: {e}"),
    })?;

    Ok(())
}

/// Build the Arrow Schema for data Parquet files with FixedSizeList types.
pub fn build_data_schema(action_dim: i32, state_dim: i32) -> Schema {
    Schema::new(vec![
        Field::new("episode_index", DataType::Int64, false),
        Field::new("frame_index", DataType::Int64, false),
        Field::new("index", DataType::Int64, false),
        Field::new("timestamp", DataType::Float32, false),
        Field::new("task_index", DataType::Int64, false),
        Field::new(
            "action",
            DataType::FixedSizeList(
                Arc::new(Field::new("item", DataType::Float32, true)),
                action_dim,
            ),
            false,
        ),
        Field::new(
            "observation.state",
            DataType::FixedSizeList(
                Arc::new(Field::new("item", DataType::Float32, true)),
                state_dim,
            ),
            false,
        ),
    ])
}

/// Build the Arrow Schema for episode metadata Parquet files.
pub fn build_episode_metadata_schema() -> Schema {
    Schema::new(vec![
        Field::new("episode_index", DataType::Int64, false),
        Field::new("length", DataType::Int64, false),
        Field::new("task_index", DataType::Int64, false),
    ])
}
