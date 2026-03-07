//! LeRobot v3 dataset reader.
//!
//! Implements [`FormatReader`] for the LeRobot v3 format. Parses
//! `meta/info.json`, reads Parquet data files via Arrow, and eagerly
//! decodes video frames via `rosetta-video` with batch decoding per chunk.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use arrow::array::RecordBatch;
use bytes::Bytes;
use rayon::prelude::*;

use rosetta_core::{
    CameraInfo, DatasetInfo, Dtype, Episode, FieldSchema, RosettaError, RosettaResult, Frame,
    ImageEncoding, LazyArray, LazyImage, ReadOutput,
};

use crate::traits::FormatReader;

use super::info::{self, InfoJson};
use super::parquet_io::{self, EpisodeData, EpisodeMetadata};

/// Stateless reader for LeRobot v3 datasets.
pub struct LeRobotV3Reader;

impl FormatReader for LeRobotV3Reader {
    fn format_name(&self) -> &str {
        "lerobot-v3"
    }

    fn can_read(&self, path: &Path) -> bool {
        info::is_lerobot_v3(path)
    }

    fn inspect(&self, path: &Path) -> RosettaResult<DatasetInfo> {
        let info = info::read_info(path)?;

        // Build field schemas from non-video data features.
        let fields: Vec<FieldSchema> = info
            .data_features()
            .iter()
            .filter(|(name, _)| {
                // Only include meaningful data features, not index columns.
                !matches!(
                    *name,
                    "frame_index" | "episode_index" | "index" | "timestamp" | "task_index"
                )
            })
            .map(|(name, spec)| {
                let dtype = map_dtype(&spec.dtype);
                let shape = spec.shape.iter().map(|&s| s as usize).collect();
                FieldSchema {
                    name: name.to_string(),
                    shape,
                    dtype,
                    description: None,
                }
            })
            .collect();

        // Build camera infos from video features.
        let cameras: Vec<CameraInfo> = info
            .video_features()
            .iter()
            .map(|(name, spec)| {
                let (height, width, channels) = if let Some(ref vi) = spec.info {
                    (vi.height, vi.width, vi.channels)
                } else if spec.shape.len() >= 3 {
                    (
                        spec.shape[0] as u32,
                        spec.shape[1] as u32,
                        spec.shape[2] as u8,
                    )
                } else {
                    (0, 0, 3)
                };

                CameraInfo {
                    name: name.to_string(),
                    height,
                    width,
                    channels,
                    encoding: "rgb".to_string(),
                }
            })
            .collect();

        // Extra metadata.
        let mut metadata = HashMap::new();
        metadata.insert(
            "chunks_size".to_string(),
            serde_json::json!(info.chunks_size),
        );
        metadata.insert(
            "codebase_version".to_string(),
            serde_json::json!(info.codebase_version),
        );

        Ok(DatasetInfo {
            format: "lerobot-v3".to_string(),
            path: path.to_path_buf(),
            total_episodes: info.total_episodes,
            total_frames: info.total_frames,
            fps: Some(info.fps as f64),
            robot_type: Some(info.robot_type.clone()),
            fields,
            cameras,
            metadata,
        })
    }

    fn read_episodes(&self, path: &Path) -> RosettaResult<ReadOutput<Vec<Episode>>> {
        let info = info::read_info(path)?;
        let episode_metadata = parquet_io::read_episode_metadata(path, &info)?;

        // Read all data Parquet files upfront.
        let all_batches = read_all_data_batches(path, &info)?;

        // Single-pass extraction: group all rows by episode_index at once.
        let all_episode_data = parquet_io::extract_all_episodes_data(&all_batches, &info)?;

        // Collect video feature names and their camera info.
        let video_features: Vec<(String, u32, u32, u8)> = info
            .video_features()
            .iter()
            .map(|(name, spec)| {
                let (h, w, c) = if let Some(ref vi) = spec.info {
                    (vi.height, vi.width, vi.channels)
                } else if spec.shape.len() >= 3 {
                    (
                        spec.shape[0] as u32,
                        spec.shape[1] as u32,
                        spec.shape[2] as u8,
                    )
                } else {
                    (0, 0, 3)
                };
                (name.to_string(), h, w, c)
            })
            .collect();

        let dataset_path = path.to_path_buf();

        // Pre-compute the video frame offset for each episode within its chunk.
        // In LeRobot v3, a chunk's MP4 concatenates all episodes' frames sequentially.
        let episode_video_offsets =
            compute_episode_video_offsets(&episode_metadata, &info);

        // Decode all video frames per (video_key, chunk) in one pass each.
        // This eliminates the O(N^2) per-frame decoder open.
        let video_caches = decode_chunk_videos(
            &dataset_path,
            &info,
            &video_features,
        )?;

        // Wrap shared read-only data for parallel access.
        let all_episode_data = Arc::new(all_episode_data);
        let video_caches = Arc::new(video_caches);
        let episode_video_offsets = Arc::new(episode_video_offsets);
        let info = Arc::new(info);

        // Build all episodes in parallel using rayon.
        let episodes: Vec<RosettaResult<Episode>> = episode_metadata
            .par_iter()
            .map(|meta| {
                build_episode_cached(
                    &dataset_path,
                    &info,
                    meta,
                    &all_episode_data,
                    &video_features,
                    &video_caches,
                    &episode_video_offsets,
                )
            })
            .collect();

        // Collect results, propagating the first error.
        let episodes: Vec<Episode> = episodes
            .into_iter()
            .collect::<RosettaResult<Vec<_>>>()?;

        Ok(ReadOutput::new(episodes))
    }
}

/// Read all data Parquet files across all chunks and file indices.
fn read_all_data_batches(path: &Path, info: &InfoJson) -> RosettaResult<Vec<RecordBatch>> {
    let mut all_batches = Vec::new();

    let num_chunks = if info.total_episodes == 0 {
        0
    } else {
        info.chunk_for_episode(info.total_episodes - 1) + 1
    };

    for chunk_idx in 0..num_chunks {
        for file_idx in 0.. {
            let data_path_rel = info.resolve_data_path(chunk_idx, file_idx);
            let data_path = path.join(&data_path_rel);

            if !data_path.exists() {
                break;
            }

            let batches = parquet_io::read_parquet_file(&data_path)?;
            all_batches.extend(batches);
        }
    }

    Ok(all_batches)
}

/// Compute the video frame offset for each episode within its chunk's MP4.
///
/// In LeRobot v3, a chunk's video concatenates all its episodes' frames
/// sequentially. Episode E's frames start at offset = sum of lengths of
/// all prior episodes in the same chunk.
fn compute_episode_video_offsets(
    episode_metadata: &[EpisodeMetadata],
    info: &InfoJson,
) -> HashMap<i64, usize> {
    let mut offsets = HashMap::with_capacity(episode_metadata.len());
    let mut current_chunk: Option<usize> = None;
    let mut current_offset: usize = 0;

    // episode_metadata is sorted by episode_index.
    for meta in episode_metadata {
        let chunk = info.chunk_for_episode(meta.episode_index as usize);
        if current_chunk != Some(chunk) {
            current_chunk = Some(chunk);
            current_offset = 0;
        }
        offsets.insert(meta.episode_index, current_offset);
        current_offset += meta.length as usize;
    }

    offsets
}

/// Decoded video frame cache: maps video_frame_index -> decoded RGB bytes.
type VideoFrameCache = HashMap<usize, Bytes>;

/// Decode ALL frames from each video file in one pass.
///
/// Scans all file indices per (video_key, chunk) — this handles both:
/// - Per-episode layout (file-000.mp4, file-001.mp4, ...): one video per episode
/// - Concatenated layout (file-000.mp4 only): all episodes in one video
///
/// Returns a map from video file path to its frame cache.
fn decode_chunk_videos(
    dataset_path: &Path,
    info: &InfoJson,
    video_features: &[(String, u32, u32, u8)],
) -> RosettaResult<HashMap<PathBuf, VideoFrameCache>> {
    let mut caches = HashMap::new();

    let num_chunks = if info.total_episodes == 0 {
        0
    } else {
        info.chunk_for_episode(info.total_episodes - 1) + 1
    };

    for (video_key, _, _, _) in video_features {
        for chunk_idx in 0..num_chunks {
            // Scan file indices until no more files are found.
            for file_idx in 0.. {
                let video_rel = info.resolve_video_path(video_key, chunk_idx, file_idx);
                let video_path = dataset_path.join(&video_rel);

                if !video_path.exists() {
                    break;
                }

                // Decode all frames in one sequential pass.
                let decoder = rosetta_video::VideoDecoder::open(&video_path)?;
                let mut cache = HashMap::new();

                for result in decoder {
                    let frame = result?;
                    cache.insert(frame.frame_index, frame.data);
                }

                caches.insert(video_path, cache);
            }
        }
    }

    Ok(caches)
}

/// Build a single Episode from pre-extracted Parquet data and cached video frames.
fn build_episode_cached(
    dataset_path: &Path,
    info: &Arc<InfoJson>,
    meta: &EpisodeMetadata,
    all_episode_data: &Arc<HashMap<i64, EpisodeData>>,
    video_features: &[(String, u32, u32, u8)],
    video_caches: &Arc<HashMap<PathBuf, VideoFrameCache>>,
    episode_video_offsets: &Arc<HashMap<i64, usize>>,
) -> RosettaResult<Episode> {
    let episode_data = all_episode_data
        .get(&meta.episode_index)
        .ok_or_else(|| {
            RosettaError::Schema {
                field_name: "episode_index".to_string(),
                expected: format!("episode {}", meta.episode_index),
                actual: "not found in parquet data".to_string(),
            }
        })?;

    let mut episode = Episode::new(meta.episode_index.to_string());
    episode
        .metadata
        .insert("task_index".to_string(), serde_json::json!(meta.task_index));

    let num_frames = episode_data.frame_indices.len();

    // Get this episode's video frame offset within its chunk's MP4.
    let video_offset = episode_video_offsets
        .get(&meta.episode_index)
        .copied()
        .unwrap_or(0);

    // Pre-resolve video cache references for each video feature (constant per episode).
    // Supports two layouts:
    //   - Per-episode: file-{ep_offset:03}.mp4 exists → frame_index maps directly
    //   - Concatenated: only file-000.mp4 exists → need video_offset + frame_index
    let chunk_idx = info.chunk_for_episode(meta.episode_index as usize);
    let first_ep_in_chunk = chunk_idx * info.chunks_size;
    let ep_offset_in_chunk = meta.episode_index as usize - first_ep_in_chunk;

    struct ResolvedVideo<'a> {
        video_key: &'a str,
        height: u32,
        width: u32,
        channels: u8,
        cache: &'a VideoFrameCache,
        frame_offset: usize,
    }

    let resolved_videos: Vec<ResolvedVideo<'_>> = video_features
        .iter()
        .filter_map(|(video_key, h, w, c)| {
            // Try per-episode file first.
            let per_ep_path = dataset_path
                .join(info.resolve_video_path(video_key, chunk_idx, ep_offset_in_chunk));
            if let Some(cache) = video_caches.get(&per_ep_path) {
                return Some(ResolvedVideo {
                    video_key: video_key.as_str(),
                    height: *h,
                    width: *w,
                    channels: *c,
                    cache,
                    frame_offset: 0, // Per-episode: no offset needed.
                });
            }

            // Fall back to concatenated file (file-000.mp4).
            let concat_path =
                dataset_path.join(info.resolve_video_path(video_key, chunk_idx, 0));
            video_caches.get(&concat_path).map(|cache| ResolvedVideo {
                video_key: video_key.as_str(),
                height: *h,
                width: *w,
                channels: *c,
                cache,
                frame_offset: video_offset, // Concatenated: offset by prior episodes' lengths.
            })
        })
        .collect();

    for frame_idx in 0..num_frames {
        let mut frame = Frame::new(episode_data.frame_indices[frame_idx] as usize);

        // Store timestamp in frame metadata.
        frame.metadata.insert(
            "timestamp".to_string(),
            serde_json::json!(episode_data.timestamps[frame_idx]),
        );

        // Set action array if present.
        if let Some(action_vecs) = episode_data.fields.get("action") {
            if frame_idx < action_vecs.len() {
                let action_f64: Vec<f64> =
                    action_vecs[frame_idx].iter().map(|&v| v as f64).collect();
                let shape = vec![action_f64.len()];
                frame.action = Some(LazyArray::from_vec(action_f64, shape, Dtype::Float32));
            }
        }

        // Set state array if present.
        if let Some(state_vecs) = episode_data.fields.get("observation.state") {
            if frame_idx < state_vecs.len() {
                let state_f64: Vec<f64> =
                    state_vecs[frame_idx].iter().map(|&v| v as f64).collect();
                let shape = vec![state_f64.len()];
                frame.state = Some(LazyArray::from_vec(state_f64, shape, Dtype::Float32));
            }
        }

        // Set eagerly-decoded video images from cache.
        for rv in &resolved_videos {
            let video_frame_index =
                rv.frame_offset + episode_data.frame_indices[frame_idx] as usize;

            let frame_data = rv
                .cache
                .get(&video_frame_index)
                .ok_or_else(|| {
                    RosettaError::Video(format!(
                        "frame {} not found in video cache for {}",
                        video_frame_index, rv.video_key
                    ))
                })?;

            let image = LazyImage::from_bytes(
                frame_data.clone(),
                rv.height,
                rv.width,
                rv.channels,
                ImageEncoding::Raw,
            );

            frame.images.insert(rv.video_key.to_string(), image);
        }

        episode.frames.push(frame);
    }

    Ok(episode)
}

/// Map a LeRobot dtype string to Rosetta's Dtype enum.
fn map_dtype(dtype_str: &str) -> Dtype {
    match dtype_str {
        "float32" => Dtype::Float32,
        "float64" => Dtype::Float64,
        "int32" => Dtype::Int32,
        "int64" => Dtype::Int64,
        "uint8" => Dtype::Uint8,
        "bool" => Dtype::Bool,
        "string" => Dtype::String,
        _ => Dtype::Float32, // Default fallback.
    }
}

/// Create a `PathBuf` for the video file of a given video key, chunk, and file index.
#[allow(dead_code)]
fn video_file_path(
    dataset_path: &Path,
    info: &InfoJson,
    video_key: &str,
    chunk_index: usize,
    file_index: usize,
) -> PathBuf {
    let rel = info.resolve_video_path(video_key, chunk_index, file_index);
    dataset_path.join(rel)
}
